use markup5ever::ns;
use offprint_document::{Document, NodeData, SafeStaticPolicy};
use offprint_model::Result;

use crate::{
    CSP_ELEMENT_ID, MANIFEST_ELEMENT_ID, MANIFEST_MEDIA_TYPE, REPAIR_DATA_ELEMENT_ID,
    REPAIR_MEDIA_TYPE, REPAIR_SCRIPT_ELEMENT_ID, STATE_SCRIPT_ELEMENT_ID,
};

use super::{attribute_value, verification_error};

pub(super) fn validate_active_content(document: &Document) -> Result<()> {
    for id in document.walk() {
        let Some(NodeData::Element { name, attrs, .. }) = document.node(id).map(|node| &node.data)
        else {
            continue;
        };
        let owned_executable_script = attribute_value(attrs, "id")
            .is_some_and(|id| matches!(id, REPAIR_SCRIPT_ELEMENT_ID | STATE_SCRIPT_ELEMENT_ID));
        if SafeStaticPolicy::is_script_element(name)
            && (name.ns == ns!(svg)
                || (!SafeStaticPolicy::is_structured_metadata_script(attrs)
                    && !owned_executable_script))
        {
            return Err(verification_error(
                "offprint.verification.active_content",
                "artifact contains an executable script element",
            ));
        }
        if let Some(id) = attribute_value(attrs, "id")
            && matches!(
                id,
                CSP_ELEMENT_ID
                    | MANIFEST_ELEMENT_ID
                    | REPAIR_DATA_ELEMENT_ID
                    | REPAIR_SCRIPT_ELEMENT_ID
                    | STATE_SCRIPT_ELEMENT_ID
            )
        {
            let valid = match id {
                CSP_ELEMENT_ID => {
                    name.ns == ns!(html)
                        && name.local.as_ref() == "meta"
                        && attribute_value(attrs, "http-equiv").is_some_and(|value| {
                            value.eq_ignore_ascii_case("content-security-policy")
                        })
                }
                MANIFEST_ELEMENT_ID => {
                    name.ns == ns!(html)
                        && name.local.as_ref() == "script"
                        && attribute_value(attrs, "type") == Some(MANIFEST_MEDIA_TYPE)
                }
                REPAIR_DATA_ELEMENT_ID => {
                    name.ns == ns!(html)
                        && name.local.as_ref() == "script"
                        && attribute_value(attrs, "type") == Some(REPAIR_MEDIA_TYPE)
                }
                REPAIR_SCRIPT_ELEMENT_ID | STATE_SCRIPT_ELEMENT_ID => {
                    name.ns == ns!(html) && name.local.as_ref() == "script"
                }
                _ => false,
            };
            if !valid {
                return Err(verification_error(
                    "offprint.verification.active_content",
                    "artifact reserves its owned element identifiers",
                ));
            }
        }
        if attrs.iter().any(SafeStaticPolicy::is_event_attribute) {
            return Err(verification_error(
                "offprint.verification.active_content",
                "artifact contains an event handler attribute",
            ));
        }
        if attrs
            .iter()
            .any(SafeStaticPolicy::is_javascript_url_attribute)
        {
            return Err(verification_error(
                "offprint.verification.active_content",
                "artifact contains a JavaScript URL",
            ));
        }
        if SafeStaticPolicy::is_base_element(name) {
            return Err(verification_error(
                "offprint.verification.active_content",
                "artifact contains a base URL element",
            ));
        }
        if SafeStaticPolicy::is_meta_refresh(name, attrs) {
            return Err(verification_error(
                "offprint.verification.active_content",
                "artifact contains a meta refresh",
            ));
        }
        if SafeStaticPolicy::is_request_triggering_link(name, attrs) {
            return Err(verification_error(
                "offprint.verification.active_content",
                "artifact contains a request-triggering link element",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_unowned_content(document: &Document) -> Result<()> {
    validate_active_content(document)?;
    for id in document.walk() {
        let Some(NodeData::Element { attrs, .. }) = document.node(id).map(|node| &node.data) else {
            continue;
        };
        if attribute_value(attrs, "id").is_some_and(|id| {
            matches!(
                id,
                CSP_ELEMENT_ID
                    | MANIFEST_ELEMENT_ID
                    | REPAIR_DATA_ELEMENT_ID
                    | REPAIR_SCRIPT_ELEMENT_ID
                    | STATE_SCRIPT_ELEMENT_ID
            )
        }) {
            return Err(verification_error(
                "offprint.verification.active_content",
                "embedded content cannot define artifact-owned elements",
            ));
        }
    }
    Ok(())
}
