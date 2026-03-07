# GNOME Shell Companion Extension

Esta microextension esta pensada para GNOME Shell 49 y oculta la ventana principal de Buscador del Dock y del selector de ventanas usando `Meta.Window.hide_from_window_list()`.

## Que hace

- Detecta ventanas cuyo `gtk_application_id` sea `com.buscador.launcher`
- Usa fallback por `wm_class` / titulo `Buscador`
- Reaplica el ocultado cuando Tauri recrea la ventana
- No toca el estilo del launcher ni su frontend

## Instalar en tu sesion

```bash
./scripts/install-gnome-extension.sh
```

Eso sincroniza una copia en:

```text
~/.local/share/gnome-shell/extensions/buscador-launcher-companion@javiju
```

En GNOME Shell 49 puede hacer falta cerrar sesion una vez para que el shell reindexe una extension nueva. El instalador deja el UUID anadido a `enabled-extensions` para que suba automaticamente en el siguiente login.

## Recargar durante desarrollo

```bash
./scripts/install-gnome-extension.sh
gnome-extensions disable buscador-launcher-companion@javiju
gnome-extensions enable buscador-launcher-companion@javiju
```

En Wayland, si GNOME Shell no recoge el cambio, la salida segura es cerrar sesion.

## Ver logs

```bash
journalctl --user -f /usr/bin/gnome-shell
```

Busca lineas con:

```text
[buscador-launcher-companion@javiju]
```

## Si cambia el app id

La extension asume el identificador Tauri actual:

```text
com.buscador.launcher
```

Si cambias `identifier` en `src-tauri/tauri.conf.json`, actualiza tambien `APP_ID` en `gnome-shell-extension/buscador-launcher-companion@javiju/extension.js`.
