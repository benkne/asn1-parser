# asn1-tool — portable desktop distribution

`asn1-tool` is the standalone desktop visualizer built from this workspace.
Release tags ship two variants per platform:

| Archive                                | Mode     | State lives in…                                                              |
| -------------------------------------- | -------- | ---------------------------------------------------------------------------- |
| `asn1-tool-<tag>-<target>.zip` / `.tar.gz`          | System   | `%LOCALAPPDATA%\asn1-tool\` (Windows), `~/.local/share/asn1-tool/` (Linux)  |
| `asn1-tool-<tag>-<target>-portable.zip` / `.tar.gz` | Portable | `./data/` next to the executable — nothing is written outside the folder    |

Both archives contain exactly the same binary; the portable archive is just
distinguished by a marker file (`portable.txt`) that the binary probes for at
startup. You can convert either way yourself:

- **Make any install portable**: drop an empty `portable.txt` next to
  `asn1-tool(.exe)`.
- **Make a portable install system-wide**: delete `portable.txt`.

## What gets written to the data directory

- `asn1-tool.log` — rolling application log (daily rotation). Set
  `ASN1_TOOL_LOG=debug` in the environment to increase verbosity.
- `crash-<timestamp>.log` — written when the process panics, includes the
  panic message and a captured backtrace.

No user-supplied `.asn` files are ever copied there; the visualizer only
reads them from their original location on disk.

## Running on Windows

- Download `asn1-tool-<tag>-x86_64-pc-windows-msvc.zip`, extract, double-click
  `asn1-tool.exe`. No installer, no admin rights, no registry keys.
- SmartScreen may warn the first time the binary is launched — the release is
  not code-signed. Click *More info → Run anyway*.
- Supported: Windows 10 / 11 on x86-64. Windows 7 / 8 / 8.1 are declared
  compatible in the manifest but are not actively tested.

## Running on Linux

- Download `asn1-tool-<tag>-x86_64-unknown-linux-gnu.tar.gz`, extract, run
  `./asn1-tool`.
- glibc 2.35+ is required (Ubuntu 22.04, Debian 12, Fedora 36, RHEL 9, and
  newer). On older distros the loader will refuse with a `GLIBC_2.xx not
  found` error — build from source instead.
- The GUI needs a desktop environment with the following shared libraries
  available (all present on every mainstream distro out of the box):
  `libgtk-3`, `libxkbcommon`, `libwayland-client`, `libxcb`, `libGL`,
  `libfontconfig`. On a headless server these are not installed — use
  `asn1-decoder visualize --export tree.html` for a no-window export.

## Uninstalling

Because there is no installer, "uninstall" just means deleting the folder.
If you want to also clean up state from a non-portable install, remove:

- Windows: `%LOCALAPPDATA%\asn1-tool\`
- Linux:   `~/.local/share/asn1-tool/`
- macOS:   `~/Library/Application Support/asn1-tool/`

## Command-line flags

```
asn1-tool [INPUT...]

INPUT   One or more `.asn` files or directories to load at startup.
        Directories are scanned recursively for `*.asn`; folders named
        `reference/` are skipped. Omit to open an empty window.

--version    Print version and exit.
--help       Print usage.
```

Environment:

- `ASN1_TOOL_LOG` — tracing filter, e.g. `ASN1_TOOL_LOG=debug` or
  `ASN1_TOOL_LOG=asn1_viz=debug,info`.
