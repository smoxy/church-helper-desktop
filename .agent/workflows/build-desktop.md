---
description: Build the desktop app for production release
---

# Build Desktop App

// turbo-all

1. Navigate to desktop directory
```bash
cd church-helper-desktop
```

2. Install dependencies
```bash
npm ci
```

3. Build production bundle
```bash
npm run tauri build
```

4. Output location
The built app will be in: `src-tauri/target/release/bundle/`
