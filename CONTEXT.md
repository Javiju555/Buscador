# Buscador — Contexto para sesiones IA

> Parte de **Fenix Desktop**. Launcher tipo Spotlight. App principal de navegación del sistema.

## Qué es
Launcher / buscador universal. Abre apps, busca archivos, ejecuta comandos, hace cálculos inline (via calculadora), navega el sistema. Actualmente tiene extensión GNOME Shell + backend Tauri.

## Stack
- Frontend: TypeScript + Vite + Bun (`frontend/`)
- Backend: Rust + Tauri v2 (`src-tauri/`)
- Extensión GNOME Shell: (`gnome-shell-extension/`) — a eliminar al migrar a Hyprland

## Arrancar en desarrollo
```bash
cd src-tauri
cargo tauri dev
```

## Estructura
```
Buscador/
├── frontend/         — UI del launcher
├── src-tauri/        — backend Rust
├── gnome-shell-extension/ — integración GNOME (temporal, migrar a wlr-layer-shell)
├── scripts/          — utilidades
└── docs/             — documentación interna
```

## Integración Fenix Desktop
- **wlr-layer-shell**: en Hyprland, se posiciona como overlay centrado (como Spotlight en macOS)
- **D-Bus cliente**: llama a `com.fenix.Calculadora` para evaluar expresiones matemáticas inline
- **Integración filesystem**: búsqueda de archivos y apertura de apps
- Invocado via atajo global de Hyprland (Super+Space o similar)

## TODOs pendientes
- [ ] Rediseño visual: quitar gradiente actual → estilo Fenix (blur + transparencia)
- [ ] Migrar de extensión GNOME a integración nativa Hyprland (wlr-layer-shell)
- [ ] Integrar D-Bus con calculadora para resultados matemáticos inline
- [ ] Integrar con file-system para búsqueda de archivos
- [ ] Mostrar ventanas abiertas (wlr-foreign-toplevel-management en Hyprland)

## Estado actual
Funcional en GNOME. La extensión GNOME Shell actúa como activador global. El backend Tauri maneja la lógica de búsqueda.

## Identidad visual objetivo
Estilo Fenix: sin gradiente, backdrop-filter blur, panel flotante centrado, bordes redondeados. Ver tema WhiteSur como referencia.
