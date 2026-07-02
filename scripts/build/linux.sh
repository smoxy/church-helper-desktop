#!/usr/bin/env bash
# Compila il binario desktop per Linux, in Docker (non servono le lib GTK sul host).
#
#   scripts/build/linux.sh [release|debug] [arm64|x86_64]   (default: release, arch host)
#
# - Include SEMPRE --features tauri/custom-protocol: senza, il binario cerca il
#   dev server su localhost:1420 invece degli asset incorporati (bug già vissuto).
# - La build "debug" onora l'override CHURCH_HELPER_API_BASE (test con lo stub);
#   la "release" lo ignora per sicurezza e va usata contro la produzione.
# - Cross-arch: x86_64 da host arm64 (e viceversa) richiede binfmt/qemu:
#     docker run --privileged --rm tonistiigi/binfmt --install amd64
#   ATTENZIONE (verificato 2026-07-02): rustc sotto qemu-user va in SIGSEGV su
#   questo host — l'emulazione NON è affidabile per compilare Rust. Per x86_64
#   usa un host x86_64 nativo (lo script è identico) o la CI GitHub, che su tag
#   v* compila già su runner ubuntu-latest x86_64 (.github/workflows/build.yml).
set -euo pipefail
cd "$(dirname "$0")/../.."

MODE="${1:-release}"
HOST_ARCH="$(uname -m)"; [ "$HOST_ARCH" = "aarch64" ] && HOST_ARCH=arm64
ARCH="${2:-$HOST_ARCH}"
case "$MODE" in release|debug) ;; *) echo "uso: $0 [release|debug] [arm64|x86_64]"; exit 2;; esac
case "$ARCH" in arm64) PLATFORM=linux/arm64;; x86_64) PLATFORM=linux/amd64;; *) echo "arch non supportata: $ARCH"; exit 2;; esac

IMAGE="chd-tauri-build:$ARCH"
CACHE="$HOME/.cache/church-helper-builds/target-$ARCH"   # mai /tmp: è tmpfs (RAM)
mkdir -p "$CACHE"

echo "== frontend (host): npm ci se serve + vite build"
[ -d node_modules ] || npm ci
npm run build

echo "== immagine docker $IMAGE ($PLATFORM)"
docker build --platform "$PLATFORM" -t "$IMAGE" -f scripts/build/tauri-linux.Dockerfile scripts/build

FLAGS="--features tauri/custom-protocol"; [ "$MODE" = release ] && FLAGS="$FLAGS --release"
echo "== cargo build $MODE ($ARCH)"
docker run --rm --platform "$PLATFORM" \
  -v "$PWD":/app -w /app/src-tauri -v "$CACHE":/app/src-tauri/target \
  "$IMAGE" cargo build $FLAGS

SUFFIX=""; [ "$MODE" = debug ] && SUFFIX="-debug"
OUT="church-helper-desktop-$ARCH$SUFFIX"
install -m755 "$CACHE/$MODE/church-helper-desktop" "$OUT"
echo "OK → ./$OUT"
