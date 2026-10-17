#!/usr/bin/env bash
# Self-test di scripts/ci/version-from-tag.sh (lo step "Update version from tag"
# dei job di build.yml). Non usa la rete né cargo: crea repo git usa-e-getta
# sotto $TMPDIR con un "origin" bare locale che fa da origin/main.
#
#   scripts/ci/version-from-tag.test.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$HERE/version-from-tag.sh"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/version-from-tag-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "ok   - $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL - $1"; [ -n "${2:-}" ] && echo "       $2"; }

# core.autocrlf=false: su windows-latest la config di sistema ha autocrlf=true e
# reset_files (git checkout) riscriverebbe le fixture in CRLF, falsando i
# confronti riga per riga su Cargo.toml.
g() { git -C "$REPO" -c core.autocrlf=false -c user.name=t -c user.email=t@t -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }
# jq -r senza \r: jq.exe nativo (windows-latest) stampa CRLF.
jqr() { jq -r "$@" | tr -d '\r'; }

# --- fixture: origin bare + clone di lavoro --------------------------------------
ORIGIN="$WORK/origin.git"
REPO="$WORK/work"
git init -q --bare -b main "$ORIGIN"
git init -q -b main "$REPO"
g remote add origin "$ORIGIN"
mkdir -p "$REPO/src-tauri"
cat > "$REPO/src-tauri/tauri.conf.json" <<'EOF'
{
  "productName": "church-helper-desktop",
  "version": "0.4.0",
  "bundle": { "createUpdaterArtifacts": true }
}
EOF
cat > "$REPO/package.json" <<'EOF'
{
  "name": "church-helper-desktop",
  "version": "0.4.0",
  "dependencies": { "x": "^1.0.0" }
}
EOF
# Due righe `version = ...` a inizio riga: deve cambiare solo la prima.
cat > "$REPO/src-tauri/Cargo.toml" <<'EOF'
[package]
name = "church-helper-desktop"
version = "0.4.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = [] }

[package.metadata.fixture]
version = "9.9.9"
EOF
g add -A
g commit -q -m base
g push -q origin main
# develop: un commit non ancora su main
g checkout -q -b develop
g commit -q --allow-empty -m feature
g checkout -q main

CARGO_BEFORE="$(cat "$REPO/src-tauri/Cargo.toml")"

reset_files() { g checkout -q -- . && g clean -q -fdx; }

# run_tag <tag> -> esegue lo script nella cwd del repo; output in $OUT, rc in $RC.
run_tag() {
  OUT="$(cd "$REPO" && GITHUB_REF_NAME="$1" bash "$SCRIPT" 2>&1)"; RC=$?
}

expect_ok() { # <descrizione> <tag>
  reset_files
  run_tag "$2"
  if [ "$RC" -eq 0 ]; then ok "$1"; else fail "$1" "rc=$RC out='$OUT'"; fi
}
expect_fail() { # <descrizione> <tag> <regex sul messaggio>
  reset_files
  run_tag "$2"
  if [ "$RC" -ne 0 ] && [[ "$OUT" =~ $3 ]]; then ok "$1 -> ${OUT%%$'\n'*}"
  else fail "$1" "rc=$RC out='$OUT'"; fi
  # In caso di errore nessun file deve essere stato toccato.
  if [ -n "$(g status --porcelain)" ]; then fail "$1: file modificati nonostante l'errore" "$(g status --porcelain)"; fi
}

# expect_versions <descrizione> <versione attesa>
expect_versions() {
  local conf pkg cargo_first cargo_rest leftovers
  conf="$(jqr .version "$REPO/src-tauri/tauri.conf.json")"
  pkg="$(jqr .version "$REPO/package.json")"
  cargo_first="$(grep -m1 '^version = ' "$REPO/src-tauri/Cargo.toml")"
  [ "$conf" = "$2" ] && ok "$1: tauri.conf.json -> $conf" || fail "$1: tauri.conf.json" "atteso $2, ottenuto $conf"
  [ "$pkg" = "$2" ] && ok "$1: package.json -> $pkg" || fail "$1: package.json" "atteso $2, ottenuto $pkg"
  [ "$cargo_first" = "version = \"$2\"" ] && ok "$1: Cargo.toml prima riga version -> $cargo_first" \
    || fail "$1: Cargo.toml prima riga version" "ottenuto '$cargo_first'"
  # Cargo.toml: deve differire dall'originale SOLO per la prima riga version.
  cargo_rest="$(printf '%s\n' "$CARGO_BEFORE" | sed "s/^version = \"0\.4\.0\"/version = \"$2\"/")"
  [ "$(cat "$REPO/src-tauri/Cargo.toml")" = "$cargo_rest" ] && ok "$1: Cargo.toml cambia solo nella prima riga version" \
    || fail "$1: Cargo.toml" "$(diff <(printf '%s\n' "$cargo_rest") "$REPO/src-tauri/Cargo.toml")"
  grep -q '^version = "9.9.9"$' "$REPO/src-tauri/Cargo.toml" && ok "$1: seconda riga version intatta" \
    || fail "$1: seconda riga version modificata"
  [ "$(jqr '.dependencies.x' "$REPO/package.json")" = "^1.0.0" ] && [ "$(jqr '.bundle.createUpdaterArtifacts' "$REPO/src-tauri/tauri.conf.json")" = "true" ] \
    && ok "$1: resto dei JSON intatto" || fail "$1: resto dei JSON modificato"
  leftovers="$(g status --porcelain --untracked-files=all | grep -v '^ M ' || true)"
  [ -z "$leftovers" ] && ok "$1: nessun file temporaneo lasciato (tmp.json, .bak)" || fail "$1: file temporanei" "$leftovers"
}

# --- stabile su main -------------------------------------------------------------
g checkout -q main
expect_ok "stabile v1.2.3 su commit di main" v1.2.3
expect_versions "stabile v1.2.3" 1.2.3
[[ "$OUT" =~ "::warning::Versione committata in tauri.conf.json (0.4.0) diversa da quella del tag (1.2.3)" ]] \
  && ok "stabile v1.2.3: warning versione committata diversa" || fail "warning versione diversa" "$OUT"
[[ "$OUT" == *"::notice::"* ]] && fail "stabile: notice rc inattesa" "$OUT" || ok "stabile: nessuna notice rc"

expect_ok "stabile v0.4.0 uguale alla versione committata" v0.4.0
expect_versions "stabile v0.4.0" 0.4.0
[[ "$OUT" == *"::warning::"* ]] && fail "v0.4.0: warning inatteso" "$OUT" || ok "v0.4.0: nessun warning se tag = versione committata"

expect_ok "stabile a più cifre v10.20.30" v10.20.30
expect_versions "stabile v10.20.30" 10.20.30

# --- stabile fuori da main ---------------------------------------------------------
g checkout -q develop
expect_fail "stabile v1.2.3 su commit non in main" v1.2.3 "::error::Tag 'v1\.2\.3' e' una release stabile ma il commit taggato non e' contenuto in main"

# --- rc fuori da main: ok, scrive solo X.Y.Z ------------------------------------
expect_ok "rc v2.0.0-rc3 su commit non in main" v2.0.0-rc3
expect_versions "rc v2.0.0-rc3 (scrive X.Y.Z)" 2.0.0
[[ "$OUT" =~ "::notice::Tag rc: nei file di versione viene scritta solo la parte 2.0.0" ]] \
  && ok "rc: notice prerelease" || fail "rc: notice prerelease" "$OUT"
expect_ok "rc a più cifre v1.2.3-rc12" v1.2.3-rc12
expect_versions "rc v1.2.3-rc12" 1.2.3

# --- rc su main ------------------------------------------------------------------
g checkout -q main
expect_fail "rc v1.2.3-rc1 su commit gia' in main" v1.2.3-rc1 "::error::Tag 'v1\.2\.3-rc1' e' una release candidate ma il commit taggato e' gia' contenuto in main"

# --- tag non validi (rifiutati prima di toccare git o i file) --------------------
for t in '' v1.2 1.2.3 v1.2.3.4 v1.2.3-rc0 v1.2.3-rc01 v1.2.3-rc v1.2.3-beta1 v1.2.3-rc1x \
         'v1.2.3 ' 'v1.2.3;rm -rf x' 'v1.2.3"' V1.2.3 'v1.2.3
v1.2.4'; do
  expect_fail "tag non valido '$t'" "$t" "::error::Tag '.*' non e' valido\. Formati ammessi"
done

# --- origin/main locale vecchio: lo script deve fare fetch di main ----------------
# Simula il checkout di una run di tag: il commit taggato arriva col tag, mentre
# origin/main locale e' indietro (non lo contiene ancora).
OTHER="$WORK/other"
git clone -q -c core.autocrlf=false "$ORIGIN" "$OTHER"
git -C "$OTHER" -c user.name=t -c user.email=t@t -c commit.gpgsign=false -c tag.gpgsign=false commit -q --allow-empty -m release
git -C "$OTHER" -c user.name=t -c user.email=t@t -c tag.gpgsign=false tag v3.0.0
git -C "$OTHER" push -q origin main v3.0.0
g fetch -q origin refs/tags/v3.0.0:refs/tags/v3.0.0
if g merge-base --is-ancestor v3.0.0 origin/main 2>/dev/null; then
  fail "fixture: origin/main locale doveva essere indietro"
fi
g checkout -q v3.0.0
expect_ok "stabile v3.0.0 su main con origin/main locale non aggiornato" v3.0.0
expect_versions "stabile v3.0.0 dopo fetch" 3.0.0
g checkout -q main

echo "== $PASS ok, $FAIL falliti"
[ "$FAIL" -eq 0 ]
