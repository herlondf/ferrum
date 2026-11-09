<#
  Installs or updates Ferrum for the current user from the latest release
  (no admin, nothing to build).

    irm https://raw.githubusercontent.com/herlondf/ferrum/main/get.ps1 | iex
    $env:FERRUM_VERSION = 'v0.2.0'; irm .../get.ps1 | iex     (a specific version)
    $env:FERRUM_UNINSTALL = '1'; irm .../get.ps1 | iex        (remove it again)

  Goes to %LOCALAPPDATA%\Ferrum, with a Start menu shortcut, and adds the
  folder to the user's PATH. Runs on Windows PowerShell 5.1 and PowerShell 7.
#>
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = if ($env:FERRUM_REPO) { $env:FERRUM_REPO } else { 'herlondf/ferrum' }
$base = if ($env:FERRUM_VERSION) {
  "https://github.com/$repo/releases/download/$($env:FERRUM_VERSION)"
} else {
  "https://github.com/$repo/releases/latest/download"
}
$name = 'ferrum-windows-x86_64.zip'
$dest = Join-Path $env:LOCALAPPDATA 'Ferrum'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Ferrum.lnk'

function Set-UserPath([bool] $add) {
  $path = [Environment]::GetEnvironmentVariable('Path', 'User')
  $parts = @($path -split ';' | Where-Object { $_ -and $_ -ne $dest })
  if ($add) { $parts += $dest }
  [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')
}

if ($env:FERRUM_UNINSTALL) {
  Get-Process ferrum -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dest\*" } | Stop-Process
  Remove-Item $shortcut -Force -ErrorAction SilentlyContinue
  Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
  Set-UserPath $false
  Write-Host 'Ferrum removed. Your settings were kept.'
  return
}

$tmp = Join-Path $env:TEMP "ferrum-get-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
  Write-Host "Downloading $base/$name"
  Invoke-WebRequest "$base/$name" -OutFile "$tmp\$name" -UseBasicParsing
  Invoke-WebRequest "$base/SHA256SUMS" -OutFile "$tmp\SHA256SUMS" -UseBasicParsing
  $line = Get-Content "$tmp\SHA256SUMS" |
    Where-Object { $_ -match "\s\*?$([regex]::Escape($name))$" } | Select-Object -First 1
  $want = if ($line) { ($line -split '\s+')[0] } else { '' }
  $got = (Get-FileHash "$tmp\$name" -Algorithm SHA256).Hash
  if (-not $want -or $want -ne $got) { throw "Checksum mismatch for $name; nothing was installed." }

  # A running copy keeps ferrum.exe locked.
  Get-Process ferrum -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dest\*" } | ForEach-Object {
    Write-Host 'Closing the running Ferrum...'
    $_.CloseMainWindow() | Out-Null
    if (-not $_.WaitForExit(10000)) { $_ | Stop-Process -Force }
  }
  Expand-Archive "$tmp\$name" -DestinationPath $dest -Force

  $shell = New-Object -ComObject WScript.Shell
  $link = $shell.CreateShortcut($shortcut)
  $link.TargetPath = Join-Path $dest 'ferrum.exe'
  $link.WorkingDirectory = $env:USERPROFILE
  $link.Description = 'Ferrum Git client'
  $link.Save()
  Set-UserPath $true
  Write-Host "Ferrum installed in $dest. Open it from the Start menu, or run 'ferrum' in a new terminal."
} finally {
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
