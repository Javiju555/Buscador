# Linux setup

These instructions cover Ubuntu, Debian and closely related distributions such as Zorin OS. The release packages are the easiest installation path; this page is for development and local installation.

## Runtime dependencies

For a local build, install the Tauri/WebKit development packages:

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
  libsoup-3.0-dev \
  libsoup2.4-dev \
  libwebkit2gtk-4.1-dev \
  libjavascriptcoregtk-4.1-dev \
  libxdo-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

Install Rust stable with [rustup](https://rustup.rs/) and install [Bun](https://bun.sh/). Restart the shell or load the corresponding environment files so both `cargo` and `bun` are on `PATH`.

## Development build

```bash
git clone https://github.com/Javiju555/Buscador.git
cd Buscador/frontend
bun install --frozen-lockfile
cd ../src-tauri
cargo tauri dev --no-watch
```

The default shortcut is `Ctrl+Space`. The fallback `Ctrl+Shift+Space` is useful on desktop environments that reserve the primary combination.

## Local release installation

From the repository root:

```bash
./scripts/install-local-linux.sh
```

The script builds a release binary and installs it at:

```text
~/.local/bin/buscador
```

It also creates the desktop entry, login autostart entry, icon and GNOME shortcut helper. The installed app reads the same settings and data directories as a packaged build.

## Path aliases

The launcher resolves these Linux aliases when the target directory exists:

```text
home  ~  desktop  documents  docs  downloads
config  data  cache  temp  tmp
```

Absolute paths support exact opening and prefix completion. For example, type `/home/user/Downloads/` to list its immediate children.

## Optional environment variables

```bash
export BUSCADOR_ROOTS="$HOME/Documents:$HOME/Projects:$HOME/Downloads"
export BUSCADOR_MAX_FILES="12000"
export BUSCADOR_HTTP_PORT="8755"
```

Use `:` between paths on Linux. Settings saved in the UI take precedence for normal configuration; these variables are useful for session-specific overrides.

## Autostart and shortcut

The installed desktop files are:

```text
~/.local/share/applications/com.buscador.launcher.desktop
~/.config/autostart/com.buscador.launcher.desktop
```

The helper used by the shortcut is `~/.local/bin/buscador-toggle`. To disable autostart, turn off **Start at login** in Settings and log in again.

## Troubleshooting

- If the global shortcut does not work under Wayland, try an X11 session or use the desktop entry directly.
- If the build cannot find WebKit, verify that `libwebkit2gtk-4.1-dev` and its dependencies are installed.
- If `bun` or `cargo` is not found after installation, restart the shell and reload the Rust/Bun environment.
- If semantic search is unavailable, install the embedding model from the Settings dialog or run `scripts/fetch-embedding-model.sh`.
