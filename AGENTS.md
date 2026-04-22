# AGENTS.md

This file is the authoritative specification for agents and contributors working on
`asn1-decoder`. It defines **what** the project must do and **how** the source tree is
organized. Treat it as the contract: if a change conflicts with this document, update
this document first (with justification in the PR description), then implement.

---

## 1. Project overview

`asn1-decoder` is a **Rust command-line tool and library** that

1. parses arbitrary ASN.1 specification files,
2. generates idiomatic **Java classes** that faithfully model those specifications so
   they can be used as a library by downstream Java projects, and
3. renders an **interactive, hierarchical visualization** of the parsed specification
   (tree view, click-to-expand / collapse).

The canonical end-to-end test input lives in `examples/poim/` and is a real-world
ETSI ITS POIM specification split across four modules
(`POIM-PDU-Description.asn`, `POIM-CommonContainers.asn`,
`POIM-ParkingAvailability.asn`, `ETSI-ITS-CDD.asn`). Anything the tool ships with
must round-trip this example without manual edits to the input.

### 1.1 Goals

- Accept **generic, standards-conformant ASN.1** — not a POIM-specific dialect.
- Produce **compilable, self-contained Java** source that preserves ASN.1 semantics
  (types, constraints, optionality, extension markers, information object sets).
- Make the parsed hierarchy **explorable** via a GUI (primary) and a machine-readable
  export (secondary).
- Adhere to a **professional, idiomatic Rust workspace layout** that a new
  contributor can navigate without guidance.

### 1.2 Non-goals

- BER/DER/PER/UPER *encoder or decoder* runtime. This project generates type
  definitions only; wire-format codecs are out of scope for v1.
- Code generation targets other than Java (no Kotlin, C#, Python, etc. in v1).
- A full IDE — the visualizer is a read-only explorer.
- Shipping the generated Java as a published Maven artifact. Downstream projects
  consume the generated source tree directly.

---

## 2. Repository layout

The project is a **Cargo workspace**. Every Rust concern lives under `crates/`;
generator templates, fixtures, and docs live at the workspace root.

```
asn1-decoder/
├── AGENTS.md                     # this file — project contract
├── README.md                     # user-facing quickstart
├── LICENSE
├── Cargo.toml                    # workspace manifest
├── Cargo.lock
├── rust-toolchain.toml           # pinned stable toolchain
├── rustfmt.toml                  # formatting rules
├── clippy.toml                   # lint configuration
├── .editorconfig
├── .gitignore
├── .github/
│   └── workflows/
│       ├── ci.yml                # fmt + clippy + test on push/PR
│       └── release.yml           # tag-triggered binary release
│
├── crates/
│   ├── asn1-parser/              # lexer + grammar → concrete syntax tree
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── lexer.rs
│   │   │   ├── grammar.rs        # nom / chumsky / pest combinators
│   │   │   ├── cst.rs            # concrete syntax tree
│   │   │   └── diagnostics.rs    # span-aware error reporting
│   │   └── tests/
│   │
│   ├── asn1-ir/                  # typed intermediate representation
│   │   ├── Cargo.toml            # resolves imports, validates references,
│   │   ├── src/                  # lowers CST → semantic IR, expands
│   │   │   ├── lib.rs            # information object classes & sets
│   │   │   ├── module.rs
│   │   │   ├── types.rs
│   │   │   ├── constraints.rs
│   │   │   ├── resolver.rs       # cross-module name resolution
│   │   │   └── lowering.rs
│   │   └── tests/
│   │
│   ├── asn1-codegen-java/        # IR → Java source files
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── emitter.rs        # writes .java files to an output dir
│   │   │   ├── naming.rs         # ASN.1 → Java identifier mapping
│   │   │   ├── mapping.rs        # ASN.1 type → Java type rules
│   │   │   └── templates/        # string templates per Java construct
│   │   └── tests/
│   │
│   ├── asn1-viz/                 # interactive tree viewer (egui)
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── app.rs            # eframe::App impl
│   │   │   ├── tree.rs           # expand/collapse model
│   │   │   └── export.rs         # JSON / HTML export
│   │   └── assets/
│   │
│   └── asn1-cli/                 # binary crate — the user-facing tool
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs
│       │   └── commands/
│       │       ├── generate.rs   # `asn1-decoder generate`
│       │       ├── visualize.rs  # `asn1-decoder visualize`
│       │       └── check.rs      # `asn1-decoder check`
│       └── tests/                # CLI integration tests (assert_cmd)
│
├── examples/
│   └── poim/                     # canonical end-to-end fixture (do not edit)
│       ├── ETSI-ITS-CDD.asn
│       ├── POIM-CommonContainers.asn
│       ├── POIM-ParkingAvailability.asn
│       └── POIM-PDU-Description.asn
│
├── tests/                        # workspace-level end-to-end tests
│   └── e2e_poim.rs               # parses examples/poim/, generates Java,
│                                 # compiles it with javac, asserts success
│
├── docs/
│   ├── asn1-support-matrix.md    # which ASN.1 features are implemented
│   ├── java-mapping.md           # ASN.1 → Java mapping table
│   └── architecture.md           # data flow: CST → IR → codegen / viz
│
└── target/                       # build output (gitignored)
```

### 2.1 Dependency direction

```
asn1-cli ──► asn1-viz ──► asn1-ir ──► asn1-parser
         └─► asn1-codegen-java ──► asn1-ir ──► asn1-parser
```

A crate **must not** depend on anything to its left in the diagram. `asn1-parser`
has no intra-workspace dependencies. `asn1-ir` is the single source of truth consumed
by both the code generator and the visualizer.

---

## 3. ASN.1 feature requirements

The parser and IR must accept the full surface area exercised by
`examples/poim/`. The following must be supported in v1; anything outside this
list that appears in the POIM fixture is also implicitly required.

**Module-level**

- Module header with object identifier (`{itu-t (0) … minor-version-1 (1)}`)
- `DEFINITIONS AUTOMATIC TAGS ::= BEGIN … END`
- `IMPORTS … FROM … WITH SUCCESSORS ;`
- Line comments (`--`) and ASN.1 doc comments (`/** … */`) — doc comments must be
  preserved and forwarded to generated Java as Javadoc.

**Type constructors**

- `SEQUENCE { … }`, `SEQUENCE (SIZE (…)) OF …`
- `SET { … }`, `SET OF …`
- `CHOICE { … }`
- `ENUMERATED { … }`
- `INTEGER` with value ranges and named numbers
- `BOOLEAN`, `NULL`, `OBJECT IDENTIFIER`, `REAL`
- `BIT STRING`, `OCTET STRING`
- String types: `UTF8String`, `IA5String`, `PrintableString`, `NumericString`,
  `VisibleString`, `BMPString`
- `OPTIONAL`, `DEFAULT`
- Extension marker `...` and extension additions `[[ … ]]`
- Constraint composition: `(SIZE (…))`, `(1..128,...)`, `WITH COMPONENTS { … }`

**Information objects**

- Information object classes: `FOO ::= CLASS { &id …, &Type } WITH SYNTAX { … }`
- Information object sets: `MySet FOO ::= { { … IDENTIFIED BY … }, ... }`
- Field references: `FOO.&id`, `FOO.&Content({Set}{@field})`
- Value assignments: `name Type ::= value`

Behavior on unsupported syntax: the parser must emit a **span-accurate diagnostic**
(file, line, column, caret) rather than silently producing wrong Java.

---

## 4. Java code generation requirements

- **One Java package per ASN.1 module.** Package name derived from the module name:
  `POIM-PDU-Description` → `poim.pdu.description` (lowercase, hyphens to dots, no
  double dots). The package prefix is configurable via CLI flag
  (`--java-package-prefix com.example`).
- **One top-level Java file per named ASN.1 type.**
- **Idiomatic Java 17+**: use `record` for immutable SEQUENCE-of-primitives where
  no extension marker is present; use sealed interfaces for `CHOICE`; use
  `enum` for `ENUMERATED`.
- **Optionality**: `OPTIONAL` and `DEFAULT` fields are represented as
  `java.util.Optional<T>` on the accessor, never as nullable reference types.
- **Constraints**: size / value constraints emit runtime validators invoked from
  the constructor (`throw new IllegalArgumentException` on violation). Extension
  markers relax the bound rather than removing it.
- **Imports across modules** translate to Java `import` statements referencing the
  mapped package of the source module.
- **Doc comments** from ASN.1 are copied to Javadoc on the corresponding class,
  field, or enum constant, preserving `@field` / `@category` / `@revision`
  annotations as Javadoc tags.
- The generated tree **must compile with `javac --release 17`** without warnings
  treated as errors. The end-to-end test enforces this.
- No runtime dependency on Jackson, Lombok, Protobuf, or any other library. The
  generated code is plain Java stdlib only.

Full mapping table lives in `docs/java-mapping.md` and must be kept in sync.

---

## 5. Visualization requirements

- **Framework**: `eframe` / `egui` for a cross-platform native window (Windows,
  Linux, macOS). No Electron, no bundled browser.
- **View**: a single-pane tree rooted at the module list. Each node shows the
  type name on the left and the ASN.1 kind (`SEQUENCE`, `CHOICE`, …) as a subtle
  badge on the right. Clicking a node expands/collapses its children.
- **Selection**: selecting a leaf opens a detail panel showing the original
  doc comment, resolved type, constraints, and the source span
  (`file:line:col`).
- **Cross-references**: type references are clickable and navigate the tree to
  the referenced definition.
- **Export**: `File → Export…` writes either a standalone HTML file
  (self-contained, no external assets) or a JSON dump of the IR. Both formats
  must round-trip through `asn1-decoder visualize --open <file>`.
- **Headless mode**: `asn1-decoder visualize --export tree.html` produces the
  export without opening a window, so the visualizer is usable in CI.

---

## 6. CLI surface

Binary name: `asn1-decoder`. Built with `clap` (derive API).

```
asn1-decoder generate   <inputs...> --out <dir> [--java-package-prefix <p>]
asn1-decoder visualize  <inputs...> [--export <file>] [--format html|json]
asn1-decoder check      <inputs...>        # parse + resolve, no output
asn1-decoder --version
asn1-decoder --help
```

- `<inputs...>` accepts individual `.asn` files and directories (recursed,
  `*.asn` only). Multiple inputs are treated as a single compilation unit so
  cross-module `IMPORTS` resolve.
- Exit codes: `0` success, `1` user error (bad args, missing file), `2` parse
  or semantic error (diagnostics printed to stderr).

---

## 7. Build, test, run

All commands are run from the workspace root.

| Task                        | Command                                             |
| --------------------------- | --------------------------------------------------- |
| Format                      | `cargo fmt --all`                                   |
| Lint (must pass clean)      | `cargo clippy --workspace --all-targets -- -D warnings` |
| Unit + integration tests    | `cargo test --workspace`                            |
| End-to-end (POIM → Java)    | `cargo test -p asn1-decoder --test e2e_poim`        |
| Run CLI (debug)             | `cargo run -p asn1-cli -- generate examples/poim --out target/java` |
| Launch visualizer           | `cargo run -p asn1-cli -- visualize examples/poim`  |
| Release build               | `cargo build --release --workspace`                 |

The e2e test requires `javac` on `PATH`; CI installs Temurin 17.

---

## 8. Coding standards

- **Toolchain**: pinned in `rust-toolchain.toml` (stable, currently 1.85+).
  No nightly features without an RFC in `docs/`.
- **Formatting**: `cargo fmt` is authoritative. CI fails on diff.
- **Lints**: `clippy` at `-D warnings` workspace-wide. Allow-list specific lints
  only in the crate that needs the exception, with a comment.
- **Errors**: library crates return `thiserror`-derived enums; the CLI uses
  `anyhow` only at the top edge. Parser errors must carry a `Span { file, start, end }`
  so diagnostics can show source context.
- **Public API**: every `pub` item in a library crate carries a rustdoc comment.
- **No `unsafe`** anywhere in the workspace. If it becomes necessary, it lives
  behind a reviewed module with a `// SAFETY:` comment per block.
- **Dependencies**: prefer well-maintained crates already in the tree. New
  dependencies require justification in the PR description.

---

## 9. Testing strategy

- **`asn1-parser`**: snapshot tests (`insta`) against a corpus of small `.asn`
  snippets covering each grammar production. Negative tests assert precise
  diagnostic spans.
- **`asn1-ir`**: unit tests for resolver edge cases (forward references,
  cyclic imports, missing symbols, information object set expansion).
- **`asn1-codegen-java`**: golden-file tests — the generated Java is checked
  into `crates/asn1-codegen-java/tests/golden/` and diffed on each run.
- **`asn1-viz`**: unit tests for the tree model (expand/collapse, selection).
  The egui surface itself is not unit-tested; the JSON export is.
- **Workspace e2e** (`tests/e2e_poim.rs`): parses `examples/poim/`, generates
  Java into a temp dir, invokes `javac --release 17 -Xlint:all -Werror`, asserts
  exit 0 and that every expected top-level type produced a `.java` file.

CI runs the full matrix on Linux, macOS, and Windows.

---

## 10. Versioning & releases

- Semantic versioning at the workspace level; all crates share one version.
- `release.yml` triggers on `v*` tags and attaches prebuilt binaries for
  `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, and
  `aarch64-apple-darwin`.
- Breaking changes to generated Java (renames, package layout) require a major
  bump and a migration note in `docs/`.

---

## 11. Roadmap

1. **M1 — Parser.** Full CST for the POIM fixture; round-trip printer.
2. **M2 — IR.** Resolver, constraint model, information object expansion.
3. **M3 — Java codegen.** POJOs + enums, doc comments, compile-clean output.
4. **M4 — CLI.** `generate` / `check` commands wired to M1–M3.
5. **M5 — Visualizer.** egui tree view, detail panel, HTML/JSON export.
6. **M6 — Polish.** Cross-platform release binaries, docs site, broader
   ASN.1 corpus beyond POIM.

Each milestone ends with green CI and an updated
`docs/asn1-support-matrix.md`.
