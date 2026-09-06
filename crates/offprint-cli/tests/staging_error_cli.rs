use std::error::Error;
use std::process::Command;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn offline_verification_staging_failure_is_a_runtime_error() -> TestResult {
    let manifest = serde_json::from_str::<offprint::ArtifactManifest>(include_str!(
        "../../../schemas/examples/artifact-manifest.json"
    ))?;
    let document =
        offprint_document::Document::parse(b"<!doctype html><html><head></head><body></body>");
    let artifact = offprint_html::encode_html(&document, &manifest)?;
    let directory = tempfile::tempdir()?;
    let artifact_path = directory.path().join("capture.html");
    std::fs::write(&artifact_path, artifact)?;
    let invalid_temporary_directory = directory.path().join("not-a-directory");
    std::fs::write(&invalid_temporary_directory, b"file")?;
    let diagnostics_directory = directory.path().join("diagnostics");

    let result = Command::new(env!("CARGO_BIN_EXE_offprint"))
        .args([
            "artifact",
            "verify",
            artifact_path
                .to_str()
                .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?,
            "--verification",
            "offline",
            "--diagnostics",
            diagnostics_directory
                .to_str()
                .ok_or_else(|| std::io::Error::other("diagnostic path is not UTF-8"))?,
            "--json",
            "--color",
            "never",
        ])
        .env("TMPDIR", &invalid_temporary_directory)
        .env("TMP", &invalid_temporary_directory)
        .env("TEMP", &invalid_temporary_directory)
        .output()?;

    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&result.stderr)?;
    assert_eq!(error["code"], "offprint.output.staging");
    assert!(error["diagnosticsPath"].is_string());
    let bundle: serde_json::Value = serde_json::from_slice(&std::fs::read(
        diagnostics_directory.join("verification.diagnostics.json"),
    )?)?;
    assert_eq!(bundle["operation"], "verify");
    assert_eq!(bundle["verificationMode"], "offline");
    assert_eq!(bundle["failure"]["code"], "offprint.output.staging");
    assert_eq!(bundle["failure"]["stage"], "encoding");
    assert!(
        bundle["failure"]["retryable"]
            .as_bool()
            .is_some_and(|retryable| retryable)
    );
    assert_eq!(
        bundle["failure"]["chain"][0]["code"],
        "offprint.output.staging"
    );
    Ok(())
}
