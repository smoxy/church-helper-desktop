#!/usr/bin/env bash
# Self-test degli argomenti con cui scripts/build/linux.sh lancia docker.
# Non usa docker né cargo né npm reali: li sostituisce con stub nel PATH e con
# HOME temporanea sotto ~/.cache/church-helper-builds (mai /tmp, che è tmpfs).
#
#   scripts/build/linux.test.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

BASE="$HOME/.cache/church-helper-builds"
mkdir -p "$BASE"
WORK="$(mktemp -d "$BASE/linux-test.XXXXXX")"
ARCH=arm64
OUT="$ROOT/church-helper-desktop-$ARCH-debug"
[ -e "$OUT" ] && { echo "esiste già $OUT: spostalo prima del test"; exit 2; }
trap 'rm -rf "$WORK" "$OUT"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "ok   - $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL - $1"; [ -n "${2:-}" ] && echo "       $2"; }

mkdir -p "$WORK/bin" "$WORK/home"
cat > "$WORK/bin/npm" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
# docker stub: registra gli argomenti di `docker run` e simula il binario
# prodotto (con la versione dentro) nella cartella target montata.
cat > "$WORK/bin/docker" <<'EOF'
#!/usr/bin/env bash
[ "$1" = run ] || exit 0
printf '%s\n' "$@" > "$DOCKER_RUN_LOG"
target=""; version=""; mode=debug; prev=""
for a in "$@"; do
  [ "$prev" = -v ] && [[ "$a" == *:/app/src-tauri/target ]] && target="${a%:/app/src-tauri/target}"
  [ "$prev" = -e ] && [[ "$a" == TAURI_CONFIG=* ]] && version="$a"
  [ "$a" = --release ] && mode=release
  prev="$a"
done
mkdir -p "$target/$mode"
printf 'bin %s\n' "$version" > "$target/$mode/church-helper-desktop"
EOF
chmod +x "$WORK/bin/npm" "$WORK/bin/docker"

export DOCKER_RUN_LOG="$WORK/docker-run.args"
if out="$(HOME="$WORK/home" PATH="$WORK/bin:$PATH" VERSION=9.9.9-test \
          "$ROOT/scripts/build/linux.sh" debug "$ARCH" 2>&1)"; then
  ok "linux.sh debug con docker stub termina con successo"
else
  fail "linux.sh debug con docker stub" "$out"
fi

CH="$WORK/home/.cache/church-helper-builds/cargo-home-$ARCH"
args="$(cat "$DOCKER_RUN_LOG" 2>/dev/null)"
has_pair() { # <flag> <valore>: la coppia compare consecutiva negli argomenti
  awk -v f="$1" -v v="$2" 'p==f && $0==v {found=1} {p=$0} END {exit !found}' <<< "$args"
}

# Regressione: l'intera CARGO_HOME deve essere persistente e condivisa, così i
# lock della package cache (nella sua radice) sono visti da build concorrenti.
has_pair -e "CARGO_HOME=/cargo-home" \
  && ok "CARGO_HOME del container = /cargo-home" \
  || fail "manca -e CARGO_HOME=/cargo-home" "$args"
has_pair -v "$CH:/cargo-home" \
  && ok "CARGO_HOME montata intera da $CH" \
  || fail "manca -v $CH:/cargo-home" "$args"
grep -qE ':/usr/local/cargo/(registry|git)$' <<< "$args" \
  && fail "montate ancora le sole sottocartelle registry/git (lock non condivisi)" "$args" \
  || ok "nessun mount parziale di registry/ o git/"
[ -d "$CH" ] && ok "cartella cache CARGO_HOME creata sull'host" || fail "cartella $CH non creata"
[ -x "$OUT" ] && ok "binario installato in ./$(basename "$OUT")" || fail "binario non installato"

echo "== $PASS ok, $FAIL falliti"
[ "$FAIL" -eq 0 ]
