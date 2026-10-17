#Requires -Version 5.1
<#
Compila il binario desktop per Windows 11 (x64). BEST EFFORT: scritto su Linux,
non testato su Windows. Prerequisiti: Rust (rustup, toolchain msvc), Node.js 20+,
WebView2 runtime (di serie su Win11), Visual Studio Build Tools (C++).

  .\scripts\build\windows.ps1            # release
  .\scripts\build\windows.ps1 -Debug     # debug (onora CHURCH_HELPER_API_BASE per i test)

Usa `npm run tauri build`, che attiva da sé la feature custom-protocol e produce
anche i bundle (.msi/.exe) configurati. Con -Debug usa `--debug`.

Versione: come linux.sh, NON usa quella di tauri.conf.json (stale sulle build locali).
VERSION=X.Y.Z[-pre] nell'ambiente vince; altrimenti si deriva da git: tag stabile
vX.Y.Z piu' alto raggiungibile da HEAD (gli -rcN sono ignorati), minor+1 e
-dev.N con N = commit da quel tag (v0.3.0 + 3 commit -> 0.4.0-dev.3), oppure
X.Y.Z se HEAD e' esattamente sul tag. La versione va a tauri-cli con --config
(file JSON temporaneo): nessun file tracciato viene modificato. Stessa logica di
scripts/build/version.sh (che ha il self-test); questa copia PowerShell NON e'
testata. WiX/MSI non accetta un prerelease non numerico come "-dev.N": con una
versione prerelease si compila solo il bundle NSIS.
#>
param([switch]$Debug)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

foreach ($tool in @("node","npm","cargo")) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Prerequisito mancante: $tool" }
}
if (-not (Test-Path node_modules)) { npm ci }

$semver = '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-(0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?$'
if ($env:VERSION) {
  if ($env:VERSION -cnotmatch $semver) { throw "VERSION='$($env:VERSION)' non e' un semver valido (atteso X.Y.Z o X.Y.Z-prerelease)" }
  $AppVersion = $env:VERSION
} else {
  $base = git tag --merged HEAD --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname |
    Where-Object { $_ -cmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$' } | Select-Object -First 1
  if (-not $base) { throw "Nessun tag stabile vX.Y.Z raggiungibile da HEAD (git fetch --tags?) oppure imposta `$env:VERSION" }
  $count = [int](git rev-list --count "$base..HEAD")
  $v = $base.Substring(1).Split('.')
  $AppVersion = if ($count -eq 0) { $base.Substring(1) } else { "$($v[0]).$([int]$v[1] + 1).0-dev.$count" }
}
Write-Host "== versione stampata nel binario: $AppVersion"

$cfg = Join-Path ([IO.Path]::GetTempPath()) "church-helper-version-$PID.json"
Set-Content -Path $cfg -Value ('{"version":"' + $AppVersion + '"}') -Encoding ascii -NoNewline
try {
  $args = @("run","tauri","build","--","--config",$cfg)
  if ($Debug) { $args += "--debug" }
  if ($AppVersion.Contains("-")) { $args += @("--bundles","nsis") }
  npm @args
  if ($LASTEXITCODE -ne 0) { throw "tauri build fallita ($LASTEXITCODE)" }
} finally {
  Remove-Item $cfg -ErrorAction SilentlyContinue
}

$mode = if ($Debug) { "debug" } else { "release" }
Write-Host "OK - versione $AppVersion, binario: src-tauri\target\$mode\church-helper-desktop.exe (bundle in src-tauri\target\$mode\bundle\)"
