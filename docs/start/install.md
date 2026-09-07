# Install Offprint

Run the command-line interface (CLI) to capture pages from a shell, or install
a language package for your application. Each interface uses the same capture
engine and verification defaults.

## Run the CLI

Use [`uvx`](https://docs.astral.sh/uv/guides/tools/), uv's Python command runner:

```console
uvx offprint capture https://example.com --output example.html --quiet
```

Or use [`npx`](https://docs.npmjs.com/cli/commands/npx), npm's command runner,
with Node.js 22 or newer:

```console
npx offprint capture https://example.com --output example.html --quiet
```

Both commands download the Offprint package and run its CLI. The capture prints
`example.html` after reopening the saved file with networking blocked. Existing
output files are preserved unless you pass `--on-exists replace`.

## Install the CLI

Install a persistent `offprint` command with [uv](https://docs.astral.sh/uv/):

```console
uv tool install offprint
offprint --version
```

Or install it with [npm](https://docs.npmjs.com/about-npm):

```console
npm install --global offprint
offprint --version
```

Standalone native archives are available from
[GitHub Releases](https://github.com/peter-gy/offprint/releases).
Continue with [your first capture](./quickstart.md).

## Choose a language package

| Interface                         | Install         | Runtime baseline         |
| --------------------------------- | --------------- | ------------------------ |
| [Node.js](#install-the-node-sdk)  | npm             | Node.js 22 or newer      |
| [Python](#install-the-python-sdk) | PyPI            | Python 3.10 through 3.14 |
| [Rust](#use-rust)                 | Source checkout | Rust 1.97                |

Native packages support Linux x86-64 with glibc, macOS arm64, macOS x86-64,
and Windows x86-64. Linux hosts also need the browser libraries listed in
[Linux runtime libraries](#linux-runtime-libraries).

## Install the Node SDK

```console
npm install offprint
```

Continue with the [Node.js integration](../integrations/node.md).

## Install the Python SDK

```console
uv add offprint
```

The package is published on [PyPI](https://pypi.org/project/offprint/), the Python
package index. Its wheels use Python's stable application binary interface from
Python 3.10. Continue with the [Python integration](../integrations/python.md).

## Use Rust

Use a checkout as a path dependency:

```toml
[dependencies]
offprint = { path = "/path/to/offprint/offprint-rs/core" }
```

Read the [Rust integration](../integrations/rust.md) for the API and
[contributor setup](https://github.com/peter-gy/offprint/blob/main/development_docs/setup.md)
for source builds.

## Linux runtime libraries

Linux capture hosts need Chromium's shared libraries. On Ubuntu 24.04:

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

Package names differ across Linux distributions. Verify that a Chromium-based
browser can start in the target image before capture.

## Browser requirement

Offprint discovers Chrome, Chromium, or Microsoft Edge based on Chromium 120 or
newer. When automatic installation is enabled and no compatible browser is
present, first use downloads and verifies the pinned
[Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/)
build, Google's versioned Chromium browser distribution.

Run `offprint doctor` to check browser availability, or call
`offprint.browsers.doctor()` through an SDK. Read
[browsers and ownership](../concepts/browsers.md) when provisioning a browser,
sharing a cache, or connecting to a remote browser.
