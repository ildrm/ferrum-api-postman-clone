# Ferrum API Architecture

## 1. Overview

Ferrum is a local-first desktop application with a dependency-inverted, layered design:

```text
Native UI -> Application services -> Domain contracts
                                  -> HTTP / SQLite / credential-store adapters
```

The domain and variable crates contain no GUI, database, or networking dependencies. Protocol,
storage, and secret implementations are adapters. The UI sends commands to application services
and receives owned results, keeping network and disk work off the render thread.

## 2. Technology choices

- **GUI: egui/eframe.** It is a mature Rust-native immediate-mode toolkit with small-state
  rendering, high-DPI support, keyboard input, custom panels, resizable layouts, and supported
  Windows/Linux/macOS backends. Iced and Slint are strong alternatives; eframe was selected for
  the fastest path to complex developer-tool panels without a web runtime.
- **Async: Tokio.** One multi-threaded runtime owns network and persistence tasks.
- **HTTP: reqwest + rustls.** Connection pooling, redirects, compression, HTTP/1.1 and HTTP/2,
  proxy support, timeouts, streaming, and cross-platform TLS without OpenSSL packaging.
- **Persistence: SQLx + SQLite.** Compile-time migrations, transactions, foreign keys, indexes,
  busy timeout, and WAL mode. Resources are normalized instead of stored as one JSON document.
- **Secrets: keyring.** Platform credential stores sit behind `SecretStore`; SQLite receives only
  references. An in-memory adapter keeps tests deterministic.
- **Errors/logging:** typed `thiserror` errors and structured `tracing` with redaction boundaries.

## 3. Cargo workspace

| Crate | Responsibility |
| --- | --- |
| `ferrum-domain` | Stable identifiers, request, collection, environment, response, history models |
| `ferrum-variables` | Scope precedence, interpolation, dynamic variables, unresolved diagnostics |
| `ferrum-http-client` | Request execution, cancellation, timing, bounded-memory streaming |
| `ferrum-storage` | SQLite lifecycle, migrations, transactions, repositories, redaction |
| `ferrum-secrets` | Cross-platform credential-store contract and adapters |
| `ferrum-app-services` | Use cases that coordinate domain and infrastructure |
| `ferrum-ui` | Design tokens, native state, panels, editors, response presentation |
| `ferrum-app` | Paths, runtime, logging, startup, desktop packaging entry point |

Future protocol, scripting, runner, spec, mock, monitor, plugin, AI, MCP, CLI, sync-server, and
agent crates attach at the application-service boundary without changing UI/domain contracts.

## 4. Domain model

A workspace owns collections and environments. Collections form an adjacency-list tree and own
requests. A request owns ordered query/header rows and a typed body. Environments own variables;
sensitive variables contain a credential key, never a plaintext current value. History is an
immutable, redacted execution record. UUIDs allow offline creation and future synchronization.

## 5. Database schema

The initial migration defines `workspaces`, `collections`, `requests`, `request_query_params`,
`request_headers`, `environments`, `variables`, `history`, and `history_headers`. Foreign keys
cascade only within owned aggregates. Ordering columns preserve editor order. URL, method,
timestamp, and parent indexes support navigation and search. Every multi-table save is atomic.

## 6. Networking architecture

`HttpEngine` accepts a fully resolved request. URL and method validation occur before I/O.
Reqwest streams response chunks. A bounded preview is retained in memory; once the threshold is
crossed, the complete body is written to a unique cache file. Cancellation is selected against
both header and body phases. The engine returns status, headers, content type, elapsed time,
byte count, preview, and optional body path. HTTP/3 is deferred until the Rust ecosystem provides
a stable, packaging-friendly reqwest/rustls route.

## 7. GUI architecture

The interface has a workspace/environment toolbar, searchable resource rail, tab-like request
editor, resizable response panel, and status bar. Central design tokens provide dark/light
palettes, spacing, radii, and semantic status colors. UI state contains editor drafts, never
database connections. Commands run on Tokio and return through a non-blocking channel; repaint
requests are sent on completion. Large result views use only the bounded preview.

## 8. Async architecture

The eframe thread only renders and mutates local view state. Tokio tasks perform HTTP and SQLite
work. Every request owns a `CancellationToken`. Task results use typed messages. Shutdown drops
new work, commits completed transactions, and lets SQLite WAL recover safely after a crash.

## 9. Security architecture

TLS verification, redirects with limits, and timeouts are secure defaults. Header values are
redacted before persistence/logging using case-insensitive sensitive-name detection. Variables
are resolved locally. Credential values come from the platform vault only at execution time and
are not included in errors. Arbitrary TLS disabling, scripts, plugins, remote AI, telemetry, and
cloud sync are absent until each receives an explicit permission and threat-modelled design.

## 10. Plugin architecture

The future plugin host uses versioned WIT interfaces and WASM/WASI components. Capabilities are
deny-by-default and granted per plugin for selected request data, network hosts, or persistent
plugin storage. Native dynamic libraries are not loaded. Extension points include protocols,
authentication, import/export, assertions, code generation, visualizers, and notifications.

## 11. Cross-platform strategy

Core crates use portable Rust. OS behavior is isolated to eframe windowing, application data
paths, and credential backends. CI targets Windows x86_64, Linux x86_64, macOS x86_64, and
macOS aarch64. Native-only dependencies never enter the domain layer.

## 12. Testing strategy

Unit tests cover interpolation, redaction, request validation, and models. SQLite tests run each
migration against a temporary database and verify aggregate round trips. HTTP integration tests
use a local Axum-style test fixture in later milestones; the engine remains internet-independent.
UI-state tests exercise commands without rendering. CI formats, lints, tests, builds, audits,
and builds documentation.

## 13. Packaging strategy

Release CI builds four target archives. Follow-up packaging jobs produce MSI, signed/notarized
universal `.app`/DMG, and Linux AppImage/DEB artifacts. Signing keys live only in CI secret stores.
Update manifests are separately signed and auto-update remains opt-in.

## 14. Roadmap

Vertical phases follow the product specification: foundation; developer features; protocols;
API lifecycle; automation; extensibility; optional collaboration; optional AI/MCP. A phase may
start only after the preceding phase builds, is tested, and is usable end to end. Detailed gates
are in `docs/ROADMAP.md`.
