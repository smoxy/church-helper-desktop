# Church Helper Desktop - AI Agent Guidelines

## App Overview

Cross-platform desktop app (Windows, macOS, Linux) that:
1. Polls server for new resources
2. Downloads materials to work directory
3. Notifies user of updates

## Stack

### Frontend (src/)
- **React 18** + TypeScript
- **Vite** - Build tool
- **Zustand** - State management
- **Tailwind CSS** - Styling
- **@tauri-apps/api** - IPC bridge

### Backend (src-tauri/)
- **Rust** 2021 edition
- **Tauri 2.0** - Desktop framework
- **tokio** - Async runtime
- **reqwest** - HTTP client
- **serde** - Serialization

## Commands

```bash
# Development
npm run tauri dev

# Build production
npm run tauri build

# Type check frontend
npx tsc --noEmit

# Test Rust
cargo test --manifest-path src-tauri/Cargo.toml

# Format Rust
cargo fmt --manifest-path src-tauri/Cargo.toml
```

## Code Patterns

### React Components (Dumb)
```tsx
// src/components/ResourceCard.tsx
interface Props {
  title: string;
  onDownload: () => void;
}
export function ResourceCard({ title, onDownload }: Props) {
  return <div onClick={onDownload}>{title}</div>;
}
```

### Tauri Commands (IPC)
```rust
// src-tauri/src/commands/resources.rs
#[tauri::command]
async fn get_resources() -> Result<Vec<Resource>, String> {
    resource_service::fetch_all().await.map_err(|e| e.to_string())
}
```

### Invoking from Frontend
```tsx
import { invoke } from '@tauri-apps/api/core';
const resources = await invoke<Resource[]>('get_resources');
```

## Guards

1. **No `any` types** - Use proper TypeScript interfaces
2. **No `unwrap()` in production** - Use `?` operator or proper error handling
3. **UI is dumb** - No business logic in React components
4. **All logic in Rust** - Services handle downloads, config, scheduling
5. **Type-safe IPC** - Define matching types in TS and Rust

## Architecture

```
┌─────────────────────────────────────────┐
│           Frontend (React)              │
│  Components → Hooks → invoke()          │
└─────────────────┬───────────────────────┘
                  │ IPC
┌─────────────────▼───────────────────────┐
│           Backend (Rust)                │
│  Commands → Services → External APIs    │
└─────────────────────────────────────────┘
```

## File Structure

```
src/                    # React frontend
├── components/         # UI components
├── hooks/              # Custom hooks (Tauri bridge)
├── stores/             # Zustand stores
└── App.tsx

src-tauri/              # Rust backend
├── src/
│   ├── commands/       # IPC handlers
│   ├── services/       # Business logic
│   └── models/         # Domain types
└── Cargo.toml
```
