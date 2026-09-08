use std::error::Error;
use std::fs;

use offprint_artifact::{ArtifactTransaction, ArtifactTransactionLimits, PreparedArtifact};
use offprint_model::{
    ArtifactFormat, ConflictPolicy, ContentDigest, FormatVerification, PortablePath,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[test]
fn bare_relative_output_commits_empty_and_existing_directories() -> TestResult {
    const CHILD: &str = "OFFPRINT_TEST_RELATIVE_EXPORT";
    if std::env::var_os(CHILD).is_none() {
        let directory = tempfile::tempdir()?;
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .arg("bare_relative_output_commits_empty_and_existing_directories")
            .arg("--exact")
            .arg("--nocapture")
            .current_dir(directory.path())
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let error = match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                Ok(None) => std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "relative export regression exceeded its deadline",
                ),
                Err(error) => error,
            };
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
        let output = child.wait_with_output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(directory.path().join("exports/first.zip"))?,
            b"first"
        );
        assert_eq!(
            fs::read(directory.path().join("exports/second.zip"))?,
            b"second"
        );
        return Ok(());
    }

    fs::create_dir("exports")?;
    let output = PortablePath::new("exports");
    for (name, content) in [
        ("first.zip", b"first".as_slice()),
        ("second.zip", b"second"),
    ] {
        let results = ArtifactTransaction::stage(
            &output,
            ConflictPolicy::Fail,
            vec![PreparedArtifact::file(
                name,
                content.to_vec(),
                FormatVerification {
                    format: ArtifactFormat::Zip,
                    bytes: u64::try_from(content.len())?,
                    sha256: ContentDigest::sha256(content),
                },
            )?],
            ArtifactTransactionLimits::new(10, 1024),
        )?
        .commit()?;
        assert_eq!(results.len(), 1);
    }
    Ok(())
}
