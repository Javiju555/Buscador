param(
  [string]$Version = "0.1.0"
)

$workspaceRoot = Split-Path -Path $PSScriptRoot -Parent
$releaseExe = Join-Path $workspaceRoot "src-tauri\target\release\buscador_tauri.exe"

if (-not (Test-Path $releaseExe)) {
  throw "No existe el binario release en '$releaseExe'. Ejecuta primero: cargo tauri build"
}

$portableRoot = Join-Path $workspaceRoot "dist\portable"
$stagingDir = Join-Path $portableRoot "Buscador-portable"
$zipPath = Join-Path $portableRoot ("Buscador_{0}_x64_portable.zip" -f $Version)

New-Item -ItemType Directory -Path $portableRoot -Force | Out-Null
if (Test-Path $stagingDir) {
  Remove-Item -Path $stagingDir -Recurse -Force
}
New-Item -ItemType Directory -Path $stagingDir -Force | Out-Null

Copy-Item -Path $releaseExe -Destination (Join-Path $stagingDir "Buscador.exe") -Force

$portableReadme = @"
Buscador Portable
=================

Uso:
1) Ejecuta Buscador.exe
2) Si quieres iniciar con Windows, ejecuta en PowerShell:
   `$app = "$PWD\Buscador.exe"
   New-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "Buscador" -Value ('"' + `$app + '"') -PropertyType String -Force

Notas:
- Esta edición portable NO modifica autostart automáticamente.
- El autostart inicial automático se aplica en instalación estándar (NSIS/MSI).
"@

Set-Content -Path (Join-Path $stagingDir "README-portable.txt") -Value $portableReadme -Encoding UTF8

if (Test-Path $zipPath) {
  Remove-Item -Path $zipPath -Force
}
Compress-Archive -Path (Join-Path $stagingDir "*") -DestinationPath $zipPath -CompressionLevel Optimal

Write-Host "Portable generado:" -ForegroundColor Green
Write-Host $zipPath
