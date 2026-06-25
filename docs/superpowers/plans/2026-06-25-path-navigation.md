# Path Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the user types or pastes an absolute path, Buscador shows matching filesystem entries with autocomplete as they type — and recognises `.desktop` files as apps with name and icon.

**Architecture:** Add `SearchMode::Path` to `search_service.rs`. `parse_mode()` activates it for multi-segment Linux paths and Windows drive-letter paths, before the existing `File` branch. `build_path_results()` does a live `read_dir` on the parent directory, returns an exact-match result at score 2000 plus prefix-filtered siblings at score 1500, sorted dirs-first. `.desktop` files get `kind=App` so the existing icon pipeline applies.

**Tech Stack:** Rust, `std::fs`, `std::path::Path` (already imported), Tauri `SearchResultKind` model.

## Global Constraints

- Only `src-tauri/src/search_service.rs` is modified — no other files.
- Existing `SearchMode::File` behaviour (single-segment `/keyword`) must be unchanged.
- Results use the existing `SearchResult` and `SearchResultKind` types verbatim.
- Tests live in a `#[cfg(test)] mod tests` block at the bottom of `search_service.rs`, consistent with the rest of the codebase (see `vector_store.rs:319`).
- Test command: `cd src-tauri && cargo test` (run from repo root as `cd src-tauri && cargo test`).
- Hidden entries (names starting with `.`) are shown only when the user's stem also starts with `.`.
- Max results capped by the `limit` parameter passed to `build_path_results`.

---

### Task 1: Add `SearchMode::Path` and update `parse_mode`

**Files:**
- Modify: `src-tauri/src/search_service.rs`

**Interfaces:**
- Produces: `SearchMode::Path` variant; `parse_mode("/home/u/docs")` → `(SearchMode::Path, "/home/u/docs")`; `parse_mode("/algo")` (non-existent single-segment) → `(SearchMode::File, "algo")` unchanged.

- [ ] **Step 1: Write the failing tests**

Add this block at the bottom of `src-tauri/src/search_service.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_mode: Path detection ---

    #[test]
    fn parse_mode_path_multi_segment_linux() {
        // Multi-segment always → Path regardless of existence
        let (mode, q) = parse_mode("/home/user/documents");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "/home/user/documents");
    }

    #[test]
    fn parse_mode_path_trailing_slash() {
        let (mode, q) = parse_mode("/home/user/");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "/home/user/");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_mode_path_existing_single_segment() {
        // /tmp always exists on Linux → Path mode even with one segment
        let (mode, q) = parse_mode("/tmp");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "/tmp");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_mode_file_nonexistent_single_segment() {
        // Non-existent single-segment stays as File mode
        let (mode, q) = parse_mode("/zzz_buscador_no_exist");
        assert_eq!(mode, SearchMode::File);
        assert_eq!(q, "zzz_buscador_no_exist");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_mode_path_windows_backslash() {
        let (mode, q) = parse_mode(r"C:\Users\test");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, r"C:\Users\test");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_mode_path_windows_forward_slash() {
        let (mode, q) = parse_mode("D:/Projects/app");
        assert_eq!(mode, SearchMode::Path);
        assert_eq!(q, "D:/Projects/app");
    }
}
```

- [ ] **Step 2: Run tests — expect compile error (SearchMode::Path doesn't exist yet)**

```bash
cd src-tauri && cargo test 2>&1 | grep "error\|FAILED"
```

Expected: compile error mentioning `SearchMode::Path` or `no variant`.

- [ ] **Step 3: Add `Path` variant to `SearchMode`**

In `search_service.rs`, find the enum (around line 130):

```rust
#[derive(PartialEq, Eq)]
enum SearchMode {
    Mixed,
    Command,
    File,
    Path,   // ← add this line
    Web,
    Calculation,
}
```

- [ ] **Step 4: Add Path detection to `parse_mode`**

In `parse_mode`, insert the Path detection **before** the existing `if let Some(stripped) = query.strip_prefix('/')` line. The full function becomes:

```rust
fn parse_mode(raw_query: &str) -> (SearchMode, String) {
    let query = raw_query.trim();
    if let Some(stripped) = query.strip_prefix("w ") {
        return (SearchMode::Web, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix("w:") {
        return (SearchMode::Web, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix('>') {
        return (SearchMode::Command, stripped.trim().to_string());
    }
    if let Some(stripped) = query.strip_prefix('=') {
        return (SearchMode::Calculation, stripped.trim().to_string());
    }

    // Absolute path detection — checked before File mode
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if query.starts_with('/') {
        let after_slash = &query[1..];
        if after_slash.contains('/') || std::path::Path::new(query).exists() {
            return (SearchMode::Path, query.to_string());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let bytes = query.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return (SearchMode::Path, query.to_string());
        }
    }

    if let Some(stripped) = query.strip_prefix('/') {
        return (SearchMode::File, stripped.trim().to_string());
    }
    (SearchMode::Mixed, query.to_string())
}
```

- [ ] **Step 5: Run tests — expect failures only in tests that call functions not yet written**

```bash
cd src-tauri && cargo test 2>&1 | grep -E "FAILED|ok$|error"
```

Expected: the `parse_mode_*` tests pass. Compilation may still fail if the Path branch in `search_internal` needs a handler — if so, add a placeholder arm in `search_internal`'s match/if chain that returns an empty response (see Task 4 for the real wiring):

```rust
// Temporary placeholder — replaced in Task 4
if mode == SearchMode::Path {
    return SearchResponse { results: vec![], file_indexing: false };
}
```

Add this after the existing `Calculation` early-return so the code compiles.

- [ ] **Step 6: Run tests — all parse_mode tests pass**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: `parse_mode_*` tests pass, overall `test result: ok`.

- [ ] **Step 7: Commit**

```bash
cd /home/javiju/proyectos/Buscador
git add src-tauri/src/search_service.rs
git commit -m "feat(path-nav): add SearchMode::Path detection in parse_mode"
```

---

### Task 2: `read_desktop_name` helper

**Files:**
- Modify: `src-tauri/src/search_service.rs`

**Interfaces:**
- Consumes: nothing from Task 1 (standalone helper)
- Produces: `fn read_desktop_name(path: &Path) -> Option<String>` — private function, returns the `Name=` value from `[Desktop Entry]` section of a `.desktop` file, or `None` if unreadable or absent.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block:

```rust
    // --- read_desktop_name ---

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_found() {
        let dir = std::env::temp_dir().join("buscador_test_dn1");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.desktop");
        std::fs::write(&p, "[Desktop Entry]\nName=My Cool App\nExec=myapp\n").unwrap();
        assert_eq!(read_desktop_name(&p), Some("My Cool App".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_outside_section_ignored() {
        let dir = std::env::temp_dir().join("buscador_test_dn2");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.desktop");
        std::fs::write(&p, "Name=Orphan\n[Desktop Entry]\nExec=myapp\n").unwrap();
        // Name= before [Desktop Entry] → None
        assert_eq!(read_desktop_name(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_nonexistent_file() {
        assert_eq!(read_desktop_name(Path::new("/no/such/file.desktop")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_name_stops_at_next_section() {
        let dir = std::env::temp_dir().join("buscador_test_dn3");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("app.desktop");
        std::fs::write(
            &p,
            "[Desktop Entry]\nExec=myapp\n[Other]\nName=Wrong\n",
        ).unwrap();
        assert_eq!(read_desktop_name(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run tests — expect compile error**

```bash
cd src-tauri && cargo test 2>&1 | grep "error\|FAILED"
```

Expected: error `cannot find function read_desktop_name`.

- [ ] **Step 3: Implement `read_desktop_name`**

Add this function to `search_service.rs` (before the `#[cfg(test)]` block):

```rust
#[cfg(target_os = "linux")]
fn read_desktop_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_section = true;
            continue;
        }
        if in_section && line.starts_with('[') {
            break;
        }
        if in_section {
            if let Some(value) = line.strip_prefix("Name=") {
                let name = value.trim().to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}
```

- [ ] **Step 4: Run tests — all desktop_name tests pass**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: `desktop_name_*` tests pass, `test result: ok`.

- [ ] **Step 5: Commit**

```bash
cd /home/javiju/proyectos/Buscador
git add src-tauri/src/search_service.rs
git commit -m "feat(path-nav): add read_desktop_name helper"
```

---

### Task 3: `make_path_result` and `build_path_results`

**Files:**
- Modify: `src-tauri/src/search_service.rs`

**Interfaces:**
- Consumes: `read_desktop_name(path: &Path) -> Option<String>` from Task 2; `SearchResult`, `SearchResultKind` from `crate::models`.
- Produces:
  - `fn make_path_result(path: &Path, score: i32) -> SearchResult` — private
  - `fn build_path_results(raw: &str, limit: usize) -> Vec<SearchResult>` — private, called by `search_internal`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
    // --- build_path_results ---

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_nonexistent_parent_empty() {
        let results = build_path_results("/zzz_buscador_no_parent/file", 10);
        assert!(results.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_exact_match_at_top() {
        // /tmp always exists
        let results = build_path_results("/tmp", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].score, 2000);
        assert_eq!(results[0].primary_value, "/tmp");
        assert_eq!(results[0].kind, SearchResultKind::File);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_prefix_filters_siblings() {
        let base = std::env::temp_dir().join("buscador_test_pr1");
        std::fs::create_dir_all(base.join("alpha")).unwrap();
        std::fs::create_dir_all(base.join("almond")).unwrap();
        std::fs::create_dir_all(base.join("beta")).unwrap();

        let query = format!("{}/al", base.to_string_lossy());
        let results = build_path_results(&query, 10);
        let names: Vec<&str> = results.iter().map(|r| r.title.as_str()).collect();
        assert!(names.contains(&"alpha"), "alpha missing from {:?}", names);
        assert!(names.contains(&"almond"), "almond missing from {:?}", names);
        assert!(!names.contains(&"beta"), "beta should be filtered out");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_dirs_before_files() {
        let base = std::env::temp_dir().join("buscador_test_pr2");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(base.join("zdir")).unwrap();
        std::fs::write(base.join("afile.txt"), "").unwrap();

        let query = format!("{}/", base.to_string_lossy());
        let results = build_path_results(&query, 10);
        // zdir (dir) must come before afile.txt (file) even though 'z' > 'a'
        let dir_pos = results.iter().position(|r| r.title == "zdir").unwrap();
        let file_pos = results.iter().position(|r| r.title == "afile.txt").unwrap();
        assert!(dir_pos < file_pos, "directory should appear before file");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_hidden_excluded_when_stem_visible() {
        let base = std::env::temp_dir().join("buscador_test_pr3");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join(".hidden"), "").unwrap();
        std::fs::write(base.join("visible.txt"), "").unwrap();

        let query = format!("{}/", base.to_string_lossy());
        let results = build_path_results(&query, 10);
        let names: Vec<&str> = results.iter().map(|r| r.title.as_str()).collect();
        assert!(names.contains(&"visible.txt"));
        assert!(!names.contains(&".hidden"), "hidden file should be excluded");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_hidden_included_when_stem_hidden() {
        let base = std::env::temp_dir().join("buscador_test_pr4");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join(".config"), "").unwrap();
        std::fs::write(base.join("visible.txt"), "").unwrap();

        let query = format!("{}/.con", base.to_string_lossy());
        let results = build_path_results(&query, 10);
        let names: Vec<&str> = results.iter().map(|r| r.title.as_str()).collect();
        assert!(names.contains(&".config"), ".config should appear when stem starts with '.'");
        assert!(!names.contains(&"visible.txt"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn path_results_desktop_file_kind_app() {
        let base = std::env::temp_dir().join("buscador_test_pr5");
        std::fs::create_dir_all(&base).unwrap();
        let dp = base.join("myapp.desktop");
        std::fs::write(&dp, "[Desktop Entry]\nName=My App\nExec=myapp\n").unwrap();

        let query = dp.to_string_lossy().to_string();
        let results = build_path_results(&query, 5);
        let app = results.iter().find(|r| r.primary_value == dp.to_string_lossy().as_ref());
        assert!(app.is_some(), "desktop file should appear in results");
        let app = app.unwrap();
        assert_eq!(app.kind, SearchResultKind::App);
        assert_eq!(app.title, "My App");

        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Run tests — expect compile error**

```bash
cd src-tauri && cargo test 2>&1 | grep "error\|FAILED"
```

Expected: errors for `build_path_results` and `make_path_result` not found.

- [ ] **Step 3: Implement `make_path_result`**

Add to `search_service.rs` (before the `#[cfg(test)]` block):

```rust
fn make_path_result(path: &Path, score: i32) -> SearchResult {
    let primary_value = path.to_string_lossy().to_string();
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| primary_value.clone());

    #[cfg(target_os = "linux")]
    if path.extension().map_or(false, |e| e == "desktop") {
        let title = read_desktop_name(path).unwrap_or_else(|| filename.clone());
        return SearchResult {
            kind: SearchResultKind::App,
            title,
            subtitle: primary_value.clone(),
            primary_value,
            score,
        };
    }

    SearchResult {
        kind: SearchResultKind::File,
        title: filename,
        subtitle: primary_value.clone(),
        primary_value,
        score,
    }
}
```

- [ ] **Step 4: Implement `build_path_results`**

Add to `search_service.rs` (after `make_path_result`, before the `#[cfg(test)]` block):

```rust
fn build_path_results(raw: &str, limit: usize) -> Vec<SearchResult> {
    if limit == 0 {
        return vec![];
    }

    let raw = raw.trim();
    let ends_with_sep = raw.ends_with('/') || raw.ends_with('\\');
    let path = Path::new(raw);

    let (list_dir, stem): (&Path, &str) = if ends_with_sep {
        (path, "")
    } else {
        let stem = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => return vec![],
        };
        (parent, stem)
    };

    let stem_lower = stem.to_lowercase();
    let mut results: Vec<SearchResult> = Vec::new();
    let mut exact_path = None;

    if !ends_with_sep && path.exists() {
        results.push(make_path_result(path, 2000));
        exact_path = Some(path.to_path_buf());
    }

    if !list_dir.is_dir() {
        return results;
    }

    let read = match std::fs::read_dir(list_dir) {
        Ok(r) => r,
        Err(_) => return results,
    };

    let mut dirs: Vec<SearchResult> = Vec::new();
    let mut files: Vec<SearchResult> = Vec::new();
    for entry in read.flatten().take(512) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') && !stem.starts_with('.') {
            continue;
        }
        if !stem_lower.is_empty() && !name_str.to_lowercase().starts_with(&stem_lower) {
            continue;
        }

        let entry_path = entry.path();
        if exact_path.as_deref() == Some(entry_path.as_path()) {
            continue;
        }

        let result = make_path_result(&entry_path, 1500);
        if entry_path.is_dir() {
            dirs.push(result);
        } else {
            files.push(result);
        }
    }

    dirs.sort_by(|a, b| a.title.cmp(&b.title));
    files.sort_by(|a, b| a.title.cmp(&b.title));

    let remaining = limit.saturating_sub(results.len());
    results.extend(dirs.into_iter().chain(files).take(remaining));
    results
}
```

- [ ] **Step 5: Run tests — all path_results tests pass**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: all `path_results_*` and `desktop_name_*` tests pass. `test result: ok`.

- [ ] **Step 6: Commit**

```bash
cd /home/javiju/proyectos/Buscador
git add src-tauri/src/search_service.rs
git commit -m "feat(path-nav): implement build_path_results and make_path_result"
```

---

### Task 4: Wire `SearchMode::Path` into `search_internal` and ship

**Files:**
- Modify: `src-tauri/src/search_service.rs`

**Interfaces:**
- Consumes: `build_path_results(raw: &str, limit: usize) -> Vec<SearchResult>` from Task 3; `SearchMode::Path` from Task 1.
- Produces: the complete feature — path queries return real filesystem results.

- [ ] **Step 1: Write the failing integration test**

Add to `mod tests`:

```rust
    // --- integration: Path mode wired into search_internal ---

    #[cfg(target_os = "linux")]
    #[test]
    fn search_path_mode_returns_filesystem_results() {
        // parse_mode + build_path_results together via the public-ish path
        let (mode, query) = parse_mode("/tmp");
        assert_eq!(mode, SearchMode::Path);
        // build_path_results should find /tmp at score 2000
        let results = build_path_results(&query, 5);
        assert!(
            results.iter().any(|r| r.primary_value == "/tmp" && r.score == 2000),
            "Expected /tmp as exact match at score 2000, got {:?}",
            results.iter().map(|r| (&r.primary_value, r.score)).collect::<Vec<_>>()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn search_path_mode_no_bleed_into_file_mode() {
        // Single-segment non-existent path must NOT be Path mode
        let (mode, _) = parse_mode("/zzz_buscador_no_exist");
        assert_eq!(mode, SearchMode::File, "Single non-existent segment must stay as File mode");
    }
```

- [ ] **Step 2: Run tests — confirm they already pass (parse_mode + build_path_results are done)**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: `search_path_mode_*` tests pass. If so, the integration is already proven.

- [ ] **Step 3: Replace the temporary placeholder with the real Path branch**

In `search_internal`, find the placeholder added in Task 1 Step 5:

```rust
// Temporary placeholder — replaced in Task 4
if mode == SearchMode::Path {
    return SearchResponse { results: vec![], file_indexing: false };
}
```

Replace it with:

```rust
if mode == SearchMode::Path {
    return SearchResponse {
        results: build_path_results(&query, limit),
        file_indexing: false,
    };
}
```

- [ ] **Step 4: Run full test suite — everything green**

```bash
cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: `test result: ok. N passed; 0 failed`.

- [ ] **Step 5: Build release binary to confirm no warnings/errors**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error|warning\[" | head -20
```

Expected: no errors. Warnings about unused code are acceptable if unrelated to new code.

- [ ] **Step 6: Final commit**

```bash
cd /home/javiju/proyectos/Buscador
git add src-tauri/src/search_service.rs
git commit -m "feat(path-nav): wire SearchMode::Path into search_internal — filesystem path navigation live"
```

- [ ] **Step 7: Push to GitHub**

```bash
cd /home/javiju/proyectos/Buscador
git push
```
