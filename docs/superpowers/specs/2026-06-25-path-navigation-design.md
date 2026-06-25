# Path Navigation Feature — Design Spec

## Overview

Add direct filesystem path navigation to Buscador. When the user pastes or types
an absolute path, the launcher shows matching entries from the real filesystem
instead of searching the index — with autocomplete as you type, exact matches
highlighted at the top, and app names/icons when the entry is a `.desktop` file.

---

## 1. Detection — new `SearchMode::Path`

`parse_mode()` in `search_service.rs` gains a `Path` branch, checked **before**
the existing `File` branch so single-segment `/keyword` queries (File mode) are
preserved.

**Linux / macOS:** triggers when the raw query starts with `/` AND either:
- contains a second `/` after position 0 (e.g. `/home/javiju/proy`), OR
- `Path::new(query).exists()` (e.g. `/home` — root-level dir, one segment)

**Windows:** triggers when the raw query matches `^[A-Za-z]:[/\\]` (drive-letter
prefix such as `D:\Proyectos` or `C:/Users`).

Queries that do NOT meet these conditions fall through to the existing modes
unchanged. The full raw string is returned as the path query (no prefix stripped).

---

## 2. Path results — `build_path_results(raw: &str, limit: usize)`

New private function in `search_service.rs`, called exclusively from the `Path`
branch of `search_internal`. Returns early with an empty `SearchResponse` if the
parent directory is not readable (permissions, non-existent).

### Logic

```
path  = Path::new(raw)
stem  = path.file_name()          // last path component
parent = path.parent()

// Case A — trailing separator: list the directory itself
if raw ends with '/' or '\':
    list parent = path (the dir itself), stem = "" (no filter)

// Case B — exact match exists
if path.exists():
    push make_path_result(path, score=2000) → results

// Prefix autocomplete from parent
if parent.is_dir():
    entries = read_dir(parent)
        .filter(name starts_with stem, case-insensitive)
        .filter(already in results → skip duplicate)
        .sort(dirs first, then files, alphabetical within each group)
        .take(limit − results.len())
    results.extend(entries.map(make_path_result(_, score=1500)))
```

Hidden entries (name starts with `.` on Linux) are included **only** if `stem`
also starts with `.`, matching standard shell/file-manager convention.

### `make_path_result(path, score) → SearchResult`

| Condition | kind | title | subtitle |
|---|---|---|---|
| `.desktop` file (Linux) | `App` | `Name=` from file (fallback: filename) | full path |
| Any other file or folder | `File` | filename / dirname | full path |

`primary_value` is always the full absolute path string. The existing `execute`
handler already knows how to open both `App` (runs the `.desktop`) and `File`
(`xdg-open` / `explorer`) — no changes needed there.

On Windows `.exe` files: no special name extraction (PE resource parsing is
out of scope). Title = filename.

---

## 3. App name and icon for `.desktop` entries

When `make_path_result` detects a `.desktop` extension, it calls a small helper
`read_desktop_name(path) → Option<String>` that:
1. Reads the file as text.
2. Scans for the `[Desktop Entry]` section.
3. Returns the value of the first `Name=` line found.

The icon is resolved by the existing icon pipeline: `kind=App` +
`primary_value=path_to_desktop_file` → the frontend calls `get_icon` with the
path, same as today for app results. No new icon code.

---

## 4. Integration into `search_internal`

```rust
if mode == SearchMode::Path {
    return SearchResponse {
        results: build_path_results(&query, limit),
        file_indexing: false,
    };
}
```

This is an early-return, identical to the existing `Calculation` and `Web`
early-returns. No mixing with apps, commands, or indexed files.

---

## 5. Edge cases

| Case | Behaviour |
|---|---|
| `read_dir` fails (permissions) | return empty results, no panic |
| Path points to a file (not dir) | parent dir used for autocomplete |
| `stem` is empty (root `/`) | list root directory, capped at `limit` |
| Result count > limit | truncated after sort |
| Symlinks | followed by default (`Path::exists` dereferences) |
| Windows mixed separators (`D:/foo\bar`) | `Path::new` normalises transparently |

---

## 6. Scope / non-goals

- No caching — `read_dir` is fast enough for a single directory.
- No relative path detection — ambiguous with normal search queries.
- No Windows `.exe` name extraction.
- No changes to the frontend, execute handler, or icon pipeline.
- No new Tauri commands — everything is inside `search_service.rs`.

---

## Files changed

| File | Change |
|---|---|
| `src-tauri/src/search_service.rs` | Add `SearchMode::Path`, `build_path_results`, `make_path_result`, `read_desktop_name` |
| `src-tauri/src/models.rs` | No change |
| `frontend/` | No change |
