#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UUID="buscador-launcher-companion@javiju"
SOURCE_DIR="$ROOT_DIR/gnome-shell-extension/$UUID"
TARGET_DIR="$HOME/.local/share/gnome-shell/extensions/$UUID"

if [[ ! -d "$SOURCE_DIR" ]]; then
  echo "No se encontro la extension en $SOURCE_DIR" >&2
  exit 1
fi

mkdir -p "$(dirname "$TARGET_DIR")"

if [[ -L "$TARGET_DIR" ]]; then
  unlink "$TARGET_DIR"
fi

mkdir -p "$TARGET_DIR"
cp -a "$SOURCE_DIR/." "$TARGET_DIR/"

gnome-extensions disable "$UUID" >/dev/null 2>&1 || true

if ! gnome-extensions enable "$UUID"; then
  current_enabled="$(gsettings get org.gnome.shell enabled-extensions)"
  if [[ "$current_enabled" != *"$UUID"* ]]; then
    python - "$UUID" "$current_enabled" <<'PY'
import ast
import subprocess
import sys

uuid = sys.argv[1]
current = ast.literal_eval(sys.argv[2])
if uuid not in current:
    current.append(uuid)
subprocess.run(
    ["gsettings", "set", "org.gnome.shell", "enabled-extensions", str(current)],
    check=True,
)
PY
  fi

  echo "GNOME Shell no ha recargado el inventario de extensiones en caliente." >&2
  echo "La extension queda marcada para activarse en la proxima apertura de sesion." >&2
fi

echo "Extension instalada y activada: $UUID"
echo "Ruta activa: $TARGET_DIR"
echo "Origen del repo: $SOURCE_DIR"
echo "Si haces cambios en el repo, vuelve a ejecutar este script para sincronizarlos."
echo "Si no ves cambios inmediatos, cierra sesion y vuelve a entrar."
