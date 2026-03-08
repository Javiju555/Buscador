# Buscador en Zorin OS (Guía rápida)

Esta guía deja el proyecto funcionando en Zorin OS (Ubuntu-based) para:

- ejecutar en modo desarrollo,
- generar build Linux,
- validar atajos/autostart.

## 1) Dependencias del sistema

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  curl \
  wget \
  file \
  pkg-config \
  libssl-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

## 2) Instalar Rust + Bun

### Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup default stable
```

### Bun

```bash
curl -fsSL https://bun.sh/install | bash
source "$HOME/.bashrc" 2>/dev/null || true
source "$HOME/.zshrc" 2>/dev/null || true
```

## 3) Clonar y ejecutar

```bash
git clone https://github.com/Javiju555/Buscador.git
cd Buscador/frontend
bun install
cd ..
cargo tauri dev --no-watch
```

Atajo por defecto: `Ctrl+Space` (fallback `Ctrl+Shift+Space`).

## 4) Build Linux

```bash
cargo tauri build
```

En Linux, Tauri suele generar artefactos como `.deb` y/o `.AppImage` según el entorno.

## 5) Alias útiles en Linux (ya soportados)

Puedes escribir directamente en Buscador:

- `home`, `~`
- `desktop`
- `documents`
- `downloads`
- `config`
- `data`
- `cache`
- `temp`, `tmp`

## 6) Variables de entorno para roots en Linux

`BUSCADOR_ROOTS` en Linux usa `:` como separador:

```bash
export BUSCADOR_ROOTS="$HOME/Documents:$HOME/Projects:$HOME/Downloads"
export BUSCADOR_MAX_FILES="12000"
cargo tauri dev --no-watch
```

## 7) Autostart en Linux

La app crea/elimina el autostart al guardar el ajuste "Iniciar con Windows" (usado como toggle de inicio automático cross-platform).

Ruta esperada de autostart:

- `$XDG_CONFIG_HOME/autostart/com.buscador.launcher.desktop`
- o `~/.config/autostart/com.buscador.launcher.desktop`

## 8) Troubleshooting rápido

- Si el atajo global falla en Wayland, prueba sesión X11 (muchos compositores limitan hooks globales).
- Si `cargo tauri dev` falla por WebKit, revisa que `libwebkit2gtk-4.1-dev` esté instalado.
- Si Bun no aparece en PATH, reinicia sesión o carga tu shell rc (`~/.bashrc` / `~/.zshrc`).

## 9) Ubicación de settings en Linux

- `$XDG_CONFIG_HOME/buscador-launcher/settings.json`
- o `~/.buscador-launcher/settings.json`
