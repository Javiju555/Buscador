#!/usr/bin/env bash

set -euo pipefail

MODEL_FILE="${1:-model_quint8_avx2.onnx}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="${BUSCADOR_MODEL_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/buscador/models/granite-embedding-97m}"
BASE_URL="https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/resolve/main/onnx"
TOKENIZER_URL="https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/resolve/main/tokenizer.json"

case "$MODEL_FILE" in
  model_quint8_avx2.onnx|model.onnx)
    ;;
  *)
    echo "Modelo no soportado: $MODEL_FILE" >&2
    echo "Usa: model_quint8_avx2.onnx o model.onnx" >&2
    exit 1
    ;;
esac

mkdir -p "$MODEL_DIR"

echo "Descargando tokenizer.json en $MODEL_DIR"
curl -L --fail "$TOKENIZER_URL" -o "$MODEL_DIR/tokenizer.json"

echo "Descargando $MODEL_FILE en $MODEL_DIR"
curl -L --fail "$BASE_URL/$MODEL_FILE" -o "$MODEL_DIR/$MODEL_FILE"

cat <<EOF

Modelo instalado en:
  $MODEL_DIR

Buscador preferirá automáticamente:
  1. model_quint8_avx2.onnx
  2. model.onnx

Si quieres forzar uno en concreto:
  export BUSCADOR_EMBEDDING_MODEL=$MODEL_FILE
EOF
