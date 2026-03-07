#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_DIR="${HOME}/.local/bin"
APPLICATIONS_DIR="${HOME}/.local/share/applications"
AUTOSTART_DIR="${HOME}/.config/autostart"
BIN_PATH="${INSTALL_DIR}/buscador"
DESKTOP_PATH="${APPLICATIONS_DIR}/buscador.desktop"
AUTOSTART_PATH="${AUTOSTART_DIR}/buscador.desktop"
ICON_PATH="${APPLICATIONS_DIR}/icons/buscador.png"

if [[ -f "${HOME}/.zshrc" ]]; then
  # Match the user's interactive Arch environment so bun/cargo are available.
  set +u
  source "${HOME}/.zshrc"
  set -u
fi

if [[ -f "${HOME}/.cargo/env" ]]; then
  source "${HOME}/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo no esta disponible en PATH" >&2
  exit 1
fi

if ! command -v bun >/dev/null 2>&1; then
  echo "bun no esta disponible en PATH" >&2
  exit 1
fi

echo "Compilando release..."
(cd "${ROOT_DIR}/frontend" && bun run build)
(cd "${ROOT_DIR}" && cargo build --release --manifest-path src-tauri/Cargo.toml)

RELEASE_BIN=""
for candidate in \
  "${ROOT_DIR}/src-tauri/target/release/buscador_tauri" \
  "${ROOT_DIR}/src-tauri/target/release/buscador"
do
  if [[ -x "${candidate}" ]]; then
    RELEASE_BIN="${candidate}"
    break
  fi
done

if [[ -z "${RELEASE_BIN}" ]]; then
  echo "No se encontro binario release despues del build" >&2
  exit 1
fi

mkdir -p "${INSTALL_DIR}" "${APPLICATIONS_DIR}" "${AUTOSTART_DIR}" "$(dirname "${ICON_PATH}")"

install -m 0755 "${RELEASE_BIN}" "${BIN_PATH}"
install -m 0644 "${ROOT_DIR}/src-tauri/icons/128x128.png" "${ICON_PATH}"

cat > "${DESKTOP_PATH}" <<EOF
[Desktop Entry]
Type=Application
Name=Buscador
Exec=${BIN_PATH}
Icon=${ICON_PATH}
Terminal=false
Categories=Utility;
StartupNotify=false
EOF

cat > "${AUTOSTART_PATH}" <<EOF
[Desktop Entry]
Type=Application
Name=Buscador
Exec=${BIN_PATH}
Icon=${ICON_PATH}
Terminal=false
Hidden=false
NoDisplay=false
StartupNotify=false
X-GNOME-Autostart-enabled=true
EOF

chmod 0644 "${DESKTOP_PATH}" "${AUTOSTART_PATH}"

echo "Instalado en: ${BIN_PATH}"
echo "Desktop entry: ${DESKTOP_PATH}"
echo "Autostart: ${AUTOSTART_PATH}"
