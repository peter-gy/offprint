use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use offprint_model::ArtifactManifest;

use crate::state_restoration_script_digest;

#[must_use]
pub fn content_security_policy(manifest: &ArtifactManifest) -> String {
    content_security_policy_with_state_restoration(manifest, true)
}

#[must_use]
pub(crate) fn sandboxed_content_security_policy(manifest: &ArtifactManifest) -> String {
    content_security_policy_with_state_restoration(manifest, false)
}

fn content_security_policy_with_state_restoration(
    manifest: &ArtifactManifest,
    restore_state: bool,
) -> String {
    let mut script_sources = restore_state
        .then(state_restoration_script_digest)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(digest) = manifest.structural_repair.script_sha256 {
        script_sources.push(digest);
    }
    let script_policy = if script_sources.is_empty() {
        "script-src 'none'".to_owned()
    } else {
        format!(
            "script-src {}",
            script_sources
                .into_iter()
                .map(|digest| format!("'sha256-{}'", STANDARD.encode(digest.as_bytes())))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    [
        "default-src 'none'",
        "base-uri 'none'",
        "connect-src 'none'",
        "font-src data:",
        "form-action 'none'",
        "frame-src data: blob:",
        "img-src data: blob:",
        "media-src data: blob:",
        "object-src 'none'",
        script_policy.as_str(),
        "style-src 'unsafe-inline' data:",
    ]
    .join("; ")
}
