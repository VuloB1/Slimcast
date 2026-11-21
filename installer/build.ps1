# Compila Slimcast y genera dist\Slimcast-<versión>-setup.exe con ffmpeg incluido.
# Uso: powershell -ExecutionPolicy Bypass -File installer\build.ps1 [-FfmpegBin <carpeta con ffmpeg.exe y ffprobe.exe>]
param([string]$FfmpegBin)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
Set-Location $root

cargo build --release
if ($LASTEXITCODE) { throw "cargo build falló" }
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

# Por defecto, el ffmpeg del PATH (el de winget es un enlace: se resuelve a su carpeta real)
if (-not $FfmpegBin) {
  $exe = Get-Item (Get-Command ffmpeg).Source
  if ($exe.Target) { $exe = Get-Item $exe.Target }
  $FfmpegBin = $exe.DirectoryName
}
$dest = New-Item -ItemType Directory -Force "$PSScriptRoot\ffmpeg"
Copy-Item "$FfmpegBin\ffmpeg.exe", "$FfmpegBin\ffprobe.exe" $dest
Copy-Item (Join-Path (Split-Path $FfmpegBin) 'LICENSE') $dest
& "$dest\ffmpeg.exe" -hide_banner -version | Select-Object -First 1

$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "No encuentro Inno Setup 6 (winget install JRSoftware.InnoSetup)" }
& $iscc /Q "/DAppVersion=$version" "$PSScriptRoot\slimcast.iss"
if ($LASTEXITCODE) { throw "ISCC falló" }
Get-Item "dist\Slimcast-$version-setup.exe" | Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
