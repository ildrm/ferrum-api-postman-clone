# Contributing

Install stable Rust, fork the repository, and create a focused branch. Before submitting changes,
run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`. Public interfaces require rustdoc. New protocol or storage adapters must
depend on domain/application contracts rather than UI types. Database changes require a new,
forward-only numbered file in `crates/storage/migrations` and a migration test. Never add real
credentials, captured private traffic, or unredacted fixtures.
