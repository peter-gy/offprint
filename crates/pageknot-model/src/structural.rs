use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum RepairNode {
    Element {
        marker: String,
        namespace: String,
        name: String,
        #[serde(default)]
        children: Vec<Self>,
        #[serde(default)]
        template_content: Vec<Self>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shadow_mode: Option<String>,
    },
    Text {
        value: String,
    },
    Comment {
        value: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralRepairTree {
    pub document_element: RepairNode,
}
