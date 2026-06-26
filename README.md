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
- Filesystem path navigation with autocomplete
- Semantic search with local ONNX embeddings
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
- Absolute path (e.g. `/home/user/docs` or `C:\Users\`): filesystem navigation with prefix autocomplete

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

## Local Embeddings

Buscador can load the IBM Granite multilingual embedding model locally to power semantic matching and hybrid fuzzy + vector ranking.

- Preferred model: `onnx/model_quint8_avx2.onnx` from `ibm-granite/granite-embedding-97m-multilingual-r2`
- Fallback model: `onnx/model.onnx`
- Tokenizer: `tokenizer.json`
- Embedding size: 384 dimensions

The loader prefers the 8-bit model automatically because it is much smaller, and falls back to the 32-bit model if that is the only file present.

Expected install directory:

- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/buscador/models/granite-embedding-97m`
- Windows: `%LOCALAPPDATA%\buscador\models\granite-embedding-97m`

Install helpers:

```bash
./scripts/fetch-embedding-model.sh
./scripts/fetch-embedding-model.sh model.onnx
```

```powershell
./scripts/fetch-embedding-model.ps1
./scripts/fetch-embedding-model.ps1 -ModelFile model.onnx
```

Optional override:

```bash
export BUSCADOR_EMBEDDING_MODEL=model.onnx
```

```powershell
$env:BUSCADOR_EMBEDDING_MODEL = "model_quint8_avx2.onnx"
```

Notes:

- `model_quint8_avx2.onnx` is about 98 MB in the upstream Hugging Face repository, while `model.onnx` is about 390 MB.
- The `avx2` variant is the default because it cuts download size and startup footprint significantly on modern CPUs.
- If a machine does not support that variant, keep `tokenizer.json` and add `model.onnx`; Buscador will fall back to it automatically.

## Configuration & Settings

You can open the **Settings** dialog by clicking the gear icon on the top right. Here is what each setting does:

- **Root folders (Carpetas raíz)**: Semicolon-separated (`;`) list of absolute paths. These directories are recursively scanned by the fast fuzzy name indexer so you can find files and directories instantly by typing their name or path (e.g., `D:\Documents;D:\Projects`).
- **Max files (Máximo de archivos)**: The maximum limit of files indexed by the fuzzy name indexer (defaults to `25000` to prevent excessive RAM consumption on very large drives).
- **Folders for semantic search (Carpetas para búsqueda semántica)**: Semicolon-separated (`;`) list of paths to index for semantic content matching. The embedding engine will read the contents of files in these folders and create vectors.
  > [!TIP]
  > Because reading file contents and generating vector embeddings is CPU-intensive, it is recommended to scope this only to your notes or document folders (e.g., `~/Documents;~/Notes`), rather than entire disk drives.
- **Web provider (Proveedor web)**: Semicolon-separated name of your preferred search engine (e.g., `brave` or `google`). Trigger a web search using the `w ` query prefix (e.g., `w weather in Madrid`).
- **Web API key (API key web)**: If using `brave` search, pasting a valid Brave Search API key will show live internet results inline directly inside the launcher window, instead of opening a browser tab.
- **Start with Windows (Iniciar con Windows)**: Toggles automatic launch at system startup.

### Internationalization (i18n)

Buscador supports both English (`en`) and Spanish (`es`). The language is automatically detected at startup based on your system/browser language (`navigator.language`).

## Notes

- On Linux, the Tauri hooks also support Bun installed at `$HOME/.bun/bin/bun`.
- The repository still contains a legacy GNOME extension and setup notes because some users still rely on that path.
- Web search is optional and works without an API key by falling back to opening the browser search.

## License

[AGPL-3.0-only](LICENSE)
