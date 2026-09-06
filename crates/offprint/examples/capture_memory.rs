use futures_util::StreamExt as _;
use offprint::{CaptureOutput, CaptureRequest, Offprint};

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let offprint = Offprint::new()?;
    let mut request = CaptureRequest::builder("https://example.com")?.build()?;
    request.output = CaptureOutput::memory(16 * 1024 * 1024);

    let job = offprint.captures().start(request).await?;
    let mut events = job.events();
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
