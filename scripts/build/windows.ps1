#Requires -Version 5.1
<#
Compila il binario desktop per Windows 11 (x64). BEST EFFORT: scritto su Linux,
non testato su Windows. Prerequisiti: Rust (rustup, toolchain msvc), Node.js 20+,
WebView2 runtime (di serie su Win11), Visual Studio Build Tools (C++).

  .\scripts\build\windows.ps1            # release
  .\scripts\build\windows.ps1 -Debug     # debug (onora CHURCH_HELPER_API_BASE per i test)

Usa `npm run tauri build`, che attiva da sé la feature custom-protocol e produce
anche i bundle (.msi/.exe) configurati. Con -Debug usa `--debug`.
#>
param([switch]$Debug)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

foreach ($tool in @("node","npm","cargo")) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Prerequisito mancante: $tool" }
}
if (-not (Test-Path node_modules)) { npm ci }

$args = @("run","tauri","build")
if ($Debug) { $args += @("--","--debug") }
npm @args
if ($LASTEXITCODE -ne 0) { throw "tauri build fallita ($LASTEXITCODE)" }

$mode = if ($Debug) { "debug" } else { "release" }
Write-Host "OK - binario: src-tauri\target\$mode\church-helper-desktop.exe (bundle in src-tauri\target\$mode\bundle\)"
