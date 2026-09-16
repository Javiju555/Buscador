# Buscador

[![CI](https://github.com/Javiju555/Buscador/actions/workflows/ci.yml/badge.svg)](https://github.com/Javiju555/Buscador/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/Javiju555/Buscador?display_name=tag)](https://github.com/Javiju555/Buscador/releases/latest)
[![License](https://img.shields.io/github/license/Javiju555/Buscador)](LICENSE)

Buscador is a fast, keyboard-first launcher for Windows and Linux. Search applications, commands, files and folders from one small window, open a full filesystem path, calculate an expression, or search the web without leaving the keyboard.

It is built with Tauri 2, Rust, TypeScript and Bun. Semantic search is optional and runs locally with an ONNX embedding model.

## Highlights

- Global shortcut launcher with native app icons.
- Application and command search.
- Fast file-name indexing with configurable roots.
- Full-path navigation with directory and file autocomplete.
- Local hybrid fuzzy + semantic search.
- Inline calculator and optional web search.
- Autostart support on Windows and Linux.
- Optional GNOME Shell companion extension for hiding the launcher from the dock and window switcher.

## Install

Download the latest native package from the [Releases page](https://github.com/Javiju555/Buscador/releases/latest):

- **Windows:** install the `.msi` package or use the NSIS installer.
- **Debian/Ubuntu:** install the `.deb` package.
- **Fedora/openSUSE:** install the `.rpm` package.
- **Other Linux distributions:** run the `.AppImage`.

The release workflow publishes Windows and Linux installers for every `v*` tag.

## How to use it

Press `Ctrl+Space` to show or hide the launcher. Type a query and press `Enter` to open the selected result.

| Input | Action |
| --- | --- |
| `firefox` | Search applications, commands and indexed files. |
| `>cargo` | Search commands only. |
| `/report` | Search files by name. |
| `=23 * 7` | Calculate an expression. |
| `w weather in Madrid` | Search the web. |
| `/home/user/Doc` | Navigate a Linux path and autocomplete children. |
| `C:\Users\me\Doc` | Navigate a Windows path and autocomplete children. |

### Opening paths and folders

Yes, path navigation is supported. Enter an absolute path and Buscador will show the exact file or folder when it exists, followed by matching children. Add a trailing slash (or backslash on Windows) to list the contents of a directory.

Examples:

```text
/home/javier/Downloads/
/home/javier/Downloads/report
C:\Users\Javier\Documents\
```

You can also type common directory aliases directly:

```text
home       documents       downloads       desktop
config     data            cache            temp
```

Aliases resolve to the current user's directories. A bare folder name is found when it belongs to an indexed root; an absolute path always uses direct filesystem navigation and does not depend on indexing.

## Settings

Open the gear button to configure the launcher:

- **Root folders:** directories scanned for fast file-name search.
- **Maximum files:** upper bound for the file-name index.
- **Semantic folders:** directories whose file names, paths and short previews of supported text files are included in semantic search.
- **Results limit:** number of visible results.
- **Web provider and API key:** optional live web search configuration.
- **Start at login:** enable or disable autostart.

Settings are stored at:

- Linux: `${XDG_CONFIG_HOME:-$HOME/.config}/fenix/buscador.json`.
- Windows: `%LOCALAPPDATA%\BuscadorLauncher\settings.json`.

The Linux `fenix` directory is a compatibility namespace used by existing installations.

## Semantic search

Semantic search is optional. Without a model, normal fuzzy search and path navigation continue to work.

The preferred model is IBM Granite multilingual embeddings in ONNX format:

- `model_quint8_avx2.onnx` (smaller, preferred on modern CPUs).
- `model.onnx` (fallback).
- `tokenizer.json`.

Download it with one of the included helpers:

```bash
./scripts/fetch-embedding-model.sh
```

```powershell
.\scripts\fetch-embedding-model.ps1
```

See [the architecture guide](docs/architecture.md) for the indexing boundaries and [the HTTP API reference](docs/http-api.md) for integrations.

## Linux integration

The repository includes a local installer that builds a release binary and creates the desktop entry, autostart entry and `Ctrl+Space` shortcut:

```bash
./scripts/install-local-linux.sh
```

For prerequisites, development setup and troubleshooting, read [docs/linux.md](docs/linux.md). The optional GNOME Shell integration is documented in [docs/gnome-shell-extension.md](docs/gnome-shell-extension.md).

## Development

Requirements:

- Rust stable and Cargo.
- Bun.
- Tauri 2 Linux or Windows prerequisites.

Install frontend dependencies and run the development app:

```bash
cd frontend
bun install --frozen-lockfile
cd ../src-tauri
cargo tauri dev --no-watch
```

Build a release locally:

```bash
cd src-tauri
cargo tauri build
```

Run the Rust checks and tests:

```bash
cd src-tauri
cargo check
cargo test vector_store -- --nocapture
```

The CI workflow runs these checks on Ubuntu and Windows. Release packages are generated by [`.github/workflows/release.yml`](.github/workflows/release.yml) when a `v*` tag is pushed.

## Project documentation

- [Architecture and data flow](docs/architecture.md)
- [Local HTTP API](docs/http-api.md)
- [Linux setup and installation](docs/linux.md)
- [GNOME Shell companion](docs/gnome-shell-extension.md)

## License

[MIT](LICENSE)
