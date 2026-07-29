use pageknot_model::{
    ArtifactManifest, BrowserProduct, ContentDigest, ErrorStage, PageKnotError, ResourceSummary,
    Result, VerificationPolicy,
};

use super::source::{
    MAXIMUM_FIELD_CHARACTERS, MAXIMUM_LIST_ITEM_CHARACTERS, MAXIMUM_LIST_ITEMS, SourceMetadata,
    language_tag_is_valid,
};
use crate::pdf::semantics::PdfSemantics;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PdfMetadata {
    pub(super) source: SourceMetadata,
    pub(super) requested_url: String,
    pub(super) final_url: String,
    pub(super) requested_url_sha256: ContentDigest,
    pub(super) final_url_sha256: ContentDigest,
    pub(super) source_artifact_sha256: ContentDigest,
    pub(super) policy_sha256: ContentDigest,
    pub(super) captured_at: String,
    pub(super) generator_name: String,
    pub(super) generator_version: String,
    pub(super) schema_version: u32,
    pub(super) artifact_format_version: u32,
    pub(super) browser_product: BrowserProduct,
    pub(super) browser_version: String,
    pub(super) browser_revision: Option<String>,
    pub(super) protocol_version: String,
    pub(super) locale: String,
    pub(super) timezone: String,
    pub(super) verification_level: VerificationPolicy,
    pub(super) frames: u32,
    pub(super) resources: ResourceSummary,
    pub(super) semantics: PdfSemantics,
    pub(super) structural_repair_applied: bool,
    pub(super) warning_codes: Vec<String>,
    pub(super) pdf_version: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct InfoField {
    pub(super) key: &'static str,
    pub(super) value: Option<String>,
}

impl PdfMetadata {
    pub(super) fn new(
        source: SourceMetadata,
        manifest: &ArtifactManifest,
        source_artifact_sha256: ContentDigest,
        semantics: PdfSemantics,
        pdf_version: String,
    ) -> Result<Self> {
        let metadata = Self {
            source,
            requested_url: manifest.source.requested_url.as_str().to_owned(),
            final_url: manifest.source.final_url.as_str().to_owned(),
            requested_url_sha256: manifest.source.requested_url_sha256,
            final_url_sha256: manifest.source.final_url_sha256,
            source_artifact_sha256,
            policy_sha256: manifest.policy_sha256,
            captured_at: manifest.captured_at.to_rfc3339(),
            generator_name: manifest.generator.name.clone(),
            generator_version: manifest.generator.version.clone(),
            schema_version: manifest.schema_version,
            artifact_format_version: manifest.format.version,
            browser_product: manifest.browser.product,
            browser_version: manifest.browser.version.clone(),
            browser_revision: manifest.browser.revision.clone(),
            protocol_version: manifest.browser.protocol_version.clone(),
            locale: manifest.environment.locale.clone(),
            timezone: manifest.environment.timezone.clone(),
            verification_level: manifest.verification.level,
            frames: manifest.frames,
            resources: manifest.resources,
            semantics,
            structural_repair_applied: manifest.structural_repair.applied,
            warning_codes: manifest.warning_codes.clone(),
            pdf_version,
        };
        metadata.validate(ErrorStage::Encoding)?;
        Ok(metadata)
    }

    pub(super) fn validate(&self, stage: ErrorStage) -> Result<()> {
        let required = [
            self.source.title.as_str(),
            self.source.language.as_str(),
            self.requested_url.as_str(),
            self.final_url.as_str(),
            self.captured_at.as_str(),
            self.generator_name.as_str(),
            self.generator_version.as_str(),
            self.browser_version.as_str(),
            self.protocol_version.as_str(),
            self.locale.as_str(),
            self.timezone.as_str(),
            self.pdf_version.as_str(),
        ];
        if required
            .iter()
            .any(|value| value.is_empty() || value.chars().count() > MAXIMUM_FIELD_CHARACTERS)
            || !language_tag_is_valid(&self.source.language)
            || self.captured_at_pdf().is_none()
            || !optional_field_is_valid(self.source.description.as_deref())
            || !optional_field_is_valid(self.source.rights.as_deref())
            || !optional_list_item_is_valid(self.source.site_name.as_deref())
            || !optional_list_item_is_valid(self.source.source_generator.as_deref())
            || !optional_list_item_is_valid(self.source.published_at.as_deref())
            || !optional_list_item_is_valid(self.source.modified_at.as_deref())
            || !optional_field_is_valid(self.browser_revision.as_deref())
            || !list_is_valid(&self.source.authors)
            || !list_is_valid(&self.source.keywords)
            || !list_is_valid(&self.warning_codes)
        {
            return Err(PageKnotError::new(
                "pageknot.export.pdf_metadata",
                stage,
                "PDF metadata contains an invalid required field",
            ));
        }
        Ok(())
    }

    pub(super) fn creator_tool(&self) -> String {
        format!(
            "{} {}",
            self.generator_name.trim(),
            self.generator_version.trim()
        )
    }

    pub(super) fn producer(&self) -> String {
        format!(
            "{} with {} {}",
            self.creator_tool(),
            browser_product(self.browser_product),
            self.browser_version.trim()
        )
    }

    pub(super) fn identifier(&self) -> String {
        format!(
            "urn:pageknot:source-artifact:sha256:{}",
            self.source_artifact_sha256
        )
    }

    pub(super) fn captured_at_pdf(&self) -> Option<String> {
        pdf_date(&self.captured_at)
    }

    pub(super) fn info_fields(&self) -> Vec<InfoField> {
        vec![
            InfoField {
                key: "Title",
                value: Some(self.source.title.clone()),
            },
            InfoField {
                key: "Author",
                value: joined(&self.source.authors),
            },
            InfoField {
                key: "Subject",
                value: self.source.description.clone(),
            },
            InfoField {
                key: "Keywords",
                value: joined(&self.source.keywords),
            },
            InfoField {
                key: "Creator",
                value: Some(self.creator_tool()),
            },
            InfoField {
                key: "Producer",
                value: Some(self.producer()),
            },
            InfoField {
                key: "CreationDate",
                value: self.captured_at_pdf(),
            },
            InfoField {
                key: "ModDate",
                value: self.captured_at_pdf(),
            },
            InfoField {
                key: "Source",
                value: Some(self.final_url.clone()),
            },
            InfoField {
                key: "PageKnotSourceArtifactSHA256",
                value: Some(self.source_artifact_sha256.to_hex()),
            },
        ]
    }
}

pub(super) const fn browser_product(product: BrowserProduct) -> &'static str {
    match product {
        BrowserProduct::Chrome => "chrome",
        BrowserProduct::Chromium => "chromium",
        BrowserProduct::Edge => "edge",
    }
}

pub(super) fn parse_browser_product(value: &str) -> Option<BrowserProduct> {
    match value {
        "chrome" => Some(BrowserProduct::Chrome),
        "chromium" => Some(BrowserProduct::Chromium),
        "edge" => Some(BrowserProduct::Edge),
        _ => None,
    }
}

pub(super) const fn verification_level(level: VerificationPolicy) -> &'static str {
    match level {
        VerificationPolicy::Static => "static",
        VerificationPolicy::Offline => "offline",
    }
}

pub(super) fn parse_verification_level(value: &str) -> Option<VerificationPolicy> {
    match value {
        "static" => Some(VerificationPolicy::Static),
        "offline" => Some(VerificationPolicy::Offline),
        _ => None,
    }
}

pub(super) const fn boolean_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

pub(super) fn parse_boolean(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn joined(values: &[String]) -> Option<String> {
    (!values.is_empty()).then(|| values.join(", "))
}

fn list_is_valid(values: &[String]) -> bool {
    values.len() <= MAXIMUM_LIST_ITEMS
        && values
            .iter()
            .all(|value| !value.is_empty() && value.chars().count() <= MAXIMUM_LIST_ITEM_CHARACTERS)
}

fn optional_field_is_valid(value: Option<&str>) -> bool {
    value.is_none_or(|value| !value.is_empty() && value.chars().count() <= MAXIMUM_FIELD_CHARACTERS)
}

fn optional_list_item_is_valid(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        !value.is_empty() && value.chars().count() <= MAXIMUM_LIST_ITEM_CHARACTERS
    })
}

fn pdf_date(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes.get(index).is_some_and(u8::is_ascii_digit) {
            return None;
        }
    }
    let suffix = value.get(19..)?;
    let offset = suffix.find(['Z', '+'])?;
    let fraction = suffix.get(..offset)?;
    let zone = suffix.get(offset..)?;
    if (!fraction.is_empty()
        && (!fraction.starts_with('.')
            || fraction.len() == 1
            || !fraction[1..].bytes().all(|byte| byte.is_ascii_digit())))
        || !matches!(zone, "Z" | "+00:00")
    {
        return None;
    }
    Some(format!(
        "D:{}{}{}{}{}{}Z",
        &value[0..4],
        &value[5..7],
        &value[8..10],
        &value[11..13],
        &value[14..16],
        &value[17..19],
    ))
}

#[cfg(test)]
mod tests {
    use super::pdf_date;

    #[test]
    fn pdf_date_accepts_utc_rfc3339_with_optional_fraction() {
        assert_eq!(
            pdf_date("2026-07-29T12:34:56+00:00").as_deref(),
            Some("D:20260729123456Z")
        );
        assert_eq!(
            pdf_date("2026-07-29T12:34:56.123Z").as_deref(),
            Some("D:20260729123456Z")
        );
    }
}
