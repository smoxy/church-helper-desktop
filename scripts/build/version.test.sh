#!/usr/bin/env bash
# Self-test della derivazione di versione (scripts/build/version.sh).
# Non usa docker né cargo: crea repo git temporanei sotto ~/.cache/church-helper-builds
# (mai /tmp, che è tmpfs).
#
#   scripts/build/version.test.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/build/version.sh
source "$HERE/version.sh"

BASE="$HOME/.cache/church-helper-builds"
mkdir -p "$BASE"
WORK="$(mktemp -d "$BASE/version-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()   { PASS=$((PASS + 1)); echo "ok   - $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL - $1"; [ -n "${2:-}" ] && echo "       $2"; }

g() { git -C "$REPO" -c user.name=t -c user.email=t@t -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }
commit() { g commit -q --allow-empty -m "${1:-c}"; }
new_repo() { REPO="$WORK/$1"; mkdir -p "$REPO"; g init -q -b main; }

# expect_version <descrizione> <atteso>   (VERSION nell'ambiente è già gestita dal chiamante)
expect_version() {
  local got
  got="$(chd_local_version "$REPO" 2>/dev/null)" || { fail "$1" "errore inatteso (atteso $2)"; return; }
  [ "$got" = "$2" ] && ok "$1 -> $got" || fail "$1" "atteso $2, ottenuto $got"
}
expect_error() { # <descrizione> <regex sul messaggio>
  local out rc
  out="$(chd_local_version "$REPO" 2>&1)"; rc=$?
  if [ "$rc" -ne 0 ] && [[ "$out" =~ $2 ]]; then ok "$1 -> errore: ${out%%$'\n'*}"
  else fail "$1" "rc=$rc out='$out'"; fi
}

unset VERSION

# --- storia: v0.2.0, rc5, v0.3.0, 2 commit, rc di 0.4.0 -----------------------
new_repo main
commit c1; g tag v0.2.0
commit c2; g tag v0.3.0-rc5
commit c3
expect_version "solo rc dopo la stabile v0.2.0 (rc5 ignorato)" "0.3.0-dev.2"
g tag v0.3.0
expect_version "HEAD esattamente su tag stabile v0.3.0" "0.3.0"
commit c4; commit c5; commit c6
expect_version "3 commit dopo v0.3.0" "0.4.0-dev.3"
g tag v0.4.0-rc1
expect_version "HEAD su un tag rc dopo la stabile: l'rc non conta" "0.4.0-dev.3"
g checkout -q v0.3.0-rc5
expect_version "HEAD su un rc con stabile precedente" "0.3.0-dev.1"
g checkout -q main

# --- ordinamento numerico dei tag (v0.10.0 > v0.9.0) --------------------------
new_repo order
commit a; g tag v0.9.0
commit b; g tag v0.10.0
commit c
expect_version "v0.10.0 è più alta di v0.9.0" "0.11.0-dev.1"

# --- hotfix mergiato dopo la stabile più alta: non deve abbassare la versione --
new_repo hotfix
commit a; g tag v0.2.0
g checkout -q -b fix; commit h; g tag v0.2.1
g checkout -q main; commit b; g tag v0.3.0
commit c
g merge -q --no-ff -m merge fix
expect_version "hotfix v0.2.1 mergiato dopo v0.3.0 (non scende sotto 0.3.0)" "0.4.0-dev.3"

# --- override VERSION ----------------------------------------------------------
REPO="$WORK/main"
for v in 1.2.3 0.4.0-rc1 0.4.0-dev.7 10.20.30-alpha.1.x-y; do
  VERSION="$v" expect_version "VERSION valida '$v' vince su git" "$v"
done
for v in v1.2.3 1.2 1.2.3. 1.2.3- 01.2.3 1.2.3-01 1.2.3-a..b '1.2.3 ' '1.2.3","x":"y' '1.2.3;rm -rf' ' '; do
  VERSION="$v" expect_error "VERSION non valida '$v'" "non è un semver valido"
done
VERSION="" expect_version "VERSION vuota = non impostata" "0.4.0-dev.3"

# --- nessun tag ---------------------------------------------------------------
new_repo empty
commit a
expect_error "nessun tag" "nessun tag stabile"
g tag v0.3.0-rc1
expect_error "solo tag rc" "nessun tag stabile"
g tag foo
expect_error "tag non semver" "nessun tag stabile"

# --- eseguito come script: stampa la versione e fallisce con exit != 0 ---------
if out="$(VERSION=9.9.9 "$HERE/version.sh" 2>&1)" && [ "$out" = "9.9.9" ]; then
  ok "version.sh eseguito direttamente rispetta VERSION"
else fail "version.sh eseguito direttamente" "out='$out'"; fi
if out="$(VERSION=bad "$HERE/version.sh" 2>&1)"; then fail "version.sh con VERSION=bad deve uscire != 0" "out='$out'"
else ok "version.sh con VERSION non valida esce != 0"; fi

echo "== $PASS ok, $FAIL falliti"
[ "$FAIL" -eq 0 ]
