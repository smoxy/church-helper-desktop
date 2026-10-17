#!/usr/bin/env bash
# Allinea la versione dei file di build al tag di release (solo build CI di tag).
#
#   GITHUB_REF_NAME=v1.2.3 scripts/ci/version-from-tag.sh
#
# Va lanciato dalla root del repository (la cwd dei job di build.yml): riscrive
# src-tauri/tauri.conf.json, package.json e la prima riga `version = "..."` di
# src-tauri/Cargo.toml. I file sono modificati solo nel checkout della CI, mai
# committati.
#
# Portabile su tutti i runner di build.yml: GNU (ubuntu), Git Bash
# (windows-latest) e macOS (BSD sed/coreutils, anche bash 3.2): niente
# `sed -i` / `0,/re/` GNU, niente feature di bash 4+, la riscrittura di
# Cargo.toml usa perl (presente su tutti e tre). Richiede jq e git.
set -euo pipefail

TAG="${GITHUB_REF_NAME:-}"

# Il tag e' l'unica fonte di verita' per la versione rilasciata:
# formati ammessi: vX.Y.Z (stabile) o vX.Y.Z-rcN (release
# candidate, N intero >= 1, es. v1.2.3-rc1). Tutto il resto
# fa fallire subito la release con un errore chiaro invece di
# produrre artefatti con versione ambigua.
# (Regex in variabile e non letterale dentro [[ ]]: stessa semantica su
# bash 3.2 e 5.)
TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+(-rc[1-9][0-9]*)?$'
if [[ ! "$TAG" =~ $TAG_RE ]]; then
  echo "::error::Tag '$TAG' non e' valido. Formati ammessi: vX.Y.Z (release stabile) oppure vX.Y.Z-rcN (release candidate, N>=1), es. v1.2.3 o v1.2.3-rc1. Interrompo la release."
  exit 1
fi

if [[ "$TAG" == *-rc* ]]; then
  IS_RC=true
else
  IS_RC=false
fi

# Vincolo di branch: le rc sono ammesse solo su commit non ancora
# su main, le stabili solo su commit gia' contenuti in main.
# Richiede fetch-depth: 0 nel checkout per avere storia
# sufficiente al merge-base.
git fetch origin main --quiet
if git merge-base --is-ancestor HEAD origin/main; then
  ON_MAIN=true
else
  ON_MAIN=false
fi

if [ "$IS_RC" = true ] && [ "$ON_MAIN" = true ]; then
  echo "::error::Tag '$TAG' e' una release candidate ma il commit taggato e' gia' contenuto in main. Le rc sono ammesse solo su commit non ancora mergiati in main. Interrompo la release."
  exit 1
fi

if [ "$IS_RC" = false ] && [ "$ON_MAIN" = false ]; then
  echo "::error::Tag '$TAG' e' una release stabile ma il commit taggato non e' contenuto in main. Le release stabili sono ammesse solo su commit gia' presenti in main. Interrompo la release."
  exit 1
fi

VERSION=${TAG#v}
# WiX/MSI (Windows) non accetta un ProductVersion con
# pre-release: per le rc nei file di versione scriviamo solo la
# parte X.Y.Z, il tag/release name completo resta quello vero.
BASE_VERSION=${VERSION%-rc*}

# Anti-injection: la versione estratta dal tag deve ripassare
# una regex stretta prima di finire in perl/jq, anche se il
# pattern del tag sopra la vincola gia'.
BASE_VERSION_RE='^[0-9]+\.[0-9]+\.[0-9]+$'
if [[ ! "$BASE_VERSION" =~ $BASE_VERSION_RE ]]; then
  echo "::error::Versione '$BASE_VERSION' estratta dal tag '$TAG' non e' un semver X.Y.Z valido. Interrompo la release."
  exit 1
fi

if [ "$IS_RC" = true ]; then
  echo "::notice::Tag rc: nei file di versione viene scritta solo la parte $BASE_VERSION (WiX/MSI non accetta prerelease nel ProductVersion). La release GitHub sara' marcata prerelease: non verra' servita dall'auto-update (latest.json ignora le prerelease)."
fi

# tr -d '\r': su windows-latest jq.exe nativo scrive lo stdout in modalita' testo
# (CRLF) e $(...) toglie solo il \n finale; senza, CURRENT finirebbe con \r e il
# warning "versione diversa" comparirebbe sempre.
CURRENT=$(jq -r '.version' src-tauri/tauri.conf.json | tr -d '\r')
if [ "$CURRENT" != "$BASE_VERSION" ]; then
  echo "::warning::Versione committata in tauri.conf.json ($CURRENT) diversa da quella del tag ($BASE_VERSION): vince il tag, i file vengono allineati solo per questa build CI."
fi
echo "Updating version to $BASE_VERSION"
jq --arg v "$BASE_VERSION" '.version = $v' src-tauri/tauri.conf.json > tmp.json && mv tmp.json src-tauri/tauri.conf.json
jq --arg v "$BASE_VERSION" '.version = $v' package.json > tmp.json && mv tmp.json package.json
# Solo la PRIMA riga `version = "..."` (quella di [package]): le dipendenze
# inline come `foo = { version = "1" }` non iniziano con `version` e restano
# intatte. La versione arriva a perl via ambiente, mai interpolata nel codice.
# -i.bak e non -i: perl su Windows rifiuta l'in-place editing senza backup.
BASE_VERSION="$BASE_VERSION" perl -i.bak -pe 'if (!$done) { $done = s/^version = ".*"/version = "$ENV{BASE_VERSION}"/ }' src-tauri/Cargo.toml
rm -f src-tauri/Cargo.toml.bak
