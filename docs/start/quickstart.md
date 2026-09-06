# Capture and verify one page

This quickstart builds the current alpha CLI, captures one rendered page, reads
its artifact manifest, and repeats offline verification.

## Prerequisites

- A checkout of this repository
- Rust 1.97, selected automatically by `rust-toolchain.toml`
- Network access for the page and, when needed, the pinned managed browser

Offprint uses a compatible Chrome, Chromium, or Microsoft Edge installation
when discovery finds one. Otherwise the first capture downloads the pinned
[Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/)
archive, verifies its SHA-256 digest, and stores it in the managed cache.

## Build the CLI

Run from the repository root:

```console
cargo build --release --locked -p offprint-cli
```

Check browser discovery, collector compatibility, the configured network-policy
summary, and atomic output capabilities in the current directory:

```console
./target/release/offprint doctor
```

`doctor` can launch a short-lived local browser probe or create a temporary
context on a configured remote endpoint. It does not install a browser or
start a capture.

## Capture the page

```console
./target/release/offprint capture https://example.com \
  --output example.html \
  --quiet
```

The command prints the committed path to standard output:

```text
example.html
```

The file appears after capture and verification succeed. The default conflict
policy returns an error when `example.html` already exists. Use
`--on-exists replace` to replace it after verification, or
`--on-exists uniquify` to choose the first available numbered name.

Offprint executes the source page inside an isolated browser context. Treat the
URL, page, artifact, and any diagnostics as untrusted content.

## Inspect the artifact manifest

```console
./target/release/offprint artifact inspect example.html
```

Inspection parses and validates the embedded artifact manifest. It reports the
format version, redacted source, browser, environment, policy digest, frame and
resource counts, warning codes, and requested verification mode. Inspection
does not verify the rest of the artifact.

## Repeat offline verification

```console
./target/release/offprint artifact verify example.html \
  --verification offline
```

Offline verification first applies static checks, then reopens the file in a
fresh Chromium context with network access denied. A successful run reports
zero observed network requests.

Verification establishes Offprint's self-containment and structural policy. It
does not certify the page's truth or make private captured content safe to
share.

## Continue

- [Control readiness, selection, and fidelity](../guides/control-capture.md)
- [Capture authenticated pages](../guides/authenticated-pages.md)
- [Inspect, verify, and export artifacts](../guides/inspect-verify-export.md)
- [Automate captures and consume JSON](../guides/automation.md)
