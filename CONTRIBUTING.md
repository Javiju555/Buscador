# Contributing

Thanks for helping improve Buscador.

## Development setup

Install Rust stable, Bun and the Tauri prerequisites for your platform. From the repository root:

```bash
cd frontend
bun install --frozen-lockfile
cd ../src-tauri
cargo check
cargo test vector_store -- --nocapture
```

Run the desktop application with:

```bash
cargo tauri dev --no-watch
```

## Pull requests

- Keep changes focused and explain user-visible behavior in the pull request description.
- Add or update tests when behavior changes.
- Keep public technical documentation in `docs/` and user-facing usage guidance in `README.md`.
- Do not commit build output, local models, databases, credentials or machine-specific configuration.
- Make sure the Ubuntu and Windows CI checks pass before requesting review.

Dependabot pull requests are automatically grouped and merged after the CI workflow succeeds when the repository settings allow the required workflow permissions.
