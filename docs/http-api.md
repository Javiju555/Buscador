# Local HTTP API

Buscador exposes a small JSON API for local tools and scripts.

Default address: `http://127.0.0.1:8755`

The port can be changed with `BUSCADOR_HTTP_PORT`. The API has no authentication and only binds to loopback. Do not proxy or rebind it to a public or LAN address without adding authentication and request controls.

## Health

```bash
curl http://127.0.0.1:8755/health
```

Response:

```json
{
  "status": "ok",
  "version": "0.1.6"
}
```

The version is the application package version.

## Stats

```bash
curl http://127.0.0.1:8755/stats
```

Example response:

```json
{
  "total_items": 87,
  "apps": 87,
  "files": 0,
  "emails": 0,
  "engine_available": true,
  "model_file": "model_quint8_avx2.onnx",
  "configured_folders": []
}
```

`engine_available` is `false` when the embedding model is not installed or could not be loaded. Fuzzy search and path navigation still work in that case.

## Search

```http
GET /search?q=terminal&limit=10&mode=hybrid
```

Parameters:

| Parameter | Required | Description |
| --- | --- | --- |
| `q` | Yes | Query text. Empty queries return `400`. |
| `limit` | No | Maximum results. Defaults to `10` and is capped at `50`. |
| `mode` | No | `hybrid`, `fuzzy` or `semantic`. Defaults to `hybrid`. |

Modes:

- `fuzzy` searches applications, commands and indexed file names.
- `semantic` searches stored vectors and returns cosine similarity. It returns `503` when the model is unavailable.
- `hybrid` combines fuzzy results with semantic results when the model is available.

The normal launcher query syntax is also accepted by the fuzzy side of the endpoint, including absolute paths, `>commands`, `/file-name`, calculator expressions and `w ` web searches.

Response shape:

```json
{
  "results": [
    {
      "kind": "app",
      "title": "Terminal",
      "subtitle": "",
      "path": "/usr/share/applications/org.gnome.Terminal.desktop",
      "score": 420,
      "similarity": 0.7
    }
  ],
  "total": 1,
  "mode": "hybrid"
}
```

`similarity` is `null` for fuzzy-only results and is a value between `0` and `1` for vector results.

## Reindex

```bash
curl -X POST http://127.0.0.1:8755/reindex
```

This rebuilds application vectors and the configured file index. It requires the embedding model and returns `503` when the engine is unavailable.

## Add indexed items

```bash
curl -X POST http://127.0.0.1:8755/index \
  -H 'Content-Type: application/json' \
  -d '{
    "items": [{
      "id": "note:123",
      "kind": "note",
      "title": "Release checklist",
      "subtitle": "",
      "path": "/home/user/notes/release.md",
      "text": "Verify the release artifacts and update the changelog.",
      "metadata": {"source": "notes"}
    }]
  }'
```

Each item must provide either `embedding` or `text`:

| Field | Required | Description |
| --- | --- | --- |
| `id` | Yes | Stable identifier used for upserts. |
| `kind` | Yes | Application-defined type such as `note` or `file`. |
| `title` | Yes | Display title. |
| `subtitle` | No | Secondary display text. |
| `path` | No | Primary location or value. |
| `embedding` | One of these | A 384-value vector. |
| `text` | One of these | Text embedded locally by Buscador. |
| `metadata` | No | JSON object stored with the item. |

Response:

```json
{
  "indexed": 1,
  "total": 88
}
```

Items are upserted by `id`. Invalid or unembeddable items are skipped and logged by the application.
