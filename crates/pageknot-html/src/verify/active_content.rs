use markup5ever::ns;
use pageknot_document::{Document, NodeData, SafeStaticPolicy};
use pageknot_model::Result;

use crate::{REPAIR_SCRIPT_ELEMENT_ID, STATE_SCRIPT_ELEMENT_ID};

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
                "pageknot.verification.active_content",
                "artifact contains an executable script element",
            ));
        }
        if attrs.iter().any(SafeStaticPolicy::is_event_attribute) {
            return Err(verification_error(
                "pageknot.verification.active_content",
                "artifact contains an event handler attribute",
            ));
        }
        if attrs
            .iter()
            .any(SafeStaticPolicy::is_javascript_url_attribute)
        {
            return Err(verification_error(
                "pageknot.verification.active_content",
                "artifact contains a JavaScript URL",
            ));
        }
        if SafeStaticPolicy::is_base_element(name) {
            return Err(verification_error(
                "pageknot.verification.active_content",
                "artifact contains a base URL element",
            ));
        }
        if SafeStaticPolicy::is_meta_refresh(name, attrs) {
            return Err(verification_error(
                "pageknot.verification.active_content",
                "artifact contains a meta refresh",
            ));
        }
        if SafeStaticPolicy::is_request_triggering_link(name, attrs) {
            return Err(verification_error(
                "pageknot.verification.active_content",
                "artifact contains a request-triggering link element",
            ));
        }
    }
    Ok(())
}
