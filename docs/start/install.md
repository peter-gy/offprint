# Install Offprint

Choose the interface that owns your application boundary. Every interface calls
the same native Rust service and uses the same canonical records.

Offprint is currently an alpha project before its first tagged release. Build
the CLI and language bindings from this checkout. The release workflow is
prepared to publish native archives and versioned registry packages from a
signed tag.

## Choose an interface

| Interface | Distribution | Runtime baseline |
| --- | --- | --- |
| CLI | Source build, then native archive | Supported host and Chromium-based browser |
| Rust | Path dependency, then crates.io | Rust 1.97 |
| Node.js | Source build, then npm package with native addons | Node.js 22 or newer |
| Python | Source build, then PyPI stable-ABI wheel | Python 3.10 through 3.14 |

Native release targets are Linux x86-64 with glibc, macOS arm64, macOS x86-64,
and Windows x86-64.

Source builds require [Git](https://git-scm.com/), the repository's pinned
[Rust](https://www.rust-lang.org/tools/install) 1.97 toolchain, and native C
build tools for the host. Collector and Node.js builds require
[Bun](https://bun.sh/) 1.3.14. Python builds require
[uv](https://docs.astral.sh/uv/) and [Maturin](https://www.maturin.rs/), which
the Python environment installs from its lockfile.

## Linux runtime libraries

Linux capture hosts need the shared libraries required by Chromium. The Ubuntu
24.04 release smoke installs the required runtime packages with:

```console
sudo apt-get update
sudo apt-get install -y \
  libasound2t64 \
  libatk-bridge2.0-0 \
  libgbm1 \
  libgtk-3-0 \
  libnss3 \
  libxkbcommon0
```

Package names differ across Linux distributions. Verify a Chromium-based
browser can start in the target image before capture.

## Build the CLI from source

```console
git clone https://github.com/peter-gy/offprint.git
cd offprint
cargo build --release --locked -p offprint-cli
./target/release/offprint --version
```

On macOS and Linux, install the built executable in a user-local command
directory before following guides that use the bare `offprint` command:

```console
mkdir -p "$HOME/.local/bin"
install -m 0755 ./target/release/offprint "$HOME/.local/bin/offprint"
export PATH="$HOME/.local/bin:$PATH"
offprint --version
```

Add that directory to the shell's persistent `PATH` when the command should be
available in later sessions. On Windows, place `offprint.exe` in a directory
already listed in `PATH`.

Release archives contain the executable, license, README, shell completions,
checksums, and build metadata. Verify the published checksum before installing
the executable on `PATH`.

## Add the Rust crate

From this checkout:

```toml
[dependencies]
offprint = { path = "/path/to/offprint/crates/offprint" }
```

Continue with the [Rust integration](../integrations/rust.md).

## Build Node.js from source

```console
cd bindings/node
bun install --frozen-lockfile
bun run build
bun run examples/capture.ts
```

The release package contains the native addons for every supported target.
Continue with the [Node.js integration](../integrations/node.md).

## Build Python from source

```console
cd bindings/python
uv sync --frozen
uv run maturin develop
uv run python examples/capture.py
```

Wheels use Python's stable application binary interface from Python 3.10.
Continue with the [Python integration](../integrations/python.md).

## Browser requirement

Offprint discovers Chrome, Chromium, or Microsoft Edge based on Chromium 120 or
newer. When automatic installation is enabled and no compatible browser is
present, first use downloads and verifies the pinned Chrome for Testing build.

Source-build CLI users can run `./target/release/offprint doctor` before an
expensive workflow. Binding users can call `offprint.browsers.doctor()`. Read
[browsers and ownership](../concepts/browsers.md)
when provisioning, cache, or remote ownership changes your deployment.
