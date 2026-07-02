# Ambiente di compilazione Tauri 2 per Linux (funziona per arm64 e, via
# --platform linux/amd64 + binfmt/qemu, anche per x86_64 da host arm64).
# Il frontend (dist/) va compilato PRIMA sul host: qui gira solo cargo.
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
    librsvg2-dev libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
