<#
  Builds the Windows release files into dist\:
    ferrum-windows-x86_64.zip        ferrum.exe + licenses (get.ps1 installs it)
    ferrum-windows-x86_64-setup.exe  Inno Setup installer (updates itself from the app)

  Run from any folder after `cargo build --release --locked -p ferrum`, or let
  this script build. Checks that ferrum.exe does not need the Visual C++
  runtime (crt-static in .cargo/config.toml). Needs Inno Setup 6 (iscc) for the
  installer: `choco install innosetup` or `winget install JRSoftware.InnoSetup`;
  without it only the zip is built, unless -Strict (CI).
#>
param([switch] $Strict)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
$exe = 'target\release\ferrum.exe'
if (-not (Test-Path $exe)) {
  cargo build --release --locked -p ferrum
  if ($LASTEXITCODE) { throw 'cargo build failed' }
}
New-Item -ItemType Directory -Force dist | Out-Null

# No VCRUNTIME140.dll & co.: the exe must run on a clean Windows.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
  $vs = & $vswhere -latest -products * -property installationPath
  $dumpbin = Get-ChildItem "$vs\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" -ErrorAction SilentlyContinue |
    Select-Object -First 1
}
if ($dumpbin) {
  $deps = & $dumpbin.FullName /dependents $exe | Out-String
  if ($deps -match '(?i)vcruntime|msvcp|api-ms-win-crt') {
    Write-Host $deps
    throw 'ferrum.exe depends on the Visual C++ runtime (see crt-static in .cargo/config.toml)'
  }
  Write-Host 'ferrum.exe does not need the Visual C++ runtime.'
} elseif ($Strict) {
  throw 'dumpbin not found; cannot check the C runtime dependency'
} else {
  Write-Warning 'dumpbin not found; skipping the C runtime check'
}

# Zip without a top folder: get.ps1 extracts it straight into %LOCALAPPDATA%\Ferrum.
$zip = 'dist\ferrum-windows-x86_64.zip'
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path $exe, 'packaging\windows\ferrum.ico', README.md, LICENSE-MIT, LICENSE-APACHE `
  -DestinationPath $zip

# Installer
$iscc = Get-Command iscc -ErrorAction SilentlyContinue
if (-not $iscc) {
  $guess = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
  if (Test-Path $guess) { $iscc = Get-Item $guess }
}
if ($iscc) {
  $path = if ($iscc.Source) { $iscc.Source } else { $iscc.FullName }
  & $path /Q "/DAppVersion=$version" packaging\windows\ferrum.iss
  if ($LASTEXITCODE) { throw 'iscc failed' }
} elseif ($Strict) {
  throw 'Inno Setup (iscc) not found'
} else {
  Write-Warning 'Inno Setup (iscc) not found; skipping the installer'
}

Get-ChildItem dist
