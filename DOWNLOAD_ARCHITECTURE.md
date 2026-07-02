# Architettura Download

> Fonte di verità architetturale: hub anti-drift
> (`architecture/docs/architecture/church-helper-desktop.md`, code_ref `arch-desktop`).
> Questo file è il dettaglio operativo del sottosistema download/queue/errata; in caso
> di conflitto vince il hub.

## Obiettivo

Tutti i download (manuali e auto-download) passano da un'unica `DownloadQueue` (adr-0007:
vietato reintrodurre un percorso di download diretto). Modalità sequenziale (`Queue`, 1
alla volta) o parallela (`Parallel`, fino a 4), priorità per i manuali, resume su `.part`,
pausa/cancellazione cooperative, registro dei file scaricati per rilevare le "errata
corrige" (risorse ripubblicate).

## Componenti

| File | Ruolo |
|---|---|
| `src-tauri/src/services/queue.rs` | `DownloadQueue`: coda + worker singleton |
| `src-tauri/src/services/download.rs` | `DownloadService`: fetch HTTP, resume, sha256, path safety |
| `src-tauri/src/commands.rs` | comandi IPC: `download_resource`, `pause_download`, `cancel_download` |
| `src-tauri/src/services/errata.rs` | registro `downloaded_files` + errata corrige |
| `src-tauri/src/lib.rs` | bootstrap: carica registro da `cache.json`, scan a startup |
| `src/stores/appStore.ts` | unico consumer degli eventi backend; UI "dumb" |

### DownloadQueue (`queue.rs`)

Stato (dietro `Arc`, lock `tokio::sync::Mutex`): `queue: VecDeque<Resource>`;
`active_count: AtomicUsize`; `active_ids: Vec<i64>` (anche nel payload evento);
`active_weeks: HashMap<i64, WeekIdentifier>` (usata da `weeks_with_pending_downloads()`
per non archiviare una settimana con download in corso); `mode: DownloadMode`
(`Queue`|`Parallel`, serializzato come stringa); `worker_started: AtomicBool` (avvia il
worker una sola volta).

Enqueue (funzioni pure testate isolatamente `can_enqueue`/`drain_queued`):
- `add_task` (coda, auto-download): rifiuta se l'id è già in coda **o** già attivo (un
  poll a metà download non deve ri-accodare: il `.part` non fa scattare `check_file_exists`).
- `add_task_priority` (fronte, manuali): rifiuta solo se già attivo; se già in coda lo
  sposta in testa.
- `remove_queued` (da `cancel_download`): rimuove solo dalla coda, mai dagli attivi;
  ritorna `true` se ha rimosso qualcosa, emette `queue-status-changed` solo in quel caso.

Worker loop (spawnato una volta, `tauri::async_runtime::spawn`):

```
limit = mode==Queue ? 1 : 4
if active_count >= limit: sleep(500ms); continue
resource = pop_front(queue)   // sotto lock: registra subito active_ids + active_weeks
if none: sleep(200ms); continue
active_count += 1; emit("queue-status-changed", {queued, active})
spawn supervisore:
  body = spawn { emit("download-started", id)
                 match download_resource(...):
                   Ok         -> record_downloaded_file(...); emit("download-complete", id)
                   Paused     -> emit("download-paused", id)
                   Cancelled  -> emit("download-cancelled", id)
                   Err(e)     -> emit("download-failed", {id, error}) }
  await body (anche su panic)   // A4: cleanup SEMPRE eseguito
  active_count -= 1; active_ids.remove(id); active_weeks.remove(id); download_signals.remove(id)
continue   // Parallel: pop subito un altro; Queue: bloccato dal limite
```

Il supervisore intercetta il `JoinError` di un eventuale panic del body: emette comunque
`download-failed` ed esegue la pulizia di `active_count`/`active_ids`/`active_weeks`/
`download_signals`, altrimenti un panic lascerebbe `active_count` inflazionato e il
worker si bloccherebbe al limite di concorrenza. `update_mode` cambia solo il valore
letto dal worker al giro successivo (nessun semaforo dinamico).

### DownloadService (`download.rs`)

- Segnale: `Arc<AtomicU8>` (`STATUS_RUNNING=0`, `STATUS_PAUSED=1`, `STATUS_CANCELLED=2`),
  registrato in `AppState.download_signals` per id prima dello stream.
- Filename: `extract_filename_from_url` (decodifica URL-encoding, rimuove query string,
  riduce al `file_name` finale rifiutando `..`/`.`/separatori/nomi riservati Windows
  CON/PRN/COM1-9/LPT1-9), fallback a `sanitize_filename(title)`.
- Path traversal: dopo il join, verifica `dest_path.parent() == Some(dest_dir)` (idem
  `.part`), altrimenti `DownloadError::InvalidFilename`.
- Resume: `.part` esistente → `Range: bytes={len}-`; se il server risponde 200 invece di
  206, riparte da zero troncando il `.part`.
- Progress: `download-progress` throttled a 100ms + un emit finale forzato al 100%.
- Ad ogni chunk controlla il segnale: `PAUSED` → `Err(Paused)` (`.part` conservato);
  `CANCELLED` → cancella il `.part`, `Err(Cancelled)`.
- Completamento: rename `.part` → file finale, SHA-256 dell'intero file, ritorna
  `(PathBuf, hash)`.
- YouTube (`resource.is_youtube()`): nessun HTTP, shortcut `.url`/`.webloc`/`.desktop` per
  piattaforma, hash placeholder `"youtube-shortcut"`.

### Comandi (`commands.rs`)

- `download_resource`: valida `work_directory`, crea la cartella settimana, **delega
  sempre** a `add_task_priority` (nessun download diretto).
- `pause_download`: `try_read` su `download_signals`, se presente `store(STATUS_PAUSED)`;
  no-op se non ancora attivo.
- `cancel_download` (async): prima `remove_queued` — se rimosso, fine subito. Solo se non
  era in coda imposta `STATUS_CANCELLED` sul segnale (download in corso).
- `get_resource_summary`: `active` = `download_queue.active_count()`, `queued` =
  `download_queue.queue_len()`.

### Registro `downloaded_files` ed errata corrige (`errata.rs`)

`AppState.downloaded_files: Vec<DownloadedFile>` (`resource_id`, `week`, `local_path`,
`downloaded_at`, `source_url`, `is_superseded`), persistito in `cache.json` (chiave
`downloaded_files`), caricato a startup.

- `record_downloaded_file` (produttore, dal worker su `Ok(...)`): upsert per
  `(resource_id, week)` — sostituisce l'entry esistente anche se era `superseded`, persiste
  e ricalcola `AppStatus.has_superseded_files`.
- `process_errata` (consumatore, in `force_poll` **prima** di `scan_and_queue` così il
  re-download è già in coda quando lo scan gira): per ogni risorsa remota con `created_at`
  più recente del `downloaded_at` locale nella stessa settimana e non ancora `superseded`:
  1. archivia il file superato (`FileRetentionService::archive_superseded`, errore solo
     loggato);
  2. marca `is_superseded = true` e persiste;
  3. se la categoria è in `auto_download_categories`, re-accoda con `add_task` (**non**
     priority);
  4. se qualcosa è stato marcato, emette `errata-detected` con gli id.

## Eventi backend -> frontend

| Evento | Payload | Emesso da |
|---|---|---|
| `download-started` | `number` (id) | worker, prima del download |
| `download-progress` | `{id, progress, current_bytes, total_bytes}` | `download_file`, ogni 100ms |
| `download-complete` | `number` (id) | worker, dopo `record_downloaded_file` |
| `download-paused` | `number` (id) | worker su `Err(Paused)` |
| `download-cancelled` | `number` (id) | worker su `Err(Cancelled)` |
| `download-failed` | `{id, error}` | worker su errore, o supervisore su panic (`"internal error"`) |
| `queue-status-changed` | `{queued: [{id, position}], active: number[]}` | `add_task`/`add_task_priority`/`remove_queued`/worker ad ogni pop |
| `errata-detected` | `{resourceIds: number[]}` | `process_errata` se marca qualcosa |
| `resources-updated` | `ResourceListResponse` | `force_poll` |

`appStore.ts` è l'unico consumer: aggiorna `activeDownloads` (`pending|downloading|
paused|completed|error`, `progress`, `currentBytes/totalBytes`, `queuePosition`) e
richiama `fetchSummary` (debounced 300ms) dopo ogni evento. Su `errata-detected` non fa
confronti locali: rilegge `get_status`/`get_resources` e mostra un toast informativo.

## Flussi

**Manuale**: click → `startDownload` marca `downloading` in locale → `invoke
('download_resource')` → `add_task_priority` → worker pop appena c'è slot →
`download-started` → `download-progress` → `download-complete`.

**Auto-download**: a startup (2s di ritardo per i listener frontend) e ad ogni
`force_poll`/`set_config`, `scan_and_queue` accoda con `add_task` le risorse di categoria
abilitata non ancora presenti su disco (`check_file_exists`).

**Errata corrige**: `force_poll` → `resources-updated` → `process_errata` (archivia/marca/
re-accoda con `add_task`, `errata-detected`) → **poi** `scan_and_queue` (dedup naturale).

## Cancellazione: in coda vs attiva

```
cancel_download(id)
  -> remove_queued(id): rimosso? --si--> fine (solo queue-status-changed)
  -> no (attivo): signal.store(STATUS_CANCELLED)
       -> al prossimo chunk: rimuove .part, Err(Cancelled), emit "download-cancelled"
```

Una risorsa in coda non ha ancora un segnale in `download_signals` (creato solo quando il
worker la avvia): per questo `cancel_download` prova prima `remove_queued`, altrimenti
impostare il segnale sarebbe un no-op silenzioso.

## Modalità Queue / Parallel

- `Queue`: limite 1, strettamente sequenziale.
- `Parallel`: limite 4; dopo aver avviato un download il worker ricontrolla subito la
  coda (`continue` senza sleep) finché il limite non è raggiunto.
- Cambio modalità (`set_config` → `update_mode`) letto dal worker al giro successivo del
  loop; nessun riavvio né gestione dinamica di semafori.
