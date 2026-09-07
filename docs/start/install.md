# Install Offprint

Build the command-line interface (CLI) to capture pages from a shell, or choose
a language integration for your application. Each interface uses the same
capture engine and verification defaults.

Offprint is an alpha project. Build the CLI and language bindings from source.

## Choose an interface

| Interface                           | Distribution    | Runtime baseline                          |
| ----------------------------------- | --------------- | ----------------------------------------- |
| [CLI](#build-the-cli-from-source)   | Source build    | Supported host and Chromium-based browser |
| [Rust](#add-the-rust-crate)         | Path dependency | Rust 1.97                                 |
| [Node.js](#build-the-node-sdk)      | Source build    | Node.js 22 or newer                       |
| [Python](#build-python-from-source) | Source build    | Python 3.10 through 3.14                  |

Supported native targets are Linux x86-64 with glibc, macOS arm64, macOS x86-64,
and Windows x86-64.

Source builds require [Git](https://git-scm.com/), the repository's pinned
[Rust](https://www.rust-lang.org/tools/install) 1.97 toolchain, and native C
build tools for the host. Collector and Node.js builds require
[Node.js](https://nodejs.org/) 24.14.1 or newer and
[pnpm](https://pnpm.io/), the workspace package manager. Activate the pinned
pnpm version with `corepack enable` after installing
[Corepack](https://nodejs.org/api/corepack.html), Node.js package-manager dispatch.
Python builds require
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
cargo build --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-cli
./offprint-rs/target/release/offprint --version
```

On macOS and Linux, install the built executable in a user-local command
directory before following guides that use the bare `offprint` command:

```console
mkdir -p "$HOME/.local/bin"
install -m 0755 ./offprint-rs/target/release/offprint "$HOME/.local/bin/offprint"
export PATH="$HOME/.local/bin:$PATH"
offprint --version
```

Add that directory to the shell's persistent `PATH` when the command should be
available in later sessions. On Windows, place `offprint.exe` in a directory
already listed in `PATH`.

Continue with [your first capture](./quickstart.md).

## Add the Rust crate

From this checkout:

```toml
[dependencies]
offprint = { path = "/path/to/offprint/offprint-rs/core" }
```

Continue with the [Rust integration](../integrations/rust.md).

## Build the Node SDK

```console
pnpm install --frozen-lockfile
pnpm --filter offprint build
cd sdk/node
node examples/capture.ts
```

Continue with the [Node.js integration](../integrations/node.md).

## Build Python from source

```console
cd sdk/python
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

Source-build CLI users can run `./offprint-rs/target/release/offprint doctor` before an
expensive workflow. Binding users can call `offprint.browsers.doctor()`. Read
[browsers and ownership](../concepts/browsers.md)
when provisioning, cache, or remote ownership changes your deployment.
