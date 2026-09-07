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
