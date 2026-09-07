use std::process::Command;

#[test]
fn help_and_version_exit_successfully() -> Result<(), Box<dyn std::error::Error>> {
    for arguments in [
        &["--help"][..],
        &["--version"][..],
        &["capture", "--json", "--help"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_offprint"))
            .args(arguments)
            .output()?;

        assert!(
            output.status.success(),
            "{arguments:?} exited with {output:?}"
        );
        assert!(output.stderr.is_empty(), "{arguments:?} wrote to stderr");
        assert!(!output.stdout.is_empty(), "{arguments:?} wrote no output");
    }
    Ok(())
}
