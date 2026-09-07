use std::collections::BTreeMap;
use std::io::Read;

use offprint::{
    BrowserCookie, CaptureCredentials, ErrorStage, OffprintError, RequestHeader, Result,
    SecretString,
};
use serde::Deserialize;
use zeroize::Zeroizing;

pub(crate) fn validate_credential_streams(
    output: Option<&str>,
    headers: Option<&str>,
    cookies: Option<&str>,
) -> Result<()> {
    let headers_from_stdin = headers == Some("-");
    let cookies_from_stdin = cookies == Some("-");
    if output == Some("-") && (headers_from_stdin || cookies_from_stdin) {
        return Err(OffprintError::new(
            "offprint.input.credentials",
            ErrorStage::Validation,
            "credential input and artifact output cannot share stdin and stdout mode",
        ));
    }
    if headers_from_stdin && cookies_from_stdin {
        return Err(OffprintError::new(
            "offprint.input.credentials",
            ErrorStage::Validation,
            "header and cookie input cannot both consume stdin",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HeaderInput {
    Map(BTreeMap<String, String>),
    List(Vec<RequestHeader>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CookieInput {
    List(Vec<BrowserCookie>),
    Object { cookies: Vec<BrowserCookie> },
}

pub(crate) fn read_credentials(
    headers_path: Option<&str>,
    cookies_path: Option<&str>,
    input: &mut dyn Read,
) -> Result<CaptureCredentials> {
    let headers = match headers_path {
        Some(path) => {
            let bytes = read_protected_input(path, input, "header")?;
            match serde_json::from_slice::<HeaderInput>(&bytes).map_err(|error| {
                credential_error(format!("header file is not valid JSON: {error}"))
            })? {
                HeaderInput::Map(values) => values
                    .into_iter()
                    .map(|(name, value)| RequestHeader {
                        name,
                        value: SecretString::new(value),
                    })
                    .collect(),
                HeaderInput::List(values) => values,
            }
        }
        None => Vec::new(),
    };
    let cookies = match cookies_path {
        Some(path) => {
            let bytes = read_protected_input(path, input, "cookie")?;
            match serde_json::from_slice::<CookieInput>(&bytes).map_err(|error| {
                credential_error(format!("cookie file is not valid JSON: {error}"))
            })? {
                CookieInput::List(values) | CookieInput::Object { cookies: values } => values,
            }
        }
        None => Vec::new(),
    };
    Ok(CaptureCredentials { headers, cookies })
}

fn read_protected_input(
    path: &str,
    input: &mut dyn Read,
    kind: &'static str,
) -> Result<Zeroizing<Vec<u8>>> {
    const MAXIMUM_CREDENTIAL_BYTES: u64 = 1024 * 1024;
    let mut bytes = Zeroizing::new(Vec::new());
    if path == "-" {
        input
            .take(MAXIMUM_CREDENTIAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                credential_error(format!("failed to read {kind} input from stdin: {error}"))
            })?;
    } else {
        let metadata = std::fs::symlink_metadata(path).map_err(|error| {
            credential_error(format!("failed to inspect protected {kind} file: {error}"))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(credential_error(format!(
                "protected {kind} input must be a regular file"
            )));
        }
        validate_secret_permissions(&metadata, std::path::Path::new(path), kind)?;
        std::fs::File::open(path)
            .map_err(|error| {
                credential_error(format!("failed to open protected {kind} file: {error}"))
            })?
            .take(MAXIMUM_CREDENTIAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                credential_error(format!("failed to read protected {kind} file: {error}"))
            })?;
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_CREDENTIAL_BYTES {
        return Err(credential_error(format!(
            "protected {kind} input exceeds the byte limit"
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn validate_secret_permissions(
    metadata: &std::fs::Metadata,
    _path: &std::path::Path,
    kind: &'static str,
) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;

    if metadata.mode() & 0o077 != 0 {
        Err(credential_error(format!(
            "protected {kind} file must not grant group or other permissions"
        )))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn validate_secret_permissions(
    _metadata: &std::fs::Metadata,
    path: &std::path::Path,
    kind: &'static str,
) -> Result<()> {
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const ACL_CHECK: &str = r#"
$ErrorActionPreference = 'Stop'
try {
    $path = [Environment]::GetEnvironmentVariable('OFFPRINT_CREDENTIAL_PATH', 'Process')
    $acl = Get-Acl -LiteralPath $path
    $current = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $owner = $acl.Owner
    try {
        $owner = ([System.Security.Principal.NTAccount]$owner).Translate(
            [System.Security.Principal.SecurityIdentifier]
        ).Value
    } catch {
        $owner = ([System.Security.Principal.SecurityIdentifier]$owner).Value
    }
    $allowed = @($current, $owner, 'S-1-5-18', 'S-1-5-32-544')
    foreach ($entry in $acl.Access) {
        if ($entry.AccessControlType -ne 'Allow') {
            continue
        }
        $sid = $entry.IdentityReference.Translate(
            [System.Security.Principal.SecurityIdentifier]
        ).Value
        if ($allowed -notcontains $sid) {
            exit 3
        }
    }
    exit 0
} catch {
    exit 4
}
"#;

    let system_root = std::env::var_os("SystemRoot").ok_or_else(|| {
        credential_error(format!(
            "protected {kind} file ACL validation is unavailable because SystemRoot is unset"
        ))
    })?;
    let powershell = std::path::PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if !powershell.is_file() {
        return Err(credential_error(format!(
            "protected {kind} file ACL validation is unavailable"
        )));
    }
    let status = Command::new(powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            ACL_CHECK,
        ])
        .env("OFFPRINT_CREDENTIAL_PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| {
            credential_error(format!(
                "failed to validate protected {kind} file ACL: {error}"
            ))
        })?;
    match status.code() {
        Some(0) => Ok(()),
        Some(3) => Err(credential_error(format!(
            "protected {kind} file must not grant access to other principals"
        ))),
        _ => Err(credential_error(format!(
            "protected {kind} file ACL could not be validated"
        ))),
    }
}

#[cfg(not(any(unix, windows)))]
fn validate_secret_permissions(
    _metadata: &std::fs::Metadata,
    _path: &std::path::Path,
    kind: &'static str,
) -> Result<()> {
    Err(credential_error(format!(
        "protected {kind} file permissions cannot be validated on this platform"
    )))
}

fn credential_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new(
        "offprint.input.credentials",
        ErrorStage::Validation,
        message,
    )
}
