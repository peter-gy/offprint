# Use Offprint from Rust

The `offprint` crate exposes one shared runtime handle and three service views
for captures, artifacts, and browsers. The current alpha requires Rust 1.97.

From this checkout, add the path dependency to a project:

```toml
[dependencies]
offprint = { path = "/path/to/offprint/offprint-rs/core" }
futures-util = "=0.3.33"
tokio = { version = "=1.53.1", features = ["macros", "rt-multi-thread"] }
```

## Capture one file

```rust
use offprint::{Offprint, VerificationMode};

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let offprint = Offprint::new()?;
    let receipt = offprint
        .capture("https://example.com")?
        .save("example.html")
        .await?;

    assert_eq!(receipt.verification.mode, VerificationMode::Offline);
    offprint.close().await?;
    Ok(())
}
```

`Offprint::capture` returns a pending `Capture` value. Browser work starts when
`save`, `bytes`, or `start` runs. File output uses `ConflictPolicy::Fail` by
default.

The same program lives in
[`capture_file.rs`](https://github.com/peter-gy/offprint/blob/main/offprint-rs/core/examples/capture_file.rs) and is
checked by `just docs-check`.

## Capture to memory and commit later

```rust
use offprint::{ConflictPolicy, Offprint};

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let offprint = Offprint::new()?;
    let receipt = offprint
        .capture("https://example.com")?
        .bytes(16 * 1024 * 1024)
        .await?;

    let name = offprint
        .artifacts()
        .suggested_capture_file_name(&receipt, "html")?;
    let receipt = offprint
        .artifacts()
        .commit_capture(receipt, name, ConflictPolicy::Fail)?;
    assert!(matches!(
        receipt.artifact,
        offprint::CaptureArtifact::File { .. }
    ));
    offprint.close().await?;
    Ok(())
}
```

`commit_capture` requires an in-memory receipt whose content, byte count,
artifact digest, and verification fields agree. These consistency checks do
not establish that a caller-constructed receipt came from Offprint.

## Configure the shared service

`OffprintBuilder` configures runtime-wide behavior:

| Method                       | Default          | Contract                                                                                                                                                                           |
| ---------------------------- | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `browser_path`               | Discovery        | Select a local executable                                                                                                                                                          |
| `cdp_url`                    | Unset            | Attach to a trusted [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/) endpoint. Requests must select unrestricted networking and static verification |
| `cache_dir`                  | Platform cache   | Store managed browsers and service cache                                                                                                                                           |
| `browser_source`             | `Auto`           | Limit discovery to automatic, managed, or system sources                                                                                                                           |
| `browser_installation`       | `InstallManaged` | Permit or forbid first-use managed installation                                                                                                                                    |
| `maximum_contexts`           | `4`              | Bound concurrent browser contexts                                                                                                                                                  |
| `browser_recycle_after_jobs` | `100`            | Restart the shared process after completed jobs                                                                                                                                    |
| `headed`                     | `false`          | Show locally launched browser windows                                                                                                                                              |
| `profile`                    | `default`        | Select the profile applied by `Offprint::capture`                                                                                                                                  |
| `register_profile`           | None             | Add a named `CaptureProfile`                                                                                                                                                       |
| `default_network_policy`     | `Standard`       | Select address policy for existing-artifact verification                                                                                                                           |

Custom clocks, capture ID generators, effective configuration records, and
browser backends support deterministic hosts and advanced adapters.

## Use a complete request and job

Select an output and call `start` to observe progress or cancel the capture:

```rust
use offprint::{CaptureOutput, Offprint};

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let offprint = Offprint::new()?;
    let job = offprint
        .capture("https://example.com")?
        .output(CaptureOutput::memory(16 * 1024 * 1024))
        .start()
        .await?;
    let mut events = job.events();

    use futures_util::StreamExt as _;
    while let Some(event) = events.next().await {
        if event.is_terminal() {
            break;
        }
    }

    let receipt = job.result().await?;
    assert!(receipt.resources.is_complete());
    offprint.close().await?;
    Ok(())
}
```

Call `into_request` to configure credentials, custom network rules, diagnostics,
or exact limits, then submit the request through `captures().start` or
`captures().batch`:

```rust
async fn configured(offprint: &offprint::Offprint) -> offprint::Result<()> {
    let mut request = offprint.capture("https://example.com")?.into_request();
    request.limits.resources = 500;
    let job = offprint.captures().start(request).await?;
    let receipt = job.result().await?;
    Ok(())
}
```

`CaptureRequest::builder(url)` constructs a service-independent request. Its
`output(CaptureOutput)` method accepts file or memory delivery, with file
conflict behavior carried by `CaptureOutput`. See [records](../reference/records.md).

The complete typed example lives in
[`capture_memory.rs`](https://github.com/peter-gy/offprint/blob/main/offprint-rs/core/examples/capture_memory.rs).

## Service APIs

The [service API reference](../reference/service-api.md) defines constructor
defaults, every service method, returns, errors, and lifecycle across hosts.

Generated rustdoc is the exact symbol inventory:

```console
cargo doc --manifest-path offprint-rs/Cargo.toml --open -p offprint
```

The advanced browser traits originate in `offprint-browser`. The service
reference maps the complete trait graph and links the compiling custom-backend
integration example.

`export` obtains fresh offline evidence for an HTML source. `export_capture`
reuses a receipt only when it contains matching offline evidence with zero
observed requests. A static remote-capture receipt cannot use that proof-reuse
path.
