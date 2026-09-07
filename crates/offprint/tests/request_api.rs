use std::time::Duration;

use offprint::{
    CaptureOutput, CaptureProfile, ConflictPolicy, MissingResourcePolicy, NetworkPolicy, Offprint,
    ReadinessMode,
};

#[tokio::test]
async fn fluent_request_preserves_profile_and_explicit_policies() -> offprint::Result<()> {
    let offprint = Offprint::builder().profile("server").build()?;
    let request = offprint
        .capture("https://example.com")?
        .wait_until(ReadinessMode::NetworkIdle)
        .delay(Duration::from_millis(750))
        .output(CaptureOutput::file("capture.html".into()))
        .conflict(ConflictPolicy::Replace)
        .into_request();

    assert_eq!(request.url.as_str(), "https://example.com/");
    assert_eq!(
        request.content.missing_resources,
        MissingResourcePolicy::Fail
    );
    assert_eq!(request.network, NetworkPolicy::Server);
    assert_eq!(request.readiness.mode, ReadinessMode::NetworkIdle);
    assert_eq!(request.readiness.delay.get(), 750);
    assert_eq!(
        request.output,
        CaptureOutput::File {
            path: "capture.html".into(),
            conflict: ConflictPolicy::Replace,
        }
    );
    offprint.close().await
}

#[tokio::test]
async fn request_customization_is_validated_before_job_start() -> offprint::Result<()> {
    let offprint = Offprint::new()?;
    let mut request = offprint.capture("https://example.com")?.into_request();
    request.limits.resources = 0;

    let result = offprint.captures().start(request).await;

    assert_eq!(
        result.err().map(|error| error.code.as_str().to_owned()),
        Some("offprint.input.limit".to_owned())
    );
    offprint.close().await
}

#[tokio::test]
async fn default_memory_request_inherits_registered_profile_limit() -> offprint::Result<()> {
    let mut profile = CaptureProfile::default();
    profile.limits.artifact_bytes = 1024;
    let offprint = Offprint::builder()
        .register_profile("small", profile)
        .profile("small")
        .build()?;
    let request = offprint.capture("https://example.com")?.into_request();

    assert_eq!(request.output, CaptureOutput::Memory { max_bytes: 1024 });
    request.validate()?;
    offprint.close().await
}
