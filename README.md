# Buscador Launcher (Tauri)

Launcher estilo Spotlight para Windows: `Ctrl+Space`, UI tipo pill con dropdown flotante, glassmorphism y tema automatico light/dark segun tema del sistema.

## Stack

- Frontend: Vite + TypeScript (`frontend/`)
- Backend: Tauri v2 + Rust (`src-tauri/`)

No necesitas implementar todo en Rust: la UI y experiencia van en web, y Rust queda para integracion nativa (hotkey global, busqueda rapida, ejecucion, sistema).

## Funciones actuales

- Hotkey global `Ctrl+Space` (fallback a `Ctrl+Shift+Space`).
- Busqueda de apps de Start Menu (`vscode`, `microsoft vs code`).
- Busqueda de comandos del `PATH`.
- Busqueda de archivos indexados en segundo plano con reindex en caliente.
- Calculadora integrada (`=3029*49` o deteccion automatica en modo mixto).
- Ejecucion de app/comando/archivo y copiar resultado de calculadora.
- Resultados progresivos (apps/comandos primero, archivos despues).
- Iconos nativos por resultado (app/comando/archivo).
- Ajustes integrados para roots y maximo de indexado (`Ctrl+,` o boton `⚙`).

## Requisitos

- Windows 10/11
- Node.js 20+
- Rust stable + cargo
- Tauri CLI (`cargo tauri --version`)

## Ejecutar en desarrollo

```powershell
cargo tauri dev --no-watch
```

## Build

```powershell
cargo tauri build
```

## Ajustes de indexado

Ahora se pueden configurar desde UI:

- Boton `⚙` en la pill (o `Ctrl+,`)
- `Carpetas raiz` separadas por `;`
- `Maximo de archivos` (`3000..100000`)
- `Guardar y reindexar`

Tambien se siguen respetando estas variables si quieres forzarlas externamente:

- `BUSCADOR_ROOTS`
- `BUSCADOR_MAX_FILES`

Ejemplo por entorno:

```powershell
$env:BUSCADOR_ROOTS="D:\Trabajo;D:\Proyectos;C:\Users\TuUsuario\Desktop"
$env:BUSCADOR_MAX_FILES="12000"
cargo tauri dev --no-watch
```

## Atajos y prefijos

- `Ctrl+Space`: abrir/cerrar launcher
- `Ctrl+,`: abrir/cerrar ajustes
- `Enter`: ejecutar seleccion
- `Esc`: ocultar launcher
- `>texto`: priorizar comandos
- `/texto`: solo archivos
- `=expresion`: solo calculadora

## Nota sobre version anterior

La version WPF anterior sigue en `src/BuscadorLauncher` como referencia visual, pero la ruta activa ahora es Tauri.
