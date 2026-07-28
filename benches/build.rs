use std::io::Write as _;
use std::process::Command;

fn main() {
    let mut stdout = std::io::stdout().lock();
    let _ignored = writeln!(stdout, "cargo:rerun-if-env-changed=RUSTC");
    let _ignored = writeln!(stdout, "cargo:rerun-if-env-changed=RUSTUP_TOOLCHAIN");
    let output = std::env::var("RUSTUP_TOOLCHAIN")
        .ok()
        .and_then(|toolchain| {
            Command::new("rustup")
                .args(["run", &toolchain, "rustc", "--version"])
                .output()
                .ok()
        })
        .or_else(|| {
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            Command::new(rustc).arg("--version").output().ok()
        });
    let version = output
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let _ignored = writeln!(
        stdout,
        "cargo:rustc-env=PAGEKNOT_BENCH_RUSTC_VERSION={version}"
    );
}
