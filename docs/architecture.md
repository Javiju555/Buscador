# Architecture

Buscador is a Tauri desktop application with a TypeScript frontend and a Rust backend.

## Components

```text
Tauri window
├── frontend/        Vite + TypeScript launcher UI
└── src-tauri/       Rust application and platform integration
    ├── app catalog  Installed applications and desktop entries
    ├── command catalog
    ├── file catalog Fast file-name/path index
    ├── search service Query parsing and ranking
    ├── vector store SQLite embeddings
    └── HTTP server Local integration API
```

### Frontend

The frontend owns the launcher window, query input, result rendering, settings dialog and translations. Tauri commands connect it to the Rust process.

### Application and command catalogs

The application catalog discovers native applications and desktop entries. On Windows it also scans the Start Menu and configured executable roots. The command catalog exposes commands available on the current system.

### File catalog

The file catalog is a lightweight name/path index. It scans the configured roots recursively, skips common build and dependency directories, and does not read file contents. If no custom roots are configured, it uses the platform's common user directories. The separate semantic indexer can read short previews from supported text files when the user explicitly configures semantic roots.

### Path navigation

Absolute paths take a separate path-navigation branch before normal file search:

1. An existing exact path is returned first.
2. The parent directory is read.
3. Matching children are filtered by prefix.
4. Directories are listed before files.

This is why typing `/home/user/Doc` or `C:\Users\user\Doc` can open a folder or executable without waiting for the file index. A trailing separator lists the immediate contents of the directory.

### Semantic search

Semantic search is optional. When the local Granite ONNX model is available, Buscador generates 384-dimensional embeddings and stores them in SQLite. Hybrid search combines the normal fuzzy results with vector results. The vector store is a brute-force cosine search, which keeps the implementation simple for the expected local dataset size.

The default data directory is:

- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/buscador/`.
- Windows: `%LOCALAPPDATA%\buscador\`.

It contains `vectors.db` and, when downloaded, `models/granite-embedding-97m/`.

### Local HTTP server

The backend starts a local HTTP server on `127.0.0.1:8755` by default. It can be changed with `BUSCADOR_HTTP_PORT`. The server is intended for local integrations; it has no authentication and must not be exposed to a network interface. See [http-api.md](http-api.md).

## Configuration flow

1. Settings are loaded at startup.
2. The app and file catalogs start with those roots.
3. Saving settings normalizes the values and refreshes the catalogs.
4. Semantic roots are indexed in the background when a model is available.
5. Autostart is updated with the saved setting.

On Linux, the settings file is `${XDG_CONFIG_HOME:-$HOME/.config}/fenix/buscador.json`. On Windows it is `%LOCALAPPDATA%\BuscadorLauncher\settings.json`.

## Platform integration

- Tauri provides the native window, packaging and application lifecycle.
- Global shortcuts use the Tauri global-shortcut plugin.
- Linux uses desktop entries and an autostart entry for login launch.
- The optional GNOME companion hides the launcher from shell chrome.
- Windows uses the current user's Run entry for autostart.
