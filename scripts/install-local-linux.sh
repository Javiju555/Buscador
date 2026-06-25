#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_DIR="${HOME}/.local/bin"
APPLICATIONS_DIR="${HOME}/.local/share/applications"
AUTOSTART_DIR="${HOME}/.config/autostart"
DESKTOP_ID="com.buscador.launcher"
BIN_PATH="${INSTALL_DIR}/buscador"
TOGGLE_PATH="${INSTALL_DIR}/buscador-toggle"
DESKTOP_PATH="${APPLICATIONS_DIR}/${DESKTOP_ID}.desktop"
AUTOSTART_PATH="${AUTOSTART_DIR}/${DESKTOP_ID}.desktop"
ICON_PATH="${APPLICATIONS_DIR}/icons/buscador.png"

if [[ -f "${HOME}/.zshrc" ]]; then
  # Match the user's interactive Arch environment so bun/cargo are available.
  set +e
  set +u
  source "${HOME}/.zshrc"
  set -e
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
(cd "${ROOT_DIR}" && cargo tauri build --no-bundle)

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
rm -f "${APPLICATIONS_DIR}/buscador.desktop" "${AUTOSTART_DIR}/buscador.desktop"
cat > "${TOGGLE_PATH}" <<EOF
#!/usr/bin/env zsh
set -euo pipefail

APP_BIN="${BIN_PATH}"
SOCKET_PATH="\${XDG_RUNTIME_DIR:-\${HOME}/.cache}/com.buscador.launcher.sock"

send_toggle() {
  python - "\${SOCKET_PATH}" <<'PY'
import socket
import sys

path = sys.argv[1]

try:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.35)
    client.connect(path)
    client.sendall(b"toggle\\n")
    client.close()
except Exception:
    raise SystemExit(1)

raise SystemExit(0)
PY
}

if send_toggle; then
  exit 0
fi

if command -v gtk-launch >/dev/null 2>&1; then
  nohup gtk-launch com.buscador.launcher >/dev/null 2>&1 & disown
else
  nohup "\${APP_BIN}" >/dev/null 2>&1 & disown
fi

for _ in {1..30}; do
  sleep 0.1
  if send_toggle; then
    exit 0
  fi
done

exit 0
EOF
chmod 0755 "${TOGGLE_PATH}"

cat > "${DESKTOP_PATH}" <<EOF
[Desktop Entry]
Type=Application
Name=Buscador
Exec=${BIN_PATH}
Icon=${ICON_PATH}
Terminal=false
Categories=Utility;
StartupNotify=false
StartupWMClass=com.buscador.launcher
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
StartupWMClass=com.buscador.launcher
EOF

chmod 0644 "${DESKTOP_PATH}" "${AUTOSTART_PATH}"

if command -v gsettings >/dev/null 2>&1; then
  export BUSCADOR_TOGGLE_PATH="${TOGGLE_PATH}"
  python - <<'PY'
import ast
import os
import subprocess

SCHEMA = "org.gnome.settings-daemon.plugins.media-keys"
BASE = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/"
TOGGLE = os.environ["BUSCADOR_TOGGLE_PATH"]

def get(cmd):
    return subprocess.check_output(cmd, text=True).strip()

def set_value(schema, path, key, value):
    subprocess.check_call([
        "gsettings",
        "set",
        f"{schema}:{path}",
        key,
        value,
    ])

current = ast.literal_eval(get(["gsettings", "get", SCHEMA, "custom-keybindings"]))

target = None
for path in current:
    base = f"{SCHEMA}.custom-keybinding:{path}"
    try:
        name = get(["gsettings", "get", base, "name"]).strip("'")
    except subprocess.CalledProcessError:
        continue
    if name == "Buscador":
        target = path
        break

if target is None:
    index = 0
    existing = set(current)
    while True:
        candidate = f"{BASE}custom{index}/"
        if candidate not in existing:
            target = candidate
            current.append(candidate)
            subprocess.check_call([
                "gsettings",
                "set",
                SCHEMA,
                "custom-keybindings",
                str(current),
            ])
            break
        index += 1

binding_schema = f"{SCHEMA}.custom-keybinding"
set_value(binding_schema, target, "name", "'Buscador'")
set_value(binding_schema, target, "command", f"'{TOGGLE}'")
set_value(binding_schema, target, "binding", "'<Control>space'")
PY
fi

echo "Instalado en: ${BIN_PATH}"
echo "Toggle helper: ${TOGGLE_PATH}"
echo "Desktop entry: ${DESKTOP_PATH}"
echo "Autostart: ${AUTOSTART_PATH}"
