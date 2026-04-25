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
   (tree view, click-to-expand / collapse), with a self-contained HTML export that
   mirrors the GUI.

The canonical end-to-end test input lives in `examples/poim/` and is a real-world
ETSI ITS POIM specification split across four modules
(`POIM-PDU-Description.asn`, `POIM-CommonContainers.asn`,
`POIM-ParkingAvailability.asn`, `ETSI-ITS-CDD.asn`). Additional fixtures live in
`examples/ts103301/` (ETSI TS 103 301 ITS facilities, pulled in as a git
submodule) and `examples/lte_nr_rrc_rel18.6_specs/` (3GPP RRC Rel-18.6). Anything
the tool ships with must round-trip these inputs without manual edits.

### 1.1 Goals

- Accept **generic, standards-conformant ASN.1** — not a POIM-specific dialect.
- Produce **compilable, self-contained Java** source that preserves ASN.1 semantics
  (types, constraints, optionality, extension markers, information object sets).
- Make the parsed hierarchy **explorable** via a native GUI (primary) and a
  self-contained HTML export (secondary).
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
fixtures and top-level config live at the workspace root.

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
├── .gitmodules                   # pulls in examples/ts103301
├── .github/
│   └── workflows/
│       ├── ci.yml                # fmt + clippy + test on push/PR
│       └── release.yml           # tag-triggered binary release
│
├── crates/
│   ├── asn1-parser/              # lexer + grammar → concrete syntax tree
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs            # `parse_source` entry point
│   │   │   ├── lexer.rs          # hand-written tokenizer
│   │   │   ├── grammar.rs        # recursive-descent parser over tokens
│   │   │   ├── cst.rs            # concrete syntax tree + `Spanned<T>`
│   │   │   └── diagnostics.rs    # `SourceMap`, `Span`, `ParseError`
│   │   └── tests/
│   │       └── poim_smoke.rs     # parses examples/poim/ end-to-end
│   │
│   ├── asn1-ir/                  # typed intermediate representation
│   │   ├── Cargo.toml            # lowers CST → semantic IR, resolves
│   │   ├── src/                  # cross-module references, exposes
│   │   │   └── lib.rs            # `IrProgram::diagnostics()` for the UI
│   │   └── tests/
│   │       └── poim_lower.rs     # asserts POIM lowers without errors
│   │
│   ├── asn1-codegen-java/        # IR → Java source files
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   └── lib.rs            # emits .java sources given `JavaOptions`
│   │   └── tests/
│   │       └── poim_codegen.rs   # golden-style check of POIM output
│   │
│   ├── asn1-viz/                 # interactive tree viewer (egui) + HTML export
│   │   ├── Cargo.toml            # library crate — no binary
│   │   ├── src/
│   │   │   ├── lib.rs            # re-exports `launch`, `launch_with_options`,
│   │   │   │                     # `LaunchOptions`, `Icon`, and `export_html`
│   │   │   ├── app.rs            # eframe::App — menus, picker, drill-down
│   │   │   ├── tree.rs           # click-to-expand renderer shared by the UI
│   │   │   ├── loader.rs         # walks inputs, parses, lowers to IR
│   │   │   ├── theme.rs          # Light / Dark / Grey palettes
│   │   │   ├── html.rs           # standalone HTML exporter
│   │   │   └── test_fixtures.rs  # shared helpers for unit tests
│   │
│   ├── asn1-tool/                # standalone desktop binary (`asn1-tool`)
│   │   ├── Cargo.toml
│   │   ├── build.rs              # embeds Windows icon/manifest/version info
│   │   ├── assets/
│   │   │   ├── icon.png          # runtime window icon (cross-platform)
│   │   │   ├── icon.ico          # Windows resource icon
│   │   │   └── app.manifest      # DPI-aware, longPathAware, UTF-8 ACP
│   │   └── src/
│   │       ├── main.rs           # windows_subsystem gate + launch hand-off
│   │       ├── paths.rs          # portable-mode vs OS data-dir resolver
│   │       └── logging.rs        # tracing subscriber + panic crash log
│   │
│   └── asn1-cli/                 # binary crate — the user-facing tool
│       ├── Cargo.toml            # produces the `asn1-decoder` binary
│       └── src/
│           └── main.rs           # `check` / `generate` / `visualize`
│
└── examples/
    ├── poim/                     # canonical end-to-end fixture (do not edit)
    │   ├── ETSI-ITS-CDD.asn
    │   ├── POIM-CommonContainers.asn
    │   ├── POIM-ParkingAvailability.asn
    │   └── POIM-PDU-Description.asn
    ├── ts103301/                 # git submodule — ETSI TS 103 301 facilities
    └── lte_nr_rrc_rel18.6_specs/ # 3GPP RRC Rel-18.6 ASN.1 sources
```

Each crate keeps its tests co-located under `crates/<name>/tests/`; there is
intentionally no workspace-level `tests/` directory today.

### 2.1 Dependency direction

```
asn1-tool ──► asn1-viz ──► asn1-ir ──► asn1-parser
asn1-cli  ──► asn1-viz ──► asn1-ir ──► asn1-parser
          └─► asn1-codegen-{java,cpp} ──► asn1-ir ──► asn1-parser
```

A crate **must not** depend on anything to its left in the diagram. `asn1-parser`
has no intra-workspace dependencies. `asn1-ir` is the single source of truth
consumed by both the code generators and the visualizer. `asn1-viz` additionally
depends on `asn1-parser` because it parses inputs itself (the GUI's File menu
has to reparse from disk, not take a pre-built IR). `asn1-tool` is a thin
desktop wrapper: it owns window-icon loading, tracing, the panic hook, and
portable-mode path resolution; all actual UI code stays in `asn1-viz`.

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
(file, line, column, caret) rather than silently producing wrong Java. References
that parse but fail to resolve surface as *warnings* via
`IrProgram::diagnostics()` and are shown in the visualizer's diagnostics panel
and in the HTML export's warnings banner — they do not abort the build.

---

## 4. Java code generation requirements

- **One Java package per ASN.1 module.** Package name derived from the module name:
  `POIM-PDU-Description` → `poim.pdu.description` (lowercase, hyphens to dots, no
  double dots). The package prefix is configurable via CLI flag
  (`--java-package-prefix com.example`, default `generated.asn1`).
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
  treated as errors.
- No runtime dependency on Jackson, Lombok, Protobuf, or any other library. The
  generated code is plain Java stdlib only.

---

## 5. Visualization requirements

- **Framework**: `eframe` / `egui` for a cross-platform native window (Windows,
  Linux, macOS). No Electron, no bundled browser.
- **Layout**: a top menu bar, a left picker panel, and a central drill-down
  panel.
  - **Picker** (left): every module in the loaded program is a collapsible
    group listing its named types. A text filter matches module or type names.
    Each module header carries a right-aligned `×` button that *removes* the
    module from the view — the module stays on disk but is dropped before
    lowering, so unresolved references it owned surface as new warnings.
  - **Drill-down** (center): the selected root type is rendered as a
    click-to-expand tree. Composite types (`SEQUENCE` / `SET` / `CHOICE` /
    `ENUMERATED` / `SEQUENCE OF` / `SET OF`) expand in place; named-type
    references resolve against the IR so the user can keep drilling through
    aliases until primitive leaves are reached. Cycles are detected and shown
    as `↺ recursive: Module.Name` rather than looped forever.
- **Sources panel**: the header bar shows module count, a parse-errors chip,
  a warnings chip, and the current drill-down root. Clicking a chip opens
  its window (parse errors or unresolved references) with the full list.
- **File menu** (all actions that require a loaded program are disabled when
  none is loaded):
  - **Open file… / Open directory…** — replace the current source set via a
    native file dialog (`rfd`).
  - **Add file… / Add directory…** — import an additional source alongside
    the current set and re-parse; references that were previously unresolved
    may now resolve. Honors any module-level exclusions already in effect.
  - **Export HTML…** — save the current tree as a standalone HTML file via a
    native save-file dialog.
  - **Close** — clear the loaded program.
- **View menu**: theme selector with **Light**, **Dark**, and **Grey** (a
  hand-tuned mid-tone neutral). The initial theme follows the OS preference
  via the `dark-light` crate, falling back to Dark.
- **HTML export**: self-contained (no external assets); embeds a `prefers-
  color-scheme` script so the exported file tracks the viewer's OS theme.
  Includes a collapsible warnings banner that mirrors the GUI's diagnostics
  panel. Unresolved references render as flat yellow leaves (no expand
  triangle), matching the GUI.
- **Headless mode**: `asn1-decoder visualize --export tree.html` produces the
  HTML export without opening a window, so the visualizer is usable in CI.

---

## 6. CLI surface

Binary name: `asn1-decoder`. Built with `clap` (derive API).

```
asn1-decoder check      <inputs...>
asn1-decoder generate   <inputs...> --out <dir> [--java-package-prefix <p>]
asn1-decoder visualize  <inputs...> [--export <file>]
asn1-decoder --version
asn1-decoder --help
```

- `<inputs...>` accepts individual `.asn` files and directories (recursed,
  `*.asn` only; directories named `reference` are skipped). Multiple inputs are
  treated as a single compilation unit so cross-module `IMPORTS` resolve.
- `visualize` without `--export` launches the native GUI; with `--export <file>`
  writes the standalone HTML and exits.
- `generate` currently emits `.java` files rooted at `--out`; the default
  package prefix is `generated.asn1`.
- Unresolved references are reported as warnings on stderr and do not change
  the exit code.

---

## 7. Build, test, run

All commands are run from the workspace root.

| Task                        | Command                                             |
| --------------------------- | --------------------------------------------------- |
| Format                      | `cargo fmt --all`                                   |
| Lint (must pass clean)      | `cargo clippy --workspace --all-targets -- -D warnings` |
| Unit + integration tests    | `cargo test --workspace`                            |
| Run CLI (debug)             | `cargo run -p asn1-cli -- generate examples/poim --out target/java` |
| Launch visualizer           | `cargo run -p asn1-cli -- visualize examples/poim`  |
| Export HTML (headless)      | `cargo run -p asn1-cli -- visualize examples/poim --export tree.html` |
| Release build               | `cargo build --release --workspace`                 |

The `ts103301` fixture is a git submodule — clone with
`git clone --recurse-submodules` or run `git submodule update --init` after a
plain clone.

---

## 8. Coding standards

- **Toolchain**: pinned in `rust-toolchain.toml` (stable, currently 1.95+).
  No nightly features.
- **Formatting**: `cargo fmt` is authoritative. CI fails on diff.
- **Lints**: `clippy` at `-D warnings` workspace-wide. Allow-list specific lints
  only in the crate that needs the exception, with a comment.
- **Errors**: library crates return `thiserror`-derived enums; the CLI uses
  `anyhow` only at the top edge. Parser errors carry a
  `Span { file, start, end }` so diagnostics can show source context.
- **Public API**: every `pub` item in a library crate carries a rustdoc comment.
- **No `unsafe`** anywhere in the workspace. If it becomes necessary, it lives
  behind a reviewed module with a `// SAFETY:` comment per block.
- **Dependencies**: prefer well-maintained crates already in the tree. New
  dependencies require justification in the PR description. The visualizer
  stack is intentionally narrow: `eframe`/`egui`, `rfd` (native dialogs),
  `dark-light` (OS theme), `walkdir` (directory recursion).

---

## 9. Testing strategy

- **`asn1-parser`**: integration tests under `crates/asn1-parser/tests/` exercise
  the parser end-to-end against the POIM fixture. Negative-path coverage is
  added as diagnostics regressions are discovered.
- **`asn1-ir`**: integration tests under `crates/asn1-ir/tests/` lower the POIM
  fixture and assert the resolver produces no unexpected diagnostics.
- **`asn1-codegen-java`**: integration tests under
  `crates/asn1-codegen-java/tests/` generate Java for the POIM fixture and
  assert on file layout, package naming, and per-type output shape.
- **`asn1-viz`**: unit tests inside `src/` (tree model, HTML export structure)
  with shared helpers in `test_fixtures.rs`. The egui surface itself is not
  unit-tested; the HTML export is.

CI (`.github/workflows/ci.yml`) runs fmt + clippy + test on Linux, macOS, and
Windows.

---

## 10. Versioning & releases

- Semantic versioning at the workspace level; all crates share one version
  (`0.1.0` today).
- `release.yml` triggers on `v*` tags and attaches prebuilt binaries:
  - `asn1-decoder` (CLI) on `x86_64-pc-windows-msvc`,
    `x86_64-unknown-linux-gnu`, and `aarch64-apple-darwin`.
  - `asn1-tool` (desktop GUI) on `x86_64-pc-windows-msvc` and
    `x86_64-unknown-linux-gnu`, in both system and portable flavors (see
    `PORTABLE.md`). Linux builds target ubuntu-22.04 → glibc 2.35 baseline.
- Breaking changes to generated Java or C++ (renames, package layout) require
  a major bump and a migration note in the release body.

---

## 11. Roadmap

1. **M1 — Parser.** Full CST for the POIM fixture; span-accurate diagnostics. ✅
2. **M2 — IR.** Resolver, constraint model, information object expansion. ✅
3. **M3 — Java codegen.** POJOs + enums, doc comments, compile-clean output. ✅
4. **M4 — CLI.** `check` / `generate` / `visualize` commands wired to M1–M3. ✅
5. **M5 — Visualizer.** egui tree view + File/View menus + HTML export. ✅
6. **M6 — Polish.** Cross-platform release binaries, broader ASN.1 corpus beyond
   POIM (ts103301, 3GPP RRC already in-tree as fixtures).
