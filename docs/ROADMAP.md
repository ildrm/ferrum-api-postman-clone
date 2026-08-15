# Implementation roadmap

## Phase 1 — Foundation

The current milestone implements native startup, request editing/execution/cancellation,
collections, environments, variables, history, persistence, bounded response viewing, and secure
credential abstraction. The exit gate is format, Clippy, workspace tests, release build, and a
desktop launch smoke test on at least one host OS.

## Phase 2 — Developer features

Authentication strategies, cookie jar, certificate/proxy UI, sandboxed scripting, assertions,
collection runner, examples, code generation, and developer console.

## Phase 3 — Protocols

First-class GraphQL, WebSocket, gRPC, and MQTT adapters, each using the common execution contract.

## Phase 4 — API lifecycle

OpenAPI 3.0/3.1 editing and validation, generated documentation, mock server, monitors,
performance tests, governance rules, defensive scanners, and API catalog.

## Phase 5 — Automation

Headless CLI, deterministic reports and exit codes, scheduled local agent, and CI examples.

## Phase 6 — Advanced platform

WASM component plugins, Git-backed open project format, workflows, revision history, and merge.

## Phase 7 — Optional collaboration

Self-hosted Axum/PostgreSQL sync server, Argon2id/WebAuthn authentication, organizations, RBAC,
comments, optimistic concurrency, presence, and conflict-safe synchronization.

## Phase 8 — Optional AI and MCP

Provider-neutral local/remote AI with mandatory redaction, MCP client/inspector/server, and
explicit approval before selected collection operations become MCP tools.
