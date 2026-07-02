<p align="center">
  <img src="docs/assets/logo-placeholder.png" alt="Church Helper Desktop Logo" width="120"/>
</p>

<h1 align="center">💻 Church Helper Desktop</h1>

<p align="center">
  <strong>Applicazione desktop cross-platform per il download automatico dei materiali</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Tauri-2.0-FFC131?logo=tauri" alt="Tauri"/>
  <img src="https://img.shields.io/badge/React-18+-61DAFB?logo=react" alt="React"/>
  <img src="https://img.shields.io/badge/Rust-1.75+-DEA584?logo=rust" alt="Rust"/>
  <img src="https://img.shields.io/badge/TypeScript-5+-3178C6?logo=typescript" alt="TypeScript"/>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-✅-blue" alt="Windows"/>
  <img src="https://img.shields.io/badge/macOS-Coming_Soon-yellow" alt="macOS"/>
  <img src="https://img.shields.io/badge/Linux-✅-orange" alt="Linux"/>
</p>

---

## 🎯 Obiettivo

Applicazione desktop che:
1. **Monitora** l'endpoint del Mail Parser Service
2. **Scarica automaticamente** i nuovi materiali
3. **Organizza** i file nella Work Directory configurata
4. **Notifica** l'operatore quando ci sono nuovi contenuti

---

## 🚀 Quick Start

### Prerequisiti

- **Node.js** 20+
- **Rust** 1.75+ ([installa qui](https://www.rust-lang.org/learn/get-started))
- **Tauri prerequisites** ([vedi guida](https://tauri.app/start/prerequisites/))

### Development

```bash
# Installa dipendenze
npm install

# Avvia in development mode
npm run tauri dev
```

### Puntare il desktop allo stub API locale (dev-only)

Per testare senza toccare l'API di produzione, il backend Rust legge (solo
nelle build di debug: `cargo tauri dev` / `cargo build` / `cargo test`, MAI
nelle build di release che arrivano agli utenti) la variabile d'ambiente
`CHURCH_HELPER_API_BASE`: se impostata e non vuota, sostituisce la costante
`API_BASE_URL` (`src-tauri/src/constants.rs`) per ogni chiamata all'API
risorse (polling automatico e "force poll" manuale).

```bash
# 1. Avvia lo stub (repo sorella api-stub/, porta di default 8787)
cd ../api-stub
node server.mjs
# In ascolto su http://localhost:8787 — scenario iniziale "base".
# Cambia scenario a runtime, es. per testare la scelta multi-video:
#   curl -X POST http://localhost:8787/stub/scenario/multi-video

# 2. In un altro terminale, punta il desktop allo stub
cd ../church-helper-desktop
CHURCH_HELPER_API_BASE=http://localhost:8787 npm run tauri dev
```

Senza questa variabile non cambia nulla: il desktop continua a usare
`API_BASE_URL` (produzione) come sempre.

### Build

```bash
# Build per la piattaforma corrente
npm run tauri build

# Output in: src-tauri/target/release/bundle/
```

---

## 🩺 Troubleshooting / Log

L'app usa `tracing`: il livello di log si controlla con la variabile d'ambiente
`RUST_LOG`. Senza `RUST_LOG` il livello di default è `info`.

```bash
# Log di debug per l'app (polling e download): utile per capire perché
# un download non parte o ogni quanto avviene il polling
RUST_LOG=church_helper_desktop_lib=debug ./church-helper-desktop

# Massimo dettaglio (molto verboso, include gli internals delle librerie)
RUST_LOG=church_helper_desktop_lib=trace ./church-helper-desktop
```

Il tick del polling è loggato a livello `debug` e include l'intervallo
configurato; gli eventi di download compaiono anch'essi a `debug`.

---

## 🏗️ Architettura

```
church-helper-desktop/
├── src/                    # Frontend React + TypeScript
│   ├── App.tsx            # Main component
│   └── main.tsx           # Entry point
├── src-tauri/             # Backend Rust
│   ├── src/
│   │   ├── lib.rs         # Tauri commands
│   │   └── main.rs        # Entry point
│   └── Cargo.toml         # Rust dependencies
└── package.json           # Node dependencies
```

---

## 📦 Stack

### Frontend
- React 18 + TypeScript
- Vite (build tool)
- @tauri-apps/api (IPC bridge)

### Backend (Rust)
- tauri 2.0 - Desktop framework
- tokio - Async runtime
- reqwest - HTTP client
- serde - Serialization
- tauri-plugin-store - Persistent storage
- tauri-plugin-notification - Desktop notifications

---

## 📄 Licenza

MIT - Vedi [LICENSE](../LICENSE)

## 🗑️ Uninstallation

The app can be removed via standard system tools (Settings > Apps on Windows, or moving to Trash on macOS).


## 🔐 Code Signing Policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

### Project Team & Roles
* **Maintainers (Committers & Approvers):** [smoxy](https://github.com/smoxy)
* **Reviewers:** Community contributors (Pull Requests are reviewed by Maintainers)