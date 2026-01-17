# Church Helper Desktop - Claude AI Context

## Quick Reference

- **Type**: Cross-platform desktop app
- **Purpose**: Download church materials automatically
- **Stack**: Tauri 2.0, React 18, TypeScript, Rust

## Key Commands

```bash
npm run tauri dev    # Development
npm run tauri build  # Production build
cargo test           # Rust tests
```

## Architecture

```
src/           # React frontend (dumb UI)
src-tauri/     # Rust backend (business logic)
```

## Guards

- No `any` in TypeScript
- No `unwrap()` in Rust production code
- UI components have no business logic
- All logic in Rust services
