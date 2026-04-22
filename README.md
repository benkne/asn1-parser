# asn1-decoder

A Rust CLI and library that parses ASN.1 specifications, generates idiomatic Java
classes from them, and provides an interactive hierarchical viewer of the parsed
specification.

See [`AGENTS.md`](AGENTS.md) for the authoritative project contract.

## Quickstart

```bash
# Parse and semantically check a specification
cargo run -p asn1-cli -- check examples/poim

# Generate Java classes
cargo run -p asn1-cli -- generate examples/poim --out target/java \
    --java-package-prefix com.example

# Launch the interactive tree viewer
cargo run -p asn1-cli -- visualize examples/poim

# Export a standalone HTML tree (no window)
cargo run -p asn1-cli -- visualize examples/poim --export tree.html
```

## Workspace layout

```
crates/
  asn1-parser/         ASN.1 lexer + grammar → concrete syntax tree
  asn1-ir/             Typed intermediate representation + resolver
  asn1-codegen-java/   IR → Java source files
  asn1-viz/            egui tree viewer + HTML/JSON export
  asn1-cli/            User-facing binary
```

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Pinned via `rust-toolchain.toml`; CI enforces fmt / clippy / test on Linux,
macOS, and Windows.

## License

MIT — see [`LICENSE`](LICENSE).
