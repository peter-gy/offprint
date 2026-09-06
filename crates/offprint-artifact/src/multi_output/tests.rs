use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use offprint_model::{
    ArtifactFormat, ConflictPolicy, ContentDigest, FormatVerification, OffprintError, PortablePath,
};

use super::fault::{FaultAction, FaultInjector, FaultPoint, OneFault};
use super::journal::JournalPhase;
use super::{ArtifactDirectory, ArtifactTransaction, ArtifactTransactionLimits, PreparedArtifact};

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn empty_output_commits_as_one_staged_directory() -> TestResult {
    let fixture = Fixture::new()?;
    let transaction = fixture.stage(
        ConflictPolicy::Fail,
        vec![
            verified_file("capture.zip", b"archive")?,
            verified_directory("capture-markdown")?,
        ],
    )?;
    let root = transaction.root().to_owned();

    let results = transaction.commit()?;

    assert_eq!(results.len(), 2);
    assert_eq!(fs::read(fixture.output.join("capture.zip"))?, b"archive");
    assert_eq!(
        fs::read(fixture.output.join("capture-markdown").join("index.md"))?,
        b"# capture\n"
    );
    assert!(!root.exists());
    assert!(transaction_roots(&fixture)?.is_empty());
    Ok(())
}

#[test]
fn directory_payload_commits_nested_files_and_explicit_entrypoint() -> TestResult {
    let fixture = Fixture::new()?;
    let directory = ArtifactDirectory::new(BTreeMap::from([
        ("assets/charts/plot.bin".to_owned(), b"plot".to_vec()),
        ("pages/readme.txt".to_owned(), b"capture".to_vec()),
    ]))?;
    let verification = verification(
        ArtifactFormat::Markdown,
        directory.bytes(),
        directory.sha256(),
    );
    let artifact =
        PreparedArtifact::directory("capture-tree", "pages/readme.txt", directory, verification)?;

    let results = fixture
        .stage(ConflictPolicy::Fail, vec![artifact])?
        .commit()?;

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entrypoint.file_name(), Some("readme.txt"));
    assert_eq!(
        fs::read(fixture.output.join("capture-tree/assets/charts/plot.bin"))?,
        b"plot"
    );
    assert_eq!(
        fs::read(fixture.output.join("capture-tree/pages/readme.txt"))?,
        b"capture"
    );
    Ok(())
}

#[test]
fn preflight_file_limit_applies_to_single_file_payloads() -> TestResult {
    let fixture = Fixture::new()?;

    let error = ArtifactTransaction::stage(
        &fixture.portable,
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"archive")?],
        ArtifactTransactionLimits::new(0, 1024),
    )
    .err()
    .ok_or("zero-file transaction limit was ignored")?;

    assert_eq!(error.code.as_str(), "offprint.export.files");
    assert!(fs::read_dir(&fixture.output)?.next().is_none());
    Ok(())
}

#[test]
fn preflight_directory_limit_runs_before_filesystem_staging() -> TestResult {
    let fixture = Fixture::new()?;
    let directory =
        ArtifactDirectory::new(BTreeMap::from([("one/two/index.txt".to_owned(), vec![])]))?;
    let verification = verification(
        ArtifactFormat::Markdown,
        directory.bytes(),
        directory.sha256(),
    );
    let artifact =
        PreparedArtifact::directory("capture-tree", "one/two/index.txt", directory, verification)?;

    let error = ArtifactTransaction::stage(
        &fixture.portable,
        ConflictPolicy::Fail,
        vec![artifact],
        ArtifactTransactionLimits::new(1, 1024),
    )
    .err()
    .ok_or("directory count limit was ignored")?;

    assert_eq!(error.code.as_str(), "offprint.export.files");
    assert!(fs::read_dir(&fixture.output)?.next().is_none());
    Ok(())
}

#[test]
fn preflight_conflict_creates_no_transaction() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.output.join("second.zip"), b"occupied")?;

    let error = fixture
        .stage(
            ConflictPolicy::Fail,
            vec![
                verified_file("first.zip", b"first")?,
                verified_file("second.zip", b"second")?,
            ],
        )
        .err()
        .ok_or("conflicting transaction was accepted")?;

    assert_eq!(error.code.as_str(), "offprint.output.exists");
    assert!(!fixture.output.join("first.zip").exists());
    assert_eq!(fs::read(fixture.output.join("second.zip"))?, b"occupied");
    assert!(transaction_roots(&fixture)?.is_empty());
    Ok(())
}

#[test]
fn live_lock_blocks_a_second_service_and_stale_prepared_state_recovers() -> TestResult {
    let fixture = Fixture::new()?;
    let first = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"first")?],
    )?;
    let stale_root = first.root().to_owned();

    let locked = fixture
        .stage(
            ConflictPolicy::Fail,
            vec![verified_file("capture.zip", b"second")?],
        )
        .err()
        .ok_or("live transaction lock was not enforced")?;
    assert!(locked.retryable);
    assert!(stale_root.exists());

    drop(first);
    let recovered = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"second")?],
    )?;
    assert!(!stale_root.exists());
    recovered.commit()?;
    assert_eq!(fs::read(fixture.output.join("capture.zip"))?, b"second");
    Ok(())
}

#[test]
fn staging_journal_crash_is_cleaned_on_the_next_entry() -> TestResult {
    let fixture = Fixture::new()?;
    let mut fault = OneFault::crash(FaultPoint::AfterJournal(JournalPhase::Staging));

    let error = ArtifactTransaction::stage_with_injector(
        &fixture.portable,
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"first")?],
        ArtifactTransactionLimits::new(10, 1024),
        &mut fault,
    )
    .err()
    .ok_or("staging journal crash was not injected")?;
    let root = transaction_path(&error)?;
    assert!(root.exists());

    let recovered = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"second")?],
    )?;
    assert!(!root.exists());
    recovered.commit()?;
    Ok(())
}

#[test]
fn journal_failure_before_mutation_rolls_back_cleanly() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.output.join("unrelated"), b"keeps entry mode")?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"new")?],
    )?;
    let root = transaction.root().to_owned();
    let mut fault = OneFault::error(FaultPoint::AfterJournal(JournalPhase::Mutating));

    let error = transaction
        .commit_with_injector(&mut fault)
        .err()
        .ok_or("journal boundary failure was ignored")?;

    assert_eq!(
        error.details.get("rollbackComplete"),
        Some(&serde_json::Value::Bool(true))
    );
    assert!(!fixture.output.join("capture.zip").exists());
    assert!(!root.exists());
    Ok(())
}

#[test]
fn crash_after_backup_restores_every_replacement_on_next_entry() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.output.join("first.zip"), b"old-first")?;
    fs::write(fixture.output.join("second.zip"), b"old-second")?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Replace,
        vec![
            verified_file("first.zip", b"new-first")?,
            verified_file("second.zip", b"new-second")?,
        ],
    )?;
    let stale_root = transaction.root().to_owned();
    let mut fault = OneFault::crash(FaultPoint::AfterBackup(0));

    let error = transaction
        .commit_with_injector(&mut fault)
        .err()
        .ok_or("backup crash was not injected")?;
    assert_eq!(
        error.details.get("recoveryPending"),
        Some(&serde_json::Value::Bool(true))
    );
    drop(transaction);
    assert!(!fixture.output.join("first.zip").exists());

    let recovered = fixture.stage(
        ConflictPolicy::Replace,
        vec![
            verified_file("first.zip", b"final-first")?,
            verified_file("second.zip", b"final-second")?,
        ],
    )?;
    assert_eq!(fs::read(fixture.output.join("first.zip"))?, b"old-first");
    assert_eq!(fs::read(fixture.output.join("second.zip"))?, b"old-second");
    assert!(!stale_root.exists());
    recovered.commit()?;
    Ok(())
}

#[test]
fn crash_after_first_commit_restores_the_original_set() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.output.join("first.zip"), b"old-first")?;
    fs::write(fixture.output.join("second.zip"), b"old-second")?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Replace,
        vec![
            verified_file("first.zip", b"new-first")?,
            verified_file("second.zip", b"new-second")?,
        ],
    )?;
    let mut fault = OneFault::crash(FaultPoint::AfterCommit(0));

    transaction
        .commit_with_injector(&mut fault)
        .err()
        .ok_or("commit crash was not injected")?;
    drop(transaction);
    assert_eq!(fs::read(fixture.output.join("first.zip"))?, b"new-first");

    let recovered = fixture.stage(
        ConflictPolicy::Replace,
        vec![
            verified_file("first.zip", b"final-first")?,
            verified_file("second.zip", b"final-second")?,
        ],
    )?;
    assert_eq!(fs::read(fixture.output.join("first.zip"))?, b"old-first");
    assert_eq!(fs::read(fixture.output.join("second.zip"))?, b"old-second");
    recovered.commit()?;
    Ok(())
}

#[test]
fn directory_swap_crash_restores_the_empty_output_directory() -> TestResult {
    let fixture = Fixture::new()?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"new")?],
    )?;
    let mut fault = OneFault::crash(FaultPoint::AfterBackup(0));

    transaction
        .commit_with_injector(&mut fault)
        .err()
        .ok_or("directory backup crash was not injected")?;
    drop(transaction);
    assert!(!fixture.output.exists());
    fs::create_dir(&fixture.output)?;

    let recovered = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"final")?],
    )?;
    assert!(fixture.output.read_dir()?.next().is_none());
    recovered.commit()?;
    Ok(())
}

#[test]
fn directory_swap_restores_content_added_during_the_commit_window() -> TestResult {
    let fixture = Fixture::new()?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"new")?],
    )?;
    let mut injector = AddFileAfterBackup {
        path: transaction
            .root()
            .join("previous-output")
            .join("concurrent"),
        error: None,
        fired: false,
    };

    let error = transaction
        .commit_with_injector(&mut injector)
        .err()
        .ok_or("directory-swap mutation was not detected")?;
    if let Some(source) = injector.error {
        return Err(source.into());
    }

    assert!(injector.fired);
    assert_eq!(
        error.details.get("rollbackComplete"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(fs::read(fixture.output.join("concurrent"))?, b"external");
    assert!(!fixture.output.join("capture.zip").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn failed_commit_journal_write_leaves_the_installed_set_recoverable() -> TestResult {
    let fixture = Fixture::new()?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"installed")?],
    )?;
    let mut injector = MakeJournalUnwritableAfterCommit {
        root: transaction.root().to_owned(),
        original_mode: None,
        error: None,
        fired: false,
    };

    let result = transaction.commit_with_injector(&mut injector);
    injector.restore()?;
    if let Some(source) = injector.error {
        return Err(source.into());
    }
    let error = result
        .err()
        .ok_or("commit succeeded without its durable journal snapshot")?;

    assert!(injector.fired);
    assert_eq!(
        error.details.get("commitStatus"),
        Some(&serde_json::Value::String("indeterminate".to_owned()))
    );
    assert_eq!(
        error.details.get("outputsInstalled"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(fs::read(fixture.output.join("capture.zip"))?, b"installed");

    drop(transaction);
    let recovered = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"final")?],
    )?;
    assert!(!fixture.output.join("capture.zip").exists());
    recovered.commit()?;
    Ok(())
}

#[test]
fn committed_journal_finalizes_cleanup_without_reverting_outputs() -> TestResult {
    let fixture = Fixture::new()?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Fail,
        vec![verified_file("capture.zip", b"committed")?],
    )?;
    let stale_root = transaction.root().to_owned();
    let mut fault = OneFault::crash(FaultPoint::BeforeCleanup);

    let error = transaction
        .commit_with_injector(&mut fault)
        .err()
        .ok_or("cleanup crash was not injected")?;
    assert_eq!(
        error.details.get("commitComplete"),
        Some(&serde_json::Value::Bool(true))
    );
    drop(transaction);
    assert_eq!(fs::read(fixture.output.join("capture.zip"))?, b"committed");

    let recovered = fixture.stage(
        ConflictPolicy::Replace,
        vec![verified_file("capture.zip", b"next")?],
    )?;
    assert!(!stale_root.exists());
    assert_eq!(fs::read(fixture.output.join("capture.zip"))?, b"committed");
    recovered.commit()?;
    Ok(())
}

#[test]
fn changed_destination_preserves_backup_and_reports_recovery_paths() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.output.join("capture.zip"), b"old")?;
    let mut transaction = fixture.stage(
        ConflictPolicy::Replace,
        vec![verified_file("capture.zip", b"new")?],
    )?;
    let stale_root = transaction.root().to_owned();
    let mut fault = OneFault::crash(FaultPoint::AfterBackup(0));

    transaction
        .commit_with_injector(&mut fault)
        .err()
        .ok_or("backup crash was not injected")?;
    drop(transaction);
    fs::write(fixture.output.join("capture.zip"), b"external")?;

    let error = fixture
        .stage(
            ConflictPolicy::Replace,
            vec![verified_file("capture.zip", b"next")?],
        )
        .err()
        .ok_or("changed destination was overwritten during recovery")?;

    assert_eq!(
        error.details.get("recoveryPending"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        error.details.get("partialCommit"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(fs::read(fixture.output.join("capture.zip"))?, b"external");
    assert!(stale_root.join("backups").join("entry-0").exists());
    assert!(
        error
            .details
            .get("recoveryPaths")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|paths| !paths.is_empty())
    );
    Ok(())
}

struct Fixture {
    _directory: tempfile::TempDir,
    output: PathBuf,
    portable: PortablePath,
}

struct AddFileAfterBackup {
    path: PathBuf,
    error: Option<std::io::Error>,
    fired: bool,
}

impl FaultInjector for AddFileAfterBackup {
    fn action(&mut self, point: FaultPoint) -> Option<FaultAction> {
        if !self.fired && point == FaultPoint::AfterBackup(0) {
            self.fired = true;
            if let Err(error) = fs::write(&self.path, b"external") {
                self.error = Some(error);
            }
        }
        None
    }
}

#[cfg(unix)]
struct MakeJournalUnwritableAfterCommit {
    root: PathBuf,
    original_mode: Option<u32>,
    error: Option<std::io::Error>,
    fired: bool,
}

#[cfg(unix)]
impl MakeJournalUnwritableAfterCommit {
    fn restore(&self) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;

        if let Some(mode) = self.original_mode {
            fs::set_permissions(&self.root, fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
impl FaultInjector for MakeJournalUnwritableAfterCommit {
    fn action(&mut self, point: FaultPoint) -> Option<FaultAction> {
        use std::os::unix::fs::PermissionsExt as _;

        if !self.fired && point == FaultPoint::AfterCommit(0) {
            self.fired = true;
            match fs::metadata(&self.root) {
                Ok(metadata) => {
                    self.original_mode = Some(metadata.permissions().mode());
                    if let Err(error) =
                        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o500))
                    {
                        self.error = Some(error);
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
        None
    }
}

impl Fixture {
    fn new() -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("output");
        fs::create_dir(&output)?;
        let portable = PortablePath::from_path_buf(output.clone())?;
        Ok(Self {
            _directory: directory,
            output,
            portable,
        })
    }

    fn stage(
        &self,
        conflict: ConflictPolicy,
        prepared: Vec<PreparedArtifact>,
    ) -> Result<ArtifactTransaction, OffprintError> {
        ArtifactTransaction::stage(
            &self.portable,
            conflict,
            prepared,
            ArtifactTransactionLimits::new(10, 1024),
        )
    }
}

fn verified_file(name: &str, contents: &[u8]) -> Result<PreparedArtifact, OffprintError> {
    PreparedArtifact::file(
        name.to_owned(),
        contents.to_vec(),
        verification(
            ArtifactFormat::Zip,
            u64::try_from(contents.len()).unwrap_or(u64::MAX),
            ContentDigest::sha256(contents),
        ),
    )
}

fn verified_directory(name: &str) -> Result<PreparedArtifact, OffprintError> {
    let directory = ArtifactDirectory::new(BTreeMap::from([(
        "index.md".to_owned(),
        b"# capture\n".to_vec(),
    )]))?;
    let verification = verification(
        ArtifactFormat::Markdown,
        directory.bytes(),
        directory.sha256(),
    );
    PreparedArtifact::directory(name.to_owned(), "index.md", directory, verification)
}

fn verification(format: ArtifactFormat, bytes: u64, sha256: ContentDigest) -> FormatVerification {
    FormatVerification {
        format,
        bytes,
        sha256,
    }
}

fn transaction_path(error: &OffprintError) -> TestResult<PathBuf> {
    let path = error
        .details
        .get("transactionPath")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing transaction path")?;
    Ok(Path::new(path).to_owned())
}

fn transaction_roots(fixture: &Fixture) -> TestResult<Vec<PathBuf>> {
    let parent = fixture.output.parent().ok_or("missing output parent")?;
    let namespace = fs::read_dir(parent)?
        .filter_map(|entry| entry.ok())
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".offprint-transactions-")
        })
        .ok_or("missing transaction namespace")?;
    Ok(fs::read_dir(namespace.path())?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("txn-"))
        .map(|entry| entry.path())
        .collect())
}
