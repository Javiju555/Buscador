# Buscador — API para Agentes

> **Para el agente de Fenix-AI-Nexus:** Este documento describe cómo interactuar con Buscador vía HTTP. Buscador es un launcher de escritorio con búsqueda semántica (embeddings) que expone una API para que otros servicios indexen y consulten contenido.

## Resumen rápido

- **Puerto:** 8755 (configurable via `BUSCADOR_HTTP_PORT`)
- **Host:** `127.0.0.1` (solo local)
- **Formato:** JSON
- **Autenticación:** Ninguna (solo localhost)

## Qué indexa Buscador (y qué NO)

| Tipo | ¿Se indexa? | Cómo |
|------|-------------|------|
| **Apps** (`.desktop`/Start Menu) | ✅ Siempre | Automático al arrancar/reindex. Solo el nombre |
| **Archivos** (`kind=file`) | ⚙️ Opt-in | Solo en carpetas que el usuario configure en `semanticRoots`. Secuencial e incremental (flush cada 64) para no disparar la RAM |
| **Contenido de archivos texto** | ✅ Preview corto | Solo tipos texto plano pequeños (`.md`, `.txt`, `.json`, `.csv`...), máx. 4 KB del inicio. Resto: solo nombre+ruta |
| **Código fuente** (`.rs`, `.py`...) | ❌ NO | Eliminado: leer miles de archivos disparaba la RAM. Se trata como archivo normal (solo nombre) si está en una carpeta configurada |
| **Correos** (`kind=email`) | ✅ Vía `POST /index` | Argos genera el embedding y lo envía; Buscador no toca el correo en disco |

**Agnóstico al SO/idioma:** las carpetas estándar se obtienen vía XDG (Linux) / Known Folders (Windows), que respetan el idioma (`Escritorio`, `Téléchargements`...). Las carpetas a indexar las elige el usuario; nada está hardcodeado.

**Config (`semanticRoots`):** lista de rutas en `~/.config/fenix/buscador.json` (Linux) o `%LOCALAPPDATA%\BuscadorLauncher\settings.json` (Windows). Acepta `~` y rutas absolutas de cualquier SO. Vacío por defecto.

## Endpoints

### `GET /health`

Health check. Retorna estado y versión.

**Response:**
```json
{
  "status": "ok",
  "version": "0.1.3"
}
```

### `GET /stats`

Estadísticas del vector store (cuántos items indexados, de qué tipo).

**Response:**
```json
{
  "total_items": 85,
  "apps": 85,
  "files": 0,
  "engine_available": true,
  "model_file": "model_quint8_avx2.onnx"
}
```

- `engine_available`: `true` si el modelo de embeddings (Granite 97M) está cargado.
- `model_file`: fichero ONNX activo. Normalmente `model_quint8_avx2.onnx`; fallback a `model.onnx`.
- `apps`: items de tipo "app" (entradas .desktop del sistema).
- `files`: items de tipo "file" (archivos del disco).
- Otros tipos posibles: `"email"`, `"code"`, `"contact"`, custom.

### `GET /search`

Búsqueda unificada. Soporta tres modos.

**Parámetros:**

| Param | Tipo | Default | Descripción |
|-------|------|---------|-------------|
| `q` | string | (requerido) | Texto a buscar |
| `limit` | int | 10 | Máximo 50 resultados |
| `mode` | string | `hybrid` | `hybrid`, `fuzzy`, o `semantic` |

**Modos:**

- **`hybrid`** (default): Combina búsqueda fuzzy (nombre/subtítulo) + semántica (cosine similarity). Merge por score.
- **`fuzzy`**: Solo matching por texto (nombre, subtítulo, ruta). Rápido, no necesita embedding model.
- **`semantic`**: Solo cosine similarity contra vectores indexados. Entiende conceptos, no solo texto.

**Response (hybrid):**
```json
{
  "results": [
    {
      "kind": "app",
      "title": "GIMP",
      "subtitle": "",
      "path": "/usr/share/applications/gimp.desktop",
      "score": 350,
      "similarity": 0.58
    }
  ],
  "total": 1,
  "mode": "hybrid"
}
```

**Campos del resultado:**

| Campo | Tipo | Descripción |
|-------|------|-------------|
| `kind` | string | Tipo de item: `"app"`, `"file"`, `"email"`, `"code"`, etc. |
| `title` | string | Nombre/título del item |
| `subtitle` | string | Descripción o subtítulo |
| `path` | string | Ruta o valor primario (para ejecución/ubicación) |
| `score` | int | Score combinado (fuzzy + semantic). Mayor = mejor |
| `similarity` | float\|null | Cosine similarity (0.0-1.0). Solo presente en resultados semánticos |

**Ejemplo: Búsqueda conceptual**
```bash
curl "http://localhost:8755/search?q=editar+fotos&mode=semantic&limit=5"
# → GIMP, Darktable, Shotwell (aunque no tengan "editar fotos" en el nombre)

curl "http://localhost:8755/search?q=escribir+código&mode=semantic"
# → Visual Studio Code, Zed, Micro

curl "http://localhost:8755/search?q=terminal"
# → Console, kitty, Warp (fuzzy + semántico)
```

### `POST /index`

Indexar items en el vector store. Recibe embeddings pre-calculados o texto para generarlos.

**Request body:**
```json
{
  "items": [
    {
      "id": "email:uuid-o-123",
      "kind": "email",
      "title": "Reunión lunes 15 de junio",
      "subtitle": "juan@empresa.com",
      "path": "inbox/2026/06/email-123.eml",
      "embedding": [0.12, -0.03, 0.45, ...],
      "metadata": {
        "from": "juan@empresa.com",
        "date": "2026-06-13T10:30:00Z",
        "unread": true
      }
    }
  ]
}
```

**Campos del item:**

| Campo | Tipo | Requerido | Descripción |
|-------|------|-----------|-------------|
| `id` | string | ✅ | ID único. Prefijo de tipo: `"app:"`, `"file:"`, `"email:"`, `"code:"` |
| `kind` | string | ✅ | Tipo de item |
| `title` | string | ✅ | Nombre/título visible |
| `subtitle` | string | | Descripción |
| `path` | string | | Ruta o ubicación |
| `embedding` | float[] | ✅* | Vector de 384 dimensiones. *O `text` para generar embedding |
| `text` | string | ✅* | Texto para generar embedding con Granite 97M. *O `embedding` |
| `metadata` | object | | Metadata libre (JSON) |

**Response:**
```json
{
  "indexed": 1,
  "total": 86
}
```

**Ejemplo: Indexar email con embedding pre-calculado (desde Argos)**
```bash
curl -X POST http://localhost:8755/index \
  -H "Content-Type: application/json" \
  -d '{
    "items": [{
      "id": "email:abc-123",
      "kind": "email",
      "title": "Propuesta Q3",
      "subtitle": "maria@empresa.com",
      "path": "inbox/msg-456.eml",
      "embedding": [0.1, 0.2, 0.3, ...],
      "metadata": {"from": "maria@empresa.com", "unread": false}
    }]
  }'
```

**Ejemplo: Indexar archivo con texto (genera embedding en Buscador)**
```bash
curl -X POST http://localhost:8755/index \
  -H "Content-Type: application/json" \
  -d '{
    "items": [{
      "id": "code:/home/user/project/main.rs",
      "kind": "code",
      "title": "main.rs",
      "path": "/home/user/project/main.rs",
      "text": "use tokio::sync::mpsc; fn main() { ... }",
      "metadata": {"language": "rust", "lines": 150}
    }]
  }'
```

## Esquemas de IDs

El `id` de cada item debe seguir el formato `tipo:identificador`:

| Prefijo | Tipo | Ejemplo |
|---------|------|---------|
| `app:` | Aplicación del sistema | `app:gimp`, `app:firefox` |
| `file:` | Archivo del disco | `file:/home/user/doc.md` |
| `email:` | Correo electrónico | `email:uuid-123` |
| `code:` | Archivo de código fuente | `code:/path/to/main.rs` |
| `contact:` | Contacto | `contact:email@domain.com` |
| `session:` | Sesión de agente | `session:blazer-uuid` |
| custom | Cualquier otro | `custom:mi-id` |

## Flujo típico: Argos → Nexus → Buscador

```
1. Argos detecta nuevo email
2. Argos genera embedding con Gemma (384 dims)
3. Argos envía a Nexus: POST /api/index { items: [...] }
4. Nexus reenvía a Buscador: POST http://localhost:8755/index
5. Usuario busca "reunión lunes" en Buscador
6. Buscador encuentra el email por cosine similarity
```

## Notas para el agente de Nexus

- **El modelo de embedding es Granite 97M** (IBM, Apache 2.0, 384 dims, 32k context).
- **Buscador prefiere el ONNX cuantizado `model_quint8_avx2.onnx`** y cae a `model.onnx` si hace falta.
- **Los vectores son de 384 dimensiones** (1536 bytes en BLOB).
- **El score fuzzy va de 0 a ~1600.** El score semántico se mapea a 0-600. Para ser competitivo, un item semántico necesita similarity > 0.3.
- **El vector store es SQLite** en `~/.local/share/buscador/vectors.db`.
- **Buscador indexa automáticamente las apps del sistema** (.desktop files).
- **Para indexar contenido ajeno** (emails, fotos, etc.), usar `POST /index` con embeddings pre-calculados.
- **El motor de embeddings puede no estar disponible** (check `GET /stats` → `engine_available`). Si no está, solo funciona fuzzy search.
- **No hay rate limiting** (solo localhost). No abusar.
- **No hay autenticación.** Si se expone a red, añadir proxy con auth.

## TODO / Futuro

- [ ] Auto-reindex cuando Nexus notifique nuevo contenido
- [ ] WebSocket para resultados live
- [ ] Búsqueda por tipo (solo emails, solo archivos)
- [ ] Filtros de metadata (fecha, remitente, etc.)
- [ ] Soporte para embeddings de otros modelos (Gemma via Argos)
- [ ] Endpoint `DELETE /index/:id` para limpiar items
