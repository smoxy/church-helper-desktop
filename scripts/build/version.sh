#!/usr/bin/env bash
# Versione da stampare nelle build locali, SENZA toccare i file tracciati.
#
#   scripts/build/version.sh            # stampa la versione derivata
#   source scripts/build/version.sh     # espone chd_local_version
#
# Regole (la CI scrive la versione dal tag solo sulle build di tag, quindi la
# versione in tauri.conf.json resta quella dell'ultimo bump manuale e non va
# usata per le build locali: l'updater offrirebbe una release più vecchia):
#   1. VERSION nell'ambiente vince su tutto (semver X.Y.Z o X.Y.Z-prerelease).
#   2. HEAD esattamente su un tag stabile vX.Y.Z      -> X.Y.Z
#   3. altrimenti, tag stabile vX.Y.Z PIÙ ALTO raggiungibile da HEAD (gli
#      -rcN sono ignorati), minor + 1 e prerelease -dev.N con N = commit da
#      quel tag: v0.3.0 + 3 commit -> 0.4.0-dev.3.
#      Ordina > ultima stabile e < prossima stabile (0.4.0-dev.3 < 0.4.0):
#      l'updater non propone mai una release vecchia e propone la 0.4.0 vera.
#   Si usa il tag più alto (e non il più vicino di `git describe`) perché un
#   hotfix v0.2.1 mergiato dopo v0.3.0 sarebbe "più vicino" e darebbe 0.3.0-dev.N,
#   cioè una versione MINORE della v0.3.0 già pubblicata.

# Regex semver 2.0 (identificatori numerici senza zeri iniziali): è la stessa
# validazione stretta che serve al crate `semver` usato da tauri-codegen.
_CHD_NUM='(0|[1-9][0-9]*)'
_CHD_PRE='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
_CHD_SEMVER_RE="^${_CHD_NUM}\.${_CHD_NUM}\.${_CHD_NUM}(-${_CHD_PRE}(\.${_CHD_PRE})*)?\$"
_CHD_STABLE_TAG_RE="^v${_CHD_NUM}\.${_CHD_NUM}\.${_CHD_NUM}\$"

# chd_local_version [dir-repo]  -> stampa la versione su stdout; errori su stderr, exit 1.
chd_local_version() {
  local repo="${1:-.}"

  if [ -n "${VERSION:-}" ]; then
    if [[ ! "$VERSION" =~ $_CHD_SEMVER_RE ]]; then
      echo "VERSION='$VERSION' non è un semver valido (atteso X.Y.Z o X.Y.Z-prerelease, es. 0.4.0 o 0.4.0-dev.3)" >&2
      return 1
    fi
    printf '%s\n' "$VERSION"
    return 0
  fi

  local base
  base="$(git -C "$repo" tag --merged HEAD --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname 2>/dev/null \
    | grep -E "$_CHD_STABLE_TAG_RE" | head -n 1 || true)"
  if [ -z "$base" ]; then
    echo "nessun tag stabile vX.Y.Z raggiungibile da HEAD: non posso derivare la versione." >&2
    echo "Scarica i tag (git fetch --tags) oppure imposta a mano, es.: VERSION=0.4.0-dev.1 $0" >&2
    return 1
  fi

  local count
  count="$(git -C "$repo" rev-list --count "${base}..HEAD")"
  local ver="${base#v}"
  if [ "$count" -eq 0 ]; then
    printf '%s\n' "$ver"
    return 0
  fi

  local major minor
  major="${ver%%.*}"
  minor="${ver#*.}"; minor="${minor%%.*}"
  printf '%s.%s.0-dev.%s\n' "$major" "$((minor + 1))" "$count"
}

# Eseguito direttamente (non via `source`): stampa la versione.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  chd_local_version "$(dirname "$0")/../.."
fi
