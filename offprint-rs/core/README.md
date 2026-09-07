# `offprint`

The `offprint` crate captures rendered pages through a shared Chromium service
and returns typed receipts with artifact, verification, resource, warning, and
timing records.

Offprint is currently alpha and requires Rust 1.97.

Add the crate from a local checkout and the Tokio runtime:

```toml
[dependencies]
offprint = { path = "/path/to/offprint/offprint-rs/core" }
tokio = { version = "=1.53.1", features = ["macros", "rt-multi-thread"] }
```

```rust
use offprint::Offprint;

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let offprint = Offprint::new()?;
    let receipt = offprint
        .capture("https://example.com")?
        .save("example.html")
        .await?;

    println!("{}", receipt.artifact.sha256());
    offprint.close().await?;
    Ok(())
}
```

Use `CaptureService` for complete requests, jobs, events, batches, and crawls.
Use `ArtifactService` to inspect, verify, commit, and export artifacts. Use
`BrowserService` for discovery and managed browser operations.

Read the complete [Rust integration guide](https://github.com/peter-gy/offprint/blob/main/docs/integrations/rust.md)
and [record reference](https://github.com/peter-gy/offprint/blob/main/docs/reference/records.md).
