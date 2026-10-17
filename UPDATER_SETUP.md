# Auto-update: setup e funzionamento

L'auto-update (`tauri-plugin-updater`) è **operativo dalla v0.3.0**: la coppia di chiavi di firma
esiste, i secrets `TAURI_SIGNING_PRIVATE_KEY`/`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` sono configurati
in CI e `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` è la **chiave pubblica reale**
(non più il placeholder). Ogni release stabile pubblica installer firmati (`.msi`, `.exe` nsis,
`.deb`), i relativi `.sig` e un `latest.json`.

Le sezioni 1-3 più sotto restano come riferimento storico/di rotazione della chiave: non vanno
rieseguite per una release normale.

## Come funziona il check (dalla v0.4.0)

Codice: `src-tauri/src/services/updater.rs`.

- **Due sorgenti, fallback costruito a mano:**
  - **Updater A (primario)**: il nostro endpoint
    `{api_base_url()}/api/desktop/update/{{target}}/{{arch}}/{{current_version}}`
    (contratto D, servito da mail-parser-service), con l'header anonimo `X-Install-Id`
    (UUID v4, vedi `PRIVACY.md`) e timeout di 30 secondi.
  - **Updater B (fallback)**: il `latest.json` di GitHub configurato in
    `plugins.updater.endpoints` di `tauri.conf.json`, interrogato **senza** header: l'id di
    installazione non arriva mai a GitHub (e viene tolto anche dalla richiesta di download
    dell'installer).
  - B viene interrogato **solo** se A fallisce: rete assente/timeout, `503` o qualunque altro
    non-2xx, corpo non decodificabile. Un `200` con la stessa versione installata e un `204`
    (hold deliberato: pin uguale all'installata o ultima release bloccata) **non** fanno
    ricadere su GitHub: farlo scavalcherebbe pin e blocchi.
  - Pin e blocco valgono quindi in modo best-effort: se il nostro endpoint non è raggiungibile
    il client vede comunque l'ultima stabile di GitHub.
  - I client v0.3.x conoscono solo GitHub: ricevono la v0.4.0 da lì, poi passano al nostro
    endpoint.
- **Quando:** un check ~10 secondi dopo l'avvio e poi ogni `update_check_interval_hours`
  (default 6, campo di `AppConfig`). Mai due check in parallelo; se ci sono download in coda o
  attivi il check viene rinviato di 15 minuti.
- **Consenso:** l'aggiornamento viene scaricato in background e installato **solo** dopo
  conferma esplicita nel dialog ("Installa e riavvia" / "Più tardi"). Nessuna installazione
  silenziosa, mai.
- **"Più tardi":** la versione rifiutata non viene riproposta, **a meno che** la release sia
  critica: `critical: true` nel manifest del nostro endpoint, oppure versione installata
  `< min_supported`. In quel caso il dialog ("Aggiornamento importante") viene riproposto a ogni
  check, sempre chiedendo il consenso.
- **Test in locale (build debug):** con `CHURCH_HELPER_API_BASE` impostato, Updater A punta a
  `api_base_url()` (es. l'api-stub) invece di saltare il check; gli scenari 200/204/503 dello
  stub esercitano l'intera tabella di decisione (sul 503 si vede il fallback verso GitHub).

## Release candidate: cosa ricevono i tester

La CI scrive nei file di versione di una `vX.Y.Z-rcN` **solo** `X.Y.Z` (WiX/MSI non accetta il
prerelease nel `ProductVersion`, vedi `.github/workflows/build.yml`). Conseguenze:

- chi ha installato una `vX.Y.Z-rcN` risulta già alla versione `X.Y.Z`: **non riceve la stabile
  `vX.Y.Z`** (stessa versione, il client non la considera più nuova). Riceverà la successiva
  (`X.Y.Z+1` o superiore); se serve la build stabile esatta va installata a mano;
- le rc sono pubblicate come *prerelease* su GitHub, quindi `releases/latest/download/latest.json`
  non le serve mai;
- **il pin su una rc è vietato** (`pinned_version` della policy admin rifiuta i `-rc`): il
  manifest di una rc annuncia `X.Y.Z` e sarebbe indistinguibile dalla stabile.

## Linux `.deb`: password di amministratore

Su Linux l'aggiornamento di un `.deb` installato dal package manager richiede i privilegi di
root: dopo il consenso nel dialog dell'app compare la **richiesta di password di polkit**
(`pkexec`). Annullarla fa fallire l'installazione (dialog "Aggiornamento non riuscito"); non
essendo un "Più tardi", l'aggiornamento viene riproposto al check successivo. Per un update
senza password servirebbe un bundle AppImage (vedi "Limite noto" più sotto).

## 1. Generare la coppia di chiavi

Dalla root del repo (richiede le dipendenze npm installate, `npm install`):

```bash
npx tauri signer generate -w ~/.tauri/church-helper-desktop.key
```

Il comando chiede (o accetta via prompt) una **password per la chiave privata**: impostala, non
lasciarla vuota. Alla fine produce due file:

- `~/.tauri/church-helper-desktop.key` — la **chiave privata**. Va tenuta segreta, custodita fuori
  dal repo (un password manager va benissimo), e va inserita SOLO nei GitHub Secrets (punto 2).
  **Se questa chiave viene persa, non sarà più possibile pubblicare aggiornamenti firmati per le
  versioni già installate dagli utenti** finché non generano l'app da zero con una nuova chiave.
- `~/.tauri/church-helper-desktop.key.pub` — la **chiave pubblica**. Questa va incollata nel codice
  (punto 3), non è un segreto.

Non generare questa chiave in CI, in uno script automatico, o farla generare a un agente: è
esattamente il tipo di credenziale che questo progetto vuole tenere sotto controllo umano diretto.

## 2. Aggiungere i secrets su GitHub

Nel repo GitHub (`Settings` → `Secrets and variables` → `Actions` → `New repository secret`),
crea questi due secrets (nomi esatti, case-sensitive — sono già referenziati così in
`.github/workflows/build.yml`):

| Nome secret | Valore |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Contenuto del file `~/.tauri/church-helper-desktop.key` (l'intero blocco, non solo una riga) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | La password scelta al punto 1 |

Questi due secrets sono configurati dalla v0.3.0. Se mancassero (es. fork), la build resta verde
ma senza artefatti di update firmati: la CI stampa un `::warning::` e disattiva
`createUpdaterArtifacts` per quella build.

## 3. Sostituire il placeholder della chiave pubblica (già fatto per la v0.3.0)

In `src-tauri/tauri.conf.json` c'è:

```json
"plugins": {
  "updater": {
    "pubkey": "PLACEHOLDER_REPLACE_WITH_TAURI_SIGNER_PUBLIC_KEY",
    "endpoints": [
      "https://github.com/smoxy/church-helper-desktop/releases/latest/download/latest.json"
    ]
  }
}
```

Sostituisci `PLACEHOLDER_REPLACE_WITH_TAURI_SIGNER_PUBLIC_KEY` con il contenuto del file
`~/.tauri/church-helper-desktop.key.pub` generato al punto 1 (una stringa base64 su una riga),
mantenendo le virgolette. Questo valore È pubblico: va committato normalmente nel repo (a
differenza della chiave privata) e va ricompilato/ripubblicato in una nuova release perché le
versioni già installate lo raccolgano — le versioni dell'app pubblicate PRIMA di questo cambio non
sanno verificare gli aggiornamenti firmati con questa chiave, quindi la primissima release dopo
aver completato questo setup andrà comunque installata manualmente dagli utenti (dopo di che,
l'auto-update funziona per le successive).

## 4. Verificare che funzioni

1. Completa i punti 1-3, fai commit del solo cambio al `pubkey` in `tauri.conf.json` (mai la
   chiave privata).
2. Pusha un tag di test (es. `git tag v0.3.0-test && git push origin v0.3.0-test`) e osserva
   `.github/workflows/build.yml` su GitHub Actions:
   - lo step "Check updater signing key" non deve più stampare il warning;
   - lo step "Build and publish release" (`tauri-apps/tauri-action@v0`) deve completare senza
     errori di firma;
   - la Release pubblicata deve contenere, oltre agli installer, i file `.sig` e un asset
     `latest.json`.
3. Verifica il contenuto di `latest.json`: deve avere una entry per `windows-x86_64` e una per
   `linux-x86_64`, ciascuna con `url` e `signature` valorizzati (non vuoti).
4. Se possibile, installa una build precedente e verifica che l'app rilevi l'aggiornamento al
   riavvio (dialog "Aggiornamento disponibile", vedi `check_for_updates` in `src-tauri/src/lib.rs`).
5. Elimina il tag/release di test quando hai finito (`git push origin :refs/tags/v0.3.0-test` +
   cancellazione della Release da GitHub).

## Limite noto: `.deb` su Linux (da decidere)

> Aggiornamento 2026-10: in pratica l'update in-place del `.deb` funziona passando da `pkexec`
> (richiesta password di amministratore, vedi sopra). Il testo seguente resta come contesto per
> l'eventuale passaggio ad AppImage.

Il plugin updater ufficiale ha supporto limitato per l'installazione in-place su `.deb`: il
meccanismo di self-update funziona in modo affidabile soprattutto per bundle `appimage` (che si
sostituisce da solo) — un `.deb` è installato dal package manager di sistema (`dpkg`/`apt`) sotto
`/usr/`, e l'updater potrebbe non avere i permessi o il meccanismo per rimpiazzarlo in-place.
Questo repo pubblica solo `.deb` per Linux (`bundle.targets` in `tauri.conf.json`); non è stato
aggiunto `appimage` in questo lavoro perché è una decisione di scope (nuovo formato di bundle,
nuovi requisiti di build) che spetta al team, non a un cambio automatico.

Due strade possibili, da scegliere quando si completa questo setup:
- **Accettare il limite**: su Linux l'app segnala comunque che è disponibile un aggiornamento
  (il check funziona), ma l'utente deve scaricare e installare il nuovo `.deb` manualmente
  (`sudo apt install ./nuovo-pacchetto.deb`), come già fa oggi senza auto-update.
- **Aggiungere `appimage`** a `bundle.targets` in `tauri.conf.json` e al job `release-linux` in
  `build.yml`, per un update automatico completo anche su Linux. Richiede verificare che il
  bundling AppImage funzioni con le dipendenze GTK/WebKit già in uso (stesso set di librerie di
  sistema del job `.deb` attuale, il bundler AppImage le impacchetta insieme al binario).

## Rotazione della chiave (se mai compromessa)

Se la chiave privata dovesse mai trapelare: genera una nuova coppia (punto 1), aggiorna i secrets
(punto 2) e il `pubkey` in `tauri.conf.json` (punto 3), pubblica una nuova release. Le versioni
dell'app già installate che avevano l'`pubkey` vecchia NON accetteranno più aggiornamenti firmati
con la chiave vecchia (comportamento corretto, è quello che rende inutile la chiave rubata), ma di
conseguenza non potranno nemmeno più auto-aggiornarsi alla versione con la chiave nuova: come al
punto 3, serve una installazione manuale una tantum per "risalire" sulla nuova chiave.
