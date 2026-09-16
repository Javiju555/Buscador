# Security policy

## Reporting a vulnerability

Please report security issues privately through GitHub's security advisory or private vulnerability reporting features. Do not disclose an exploitable issue in a public issue or pull request before it has been assessed.

Include the affected version, platform, reproduction steps and any relevant logs. Avoid attaching secrets or personal data.

## Local API boundary

Buscador's HTTP API binds to `127.0.0.1` and has no authentication. It is intended for local integrations only. Do not expose it through a reverse proxy, firewall rule or LAN bind without adding authentication, authorization and request limits.

Dependency updates are monitored through Dependabot and the CI workflow runs on Ubuntu and Windows.
