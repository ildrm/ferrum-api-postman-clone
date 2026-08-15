# Ferrum API

Ferrum API is a local-first, native API development workspace written in Rust. The first
milestone provides a real HTTP request builder, cancellable asynchronous execution,
streamed response storage, collections, environments, scoped variables, history, and
SQLite-backed session durability.

The name, interface, storage format, and implementation are original and are not affiliated
with Postman, Inc.

## Current milestone

- Native `egui`/`eframe` desktop interface on Windows, Linux, Intel macOS, and Apple Silicon.
- GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, and custom HTTP methods.
- Query parameters, headers, JSON/text bodies, response headers, formatted JSON, timing,
  cancellation, and bounded-memory response streaming.
- Persisted workspaces, nested collections, requests, environments, variables, and redacted
  request history.
- Scope-aware `{{variable}}` interpolation and dynamic `$uuid`, `$timestamp`, `$randomInt`,
  and `$randomEmail` values.
- Secrets kept out of SQLite through an operating-system credential-store abstraction.

See [ARCHITECTURE.md](ARCHITECTURE.md) for decisions and [docs/ROADMAP.md](docs/ROADMAP.md)
for the phased implementation plan.

## Prerequisites

- Stable Rust (MSRV 1.88 or newer)
- Platform build dependencies required by eframe:
  - Linux: `libx11-dev libxi-dev libgl1-mesa-dev libwayland-dev libxkbcommon-dev`
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio Build Tools with Desktop C++ workload
- Linux secret storage: a running Secret Service provider such as GNOME Keyring

## Build and run

```bash
cargo run -p ferrum-app
```

Ferrum stores its database and streamed response cache in the platform application-data
directory. Override it for development with `FERRUM_DATA_DIR`.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

## Project configuration

Copy `.env.example` only when you need a non-default development data directory. Ferrum does
not require a cloud account, API key, telemetry endpoint, or network service to start.

## Privacy and security

TLS verification and HTTPS are enabled by default. Authorization, cookie, API-key, token, and
secret-like headers are redacted before history persistence and logging. Sensitive environment
variables store only an opaque credential reference in SQLite; their values are held by the OS
credential manager. No request or response data leaves the machine except through requests that
the user explicitly sends.

## License

MIT. See [LICENSE](LICENSE) and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
