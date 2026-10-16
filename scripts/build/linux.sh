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
# - Versione: la build locale NON usa quella di tauri.conf.json (la CI la scrive
#   solo sui tag, quindi qui è vecchia e l'updater offrirebbe una release più
#   vecchia). Viene derivata da git (scripts/build/version.sh: es. v0.3.0 + 3
#   commit -> 0.4.0-dev.3) o presa da VERSION=X.Y.Z[-pre], e passata al compilatore
#   Tauri via TAURI_CONFIG (JSON merge patch letto da tauri-build/tauri-codegen):
#   nessun file tracciato viene modificato.
set -euo pipefail
cd "$(dirname "$0")/../.."

MODE="${1:-release}"
HOST_ARCH="$(uname -m)"; [ "$HOST_ARCH" = "aarch64" ] && HOST_ARCH=arm64
ARCH="${2:-$HOST_ARCH}"
case "$MODE" in release|debug) ;; *) echo "uso: $0 [release|debug] [arm64|x86_64]"; exit 2;; esac
case "$ARCH" in arm64) PLATFORM=linux/arm64;; x86_64) PLATFORM=linux/amd64;; *) echo "arch non supportata: $ARCH"; exit 2;; esac

# shellcheck source=scripts/build/version.sh
source scripts/build/version.sh
APP_VERSION="$(chd_local_version .)" || exit 1
echo "== versione stampata nel binario: $APP_VERSION"

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
  -e TAURI_CONFIG="{\"version\":\"$APP_VERSION\"}" \
  -v "$PWD":/app -w /app/src-tauri -v "$CACHE":/app/src-tauri/target \
  "$IMAGE" cargo build $FLAGS

SUFFIX=""; [ "$MODE" = debug ] && SUFFIX="-debug"
OUT="church-helper-desktop-$ARCH$SUFFIX"
# Controllo di coerenza BEST-EFFORT (non una prova): cerca la versione come
# sottostringa nel binario (letterale in package_info, tauri-codegen context.rs:273-282).
# Non distingue 0.4.0-dev.2 da 0.4.0-dev.21 né da una versione di un crate
# dipendente uguale a quella di tag esatto. La garanzia vera che il binario non sia
# vecchio è `cargo:rerun-if-env-changed=TAURI_CONFIG` (tauri-build lib.rs:472):
# cambiando la versione cargo ricompila il crate.
grep -aqF -- "$APP_VERSION" "$CACHE/$MODE/church-helper-desktop" \
  || { echo "ERRORE: la versione $APP_VERSION non risulta nel binario: build non valida"; exit 1; }
install -m755 "$CACHE/$MODE/church-helper-desktop" "$OUT"
echo "OK → ./$OUT (versione $APP_VERSION)"
