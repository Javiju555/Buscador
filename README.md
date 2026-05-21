# Buscador

Spotlight-style launcher for Windows and Linux.

Buscador searches apps, commands and files, opens results fast and includes inline calculator behavior for quick queries. It started as part of the Fenix desktop environment, but it is useful enough to stand on its own as a small launcher project.

## Status

- Daily-use app
- Windows and Linux supported
- GNOME and Wayland flows are regularly exercised
- KDE should be possible, but it is not part of the regular validation loop yet
- Some older GNOME-specific integration files are still kept in the repository for compatibility

## Features

- Global shortcut launcher
- App search
- Command search
- File indexing and search
- Inline calculator mode
- Optional web search mode
- Native icons
- Autostart support on Windows and Linux

## Query Modes

- Default: mixed search
- `>text`: commands
- `/text`: files
- `=expr`: calculator
- `w text`: web search

## Stack

- Frontend: Vite + TypeScript + Bun
- Backend: Tauri v2 + Rust

## Build

Requirements:

- Rust toolchain
- Bun
- Tauri prerequisites for your platform

Development:

```bash
cd src-tauri
cargo tauri dev --no-watch
```

Release build:

```bash
cd src-tauri
cargo tauri build
```

## Notes

- On Linux, the Tauri hooks also support Bun installed at `$HOME/.bun/bin/bun`.
- The repository still contains a legacy GNOME extension and setup notes because some users still rely on that path.
- Web search is optional and works without an API key by falling back to opening the browser search.

## License

[AGPL-3.0-only](LICENSE)
