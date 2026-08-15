# Third-party licenses

Ferrum API is MIT licensed. Its direct Rust dependencies are selected from permissively licensed
ecosystem crates. Release automation must regenerate the complete transitive license inventory
with `cargo-about` and fail on licenses outside the approved MIT, Apache-2.0, BSD, ISC, Unicode,
and Zlib families. Key direct dependencies include eframe/egui (MIT OR Apache-2.0), Tokio (MIT),
Reqwest (MIT OR Apache-2.0), Rustls (Apache-2.0 OR ISC OR MIT), SQLx (MIT OR Apache-2.0), Keyring
(MIT OR Apache-2.0), Serde (MIT OR Apache-2.0), and Tracing (MIT).
