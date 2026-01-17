# GitHub Copilot Instructions - Church Helper Desktop

## Project Overview

Cross-platform desktop app (Windows, macOS, Linux) for downloading church materials.

## Stack
- **Frontend**: React 18 + TypeScript + Vite
- **Backend**: Rust 2021 + Tauri 2.0
- **State**: Zustand
- **Styling**: Tailwind CSS

## Coding Standards

### TypeScript
- Strict mode enabled
- Functional components with hooks only
- No `any` types - use proper interfaces
- Use Zustand for state management

### Rust
- 2021 edition
- Handle errors properly - no `unwrap()` in production
- Use `thiserror` for custom error types
- Async with tokio runtime

### Architecture
- **UI is dumb**: No business logic in React components
- **All logic in Rust**: Services handle downloads, config, scheduling
- **Type-safe IPC**: Matching types in TS and Rust

## Guards
- No `any` in TypeScript
- No `unwrap()` in production Rust
- Secrets via environment variables
- Business logic in Rust services only

## Commands
```bash
npm run tauri dev    # Development
npm run tauri build  # Production
cargo test           # Rust tests
```
