param(
  [ValidateSet("model_quint8_avx2.onnx", "model.onnx")]
  [string]$ModelFile = "model_quint8_avx2.onnx",
  [string]$ModelDir = $env:BUSCADOR_MODEL_DIR
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ModelDir)) {
  $localAppData = [Environment]::GetFolderPath("LocalApplicationData")
  $ModelDir = Join-Path $localAppData "buscador\models\granite-embedding-97m"
}

$baseUrl = "https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/resolve/main/onnx"
$tokenizerUrl = "https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/resolve/main/tokenizer.json"

New-Item -ItemType Directory -Path $ModelDir -Force | Out-Null

Write-Host "Descargando tokenizer.json en $ModelDir"
Invoke-WebRequest -Uri $tokenizerUrl -OutFile (Join-Path $ModelDir "tokenizer.json")

Write-Host "Descargando $ModelFile en $ModelDir"
Invoke-WebRequest -Uri "$baseUrl/$ModelFile" -OutFile (Join-Path $ModelDir $ModelFile)

Write-Host ""
Write-Host "Modelo instalado en:" -ForegroundColor Green
Write-Host "  $ModelDir"
Write-Host ""
Write-Host "Buscador preferirá automáticamente:"
Write-Host "  1. model_quint8_avx2.onnx"
Write-Host "  2. model.onnx"
Write-Host ""
Write-Host "Para forzar uno en concreto en la sesión actual:"
Write-Host "  `$env:BUSCADOR_EMBEDDING_MODEL = `"$ModelFile`""
