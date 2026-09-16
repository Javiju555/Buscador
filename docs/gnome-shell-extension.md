# GNOME Shell companion

Buscador's optional GNOME Shell extension keeps the launcher out of the dock and the window switcher while it is running. The extension does not change the launcher UI or search behavior.

## Install

From the repository root:

```bash
./scripts/install-gnome-extension.sh
```

The installer copies the extension to:

```text
~/.local/share/gnome-shell/extensions/buscador-launcher-companion@javiju
```

It also adds the UUID to GNOME's enabled extensions. GNOME Shell may need a new login before it discovers a newly installed extension.

## Reload during development

```bash
./scripts/install-gnome-extension.sh
gnome-extensions disable buscador-launcher-companion@javiju
gnome-extensions enable buscador-launcher-companion@javiju
```

On Wayland, logging out and back in is the reliable way to reload GNOME Shell extensions.

## What it matches

The extension looks for the Tauri application ID `com.buscador.launcher`, with a fallback to the window class or title `Buscador`. If the application ID changes in `src-tauri/tauri.conf.json`, update `APP_ID` in `gnome-shell-extension/buscador-launcher-companion@javiju/extension.js` as well.

## Logs

```bash
journalctl --user -f /usr/bin/gnome-shell
```

Filter for:

```text
[buscador-launcher-companion@javiju]
```
