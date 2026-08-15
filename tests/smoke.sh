#!/usr/bin/env sh
set -eu

# Repository-level quality gate used locally and by generic shell CI systems.
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p ferrum-app
