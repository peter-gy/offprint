use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{CONTRACTS, schema_bytes};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractInventory {
    schema_version: u32,
    contracts: Vec<ContractRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractRecord {
    name: &'static str,
    schema: &'static str,
    sha256: String,
    fields: Vec<FieldRecord>,
    enums: Vec<EnumRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldRecord {
    path: String,
    name: String,
    required: bool,
    nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnumRecord {
    path: String,
    values: Vec<Value>,
}

pub(super) fn contract_inventory(
    documents: &BTreeMap<&'static str, Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let mut contracts = Vec::new();
    for &(schema_name, public_name) in CONTRACTS {
        let bytes = schema_bytes(documents, schema_name)?;
        let schema: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid generated schema {schema_name}: {error}"))?;
        let mut fields = Vec::new();
        let mut enums = Vec::new();
        collect_inventory(&schema, "#", &mut fields, &mut enums);
        fields.sort_by(|left, right| left.path.cmp(&right.path));
        enums.sort_by(|left, right| left.path.cmp(&right.path));
        contracts.push(ContractRecord {
            name: public_name,
            schema: schema_name,
            sha256: hex::encode(Sha256::digest(bytes)),
            fields,
            enums,
        });
    }
    pretty_json(&ContractInventory {
        schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
        contracts,
    })
}

pub(super) fn cli_json_contracts() -> Result<Vec<u8>, String> {
    pretty_json(&json!({
        "schemaVersion": pageknot_model::PUBLIC_SCHEMA_VERSION,
        "streams": {
            "stdout": "result",
            "stderr": "diagnostics"
        },
        "commands": [
            {
                "command": "capture",
                "successSchema": "capture-result.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "batch",
                "successSchema": "batch-result.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "crawl",
                "successSchema": "crawl-result.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "export",
                "successSchema": "artifact-export-result.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "verify",
                "successSchema": "verification-result.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "inspect",
                "successSchema": "artifact-manifest.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "doctor",
                "successSchema": "browser-doctor-report.schema.json",
                "errorSchema": "error.schema.json"
            },
            {
                "command": "browser",
                "successSchema": "browser-operation-result.schema.json",
                "errorSchema": "error.schema.json"
            }
        ]
    }))
}

fn collect_inventory(
    schema: &Value,
    path: &str,
    fields: &mut Vec<FieldRecord>,
    enums: &mut Vec<EnumRecord>,
) {
    let Some(object) = schema.as_object() else {
        return;
    };
    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        enums.push(EnumRecord {
            path: path.to_owned(),
            values: values.clone(),
        });
    }
    if let Some(value) = object.get("const") {
        enums.push(EnumRecord {
            path: path.to_owned(),
            values: vec![value.clone()],
        });
    }
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        let required = object
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        for (name, property) in properties {
            let property_path = format!("{path}/properties/{}", escape_pointer(name));
            fields.push(FieldRecord {
                path: property_path.clone(),
                name: name.clone(),
                required: required.contains(name.as_str()),
                nullable: is_nullable(property),
                default: property.get("default").cloned(),
            });
            collect_inventory(property, &property_path, fields, enums);
        }
    }
    for key in ["$defs", "oneOf", "anyOf", "allOf", "items"] {
        let Some(child) = object.get(key) else {
            continue;
        };
        match child {
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    collect_inventory(value, &format!("{path}/{key}/{index}"), fields, enums);
                }
            }
            Value::Object(values) if key == "$defs" => {
                for (name, value) in values {
                    collect_inventory(
                        value,
                        &format!("{path}/{key}/{}", escape_pointer(name)),
                        fields,
                        enums,
                    );
                }
            }
            _ => collect_inventory(child, &format!("{path}/{key}"), fields, enums),
        }
    }
}

fn is_nullable(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    if object.get("type").is_some_and(|kind| match kind {
        Value::String(kind) => kind == "null",
        Value::Array(kinds) => kinds.iter().any(|kind| kind == "null"),
        _ => false,
    }) {
        return true;
    }
    ["oneOf", "anyOf"].iter().any(|key| {
        object
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|variants| variants.iter().any(is_nullable))
    })
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
