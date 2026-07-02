# Setup dell'auto-update (passo manuale richiesto)

Questo repo ha il meccanismo di auto-update (`tauri-plugin-updater`) già cablato nel codice e nella
CI (`.github/workflows/build.yml`), ma **non è ancora operativo**: manca la coppia di chiavi di
firma, che va generata da una persona e non deve mai essere generata o committata da un agente
automatico. Finché questo setup non è completato:

- il codice compila ed esegue normalmente;
- gli installer (`.msi`, `.exe` nsis, `.deb`) continuano a essere pubblicati su ogni release, come
  prima di questa modifica;
- l'app, ad ogni avvio, prova comunque a controllare aggiornamenti (`check_for_updates` in
  `src-tauri/src/lib.rs`) ma il check fallisce silenziosamente (loggato come `warn`, mai un crash)
  perché l'endpoint non pubblica ancora un `latest.json` valido;
- la CI stampa un warning ben visibile (`::warning::...`) in ogni release finché i secrets non
  sono configurati, e disabilita automaticamente la generazione degli artefatti di update firmati
  per quella build (l'installer normale viene comunque pubblicato).

Questo documento è la checklist per completare il setup.

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

Questi due secrets NON esistono ancora nel repo: finché non li crei, vale il comportamento
descritto in cima a questo file (build sempre verde, ma senza artefatti di update firmati).

## 3. Sostituire il placeholder della chiave pubblica

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
