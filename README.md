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
  <img src="https://img.shields.io/badge/macOS-✅-silver" alt="macOS"/>
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

### Build

```bash
# Build per la piattaforma corrente
npm run tauri build

# Output in: src-tauri/target/release/bundle/
```

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
