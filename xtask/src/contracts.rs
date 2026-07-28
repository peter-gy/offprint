use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

const CONTRACTS: &[(&str, &str)] = &[
    (
        "artifact-export-request.schema.json",
        "ArtifactExportRequest",
    ),
    ("artifact-export-result.schema.json", "ArtifactExportResult"),
    ("artifact-manifest.schema.json", "ArtifactManifest"),
    (
        "artifact-variant-verification.schema.json",
        "ArtifactVariantVerification",
    ),
    ("batch-request.schema.json", "BatchRequest"),
    ("batch-result.schema.json", "BatchResult"),
    ("browser-doctor-report.schema.json", "BrowserDoctorReport"),
    (
        "browser-operation-result.schema.json",
        "BrowserOperationResult",
    ),
    ("capture-event.schema.json", "CaptureEvent"),
    ("capture-policy.schema.json", "CapturePolicy"),
    ("capture-request.schema.json", "CaptureRequest"),
    ("capture-result.schema.json", "CaptureResult"),
    ("crawl-request.schema.json", "CrawlRequest"),
    ("crawl-result.schema.json", "CrawlResult"),
    ("error.schema.json", "PageKnotErrorRecord"),
    ("resume-manifest.schema.json", "ResumeManifest"),
    ("verification-result.schema.json", "VerificationResult"),
];

const PUBLIC_ALIASES: &[(&str, &str, &str)] = &[
    (
        "ArtifactVariant",
        "ArtifactExportRequest",
        "ArtifactVariant",
    ),
    (
        "ArtifactVariantKind",
        "ArtifactVariantVerification",
        "ArtifactVariantKind",
    ),
    ("BatchJob", "BatchRequest", "BatchJob"),
    ("BrowserInfo", "ArtifactManifest", "BrowserInfo"),
    ("CaptureLimits", "CaptureRequest", "CaptureLimits"),
    ("CaptureScope", "CapturePolicy", "CaptureScope"),
    ("ConflictPolicy", "ArtifactExportRequest", "ConflictPolicy"),
    ("CrawlPageOutcome", "CrawlResult", "CrawlPageOutcome"),
    ("ErrorStage", "PageKnotErrorRecord", "ErrorStage"),
    (
        "ExportedArtifact",
        "ArtifactExportResult",
        "ExportedArtifact",
    ),
    (
        "MarkdownOptions",
        "ArtifactExportRequest",
        "MarkdownOptions",
    ),
    ("PdfOptions", "ArtifactExportRequest", "PdfOptions"),
    ("ResumeOptions", "BatchRequest", "ResumeOptions"),
    (
        "ScheduledCaptureOutcome",
        "BatchResult",
        "ScheduledCaptureOutcome",
    ),
    (
        "VerificationPolicy",
        "VerificationResult",
        "VerificationPolicy",
    ),
    ("Viewport", "CaptureRequest", "Viewport"),
];

pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: Vec<u8>,
}

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

pub fn contract_inventory(documents: &BTreeMap<&'static str, Vec<u8>>) -> Result<Vec<u8>, String> {
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

pub fn cli_json_contracts() -> Result<Vec<u8>, String> {
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

pub fn binding_files(
    documents: &BTreeMap<&'static str, Vec<u8>>,
) -> Result<Vec<GeneratedFile>, String> {
    let schemas = parsed_contracts(documents)?;
    Ok(vec![
        GeneratedFile {
            path: PathBuf::from("bindings/node/contracts.generated.d.ts"),
            content: typescript_contracts(&schemas).into_bytes(),
        },
        GeneratedFile {
            path: PathBuf::from("bindings/python/python/pageknot/_contracts.pyi"),
            content: python_contracts(&schemas)?.into_bytes(),
        },
    ])
}

fn parsed_contracts(
    documents: &BTreeMap<&'static str, Vec<u8>>,
) -> Result<Vec<(&'static str, Value)>, String> {
    CONTRACTS
        .iter()
        .map(|&(schema_name, public_name)| {
            let schema = serde_json::from_slice(schema_bytes(documents, schema_name)?)
                .map_err(|error| format!("invalid generated schema {schema_name}: {error}"))?;
            Ok((public_name, schema))
        })
        .collect()
}

fn schema_bytes<'a>(
    documents: &'a BTreeMap<&'static str, Vec<u8>>,
    name: &str,
) -> Result<&'a [u8], String> {
    documents
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing generated schema {name}"))
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

fn typescript_contracts(contracts: &[(&'static str, Value)]) -> String {
    let mut output =
        String::from("// Generated by `cargo run -p xtask -- codegen`. Do not edit.\n\n");
    for &(root_name, ref schema) in contracts {
        let context = SchemaContext::new(root_name, schema);
        for (definition, value) in &context.definitions {
            output.push_str("type ");
            output.push_str(&context.definition_name(definition));
            output.push_str(" = ");
            output.push_str(&typescript_type(value, &context, 0));
            output.push_str(";\n\n");
        }
        output.push_str("export type ");
        output.push_str(root_name);
        output.push_str(" = ");
        output.push_str(&typescript_type(schema, &context, 0));
        output.push_str(";\n\n");
    }
    for &(alias, root, definition) in PUBLIC_ALIASES {
        output.push_str("export type ");
        output.push_str(alias);
        output.push_str(" = ");
        output.push_str(root);
        output.push_str(definition);
        output.push_str(";\n");
    }
    output
}

struct SchemaContext<'a> {
    root_name: &'static str,
    definitions: BTreeMap<String, &'a Value>,
}

impl<'a> SchemaContext<'a> {
    fn new(root_name: &'static str, schema: &'a Value) -> Self {
        let definitions = schema
            .get("$defs")
            .and_then(Value::as_object)
            .map(|values| {
                values
                    .iter()
                    .map(|(name, value)| (name.clone(), value))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            root_name,
            definitions,
        }
    }

    fn definition_name(&self, definition: &str) -> String {
        format!("{}{}", self.root_name, definition)
    }

    fn reference_name(&self, reference: &str) -> Option<String> {
        if reference == "#" {
            return Some(self.root_name.to_owned());
        }
        reference
            .strip_prefix("#/$defs/")
            .map(|definition| self.definition_name(definition))
    }
}

fn typescript_type(schema: &Value, context: &SchemaContext<'_>, depth: usize) -> String {
    let Some(object) = schema.as_object() else {
        return "unknown".to_owned();
    };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str)
        && let Some(name) = context.reference_name(reference)
    {
        return name;
    }
    if let Some(value) = object.get("const") {
        return json_literal(value);
    }
    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        return join_types(values.iter().map(json_literal), " | ");
    }
    for (key, separator) in [("oneOf", " | "), ("anyOf", " | "), ("allOf", " & ")] {
        if let Some(values) = object.get(key).and_then(Value::as_array) {
            return join_types(
                values
                    .iter()
                    .map(|value| typescript_type(value, context, depth)),
                separator,
            );
        }
    }
    if let Some(kinds) = object.get("type").and_then(Value::as_array) {
        return join_types(
            kinds
                .iter()
                .map(|kind| typescript_primitive(kind, object, context, depth)),
            " | ",
        );
    }
    if let Some(kind) = object.get("type") {
        return typescript_primitive(kind, object, context, depth);
    }
    if object.contains_key("properties") {
        return typescript_object(object, context, depth);
    }
    "unknown".to_owned()
}

fn typescript_primitive(
    kind: &Value,
    schema: &Map<String, Value>,
    context: &SchemaContext<'_>,
    depth: usize,
) -> String {
    match kind.as_str() {
        Some("null") => "null".to_owned(),
        Some("boolean") => "boolean".to_owned(),
        Some("integer" | "number") => "number".to_owned(),
        Some("string") => "string".to_owned(),
        Some("array") => {
            let item = schema.get("items").map_or_else(
                || "unknown".to_owned(),
                |value| typescript_type(value, context, depth),
            );
            format!("Array<{item}>")
        }
        Some("object") => typescript_object(schema, context, depth),
        _ => "unknown".to_owned(),
    }
}

fn typescript_object(
    schema: &Map<String, Value>,
    context: &SchemaContext<'_>,
    depth: usize,
) -> String {
    let properties = schema.get("properties").and_then(Value::as_object);
    let additional = schema.get("additionalProperties");
    let Some(properties) = properties else {
        return match additional {
            Some(Value::Object(value)) => format!(
                "Record<string, {}>",
                typescript_type(&Value::Object(value.clone()), context, depth)
            ),
            Some(Value::Bool(false)) => "Record<string, never>".to_owned(),
            _ => "Record<string, unknown>".to_owned(),
        };
    };
    if properties.is_empty() {
        return match additional {
            Some(Value::Object(value)) => format!(
                "Record<string, {}>",
                typescript_type(&Value::Object(value.clone()), context, depth)
            ),
            Some(Value::Bool(false)) => "Record<string, never>".to_owned(),
            _ => "Record<string, unknown>".to_owned(),
        };
    }
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);
    let mut output = String::from("{\n");
    for (name, property) in properties {
        output.push_str(&child_indent);
        output.push_str(&typescript_property_name(name));
        if !required.contains(name.as_str()) {
            output.push('?');
        }
        output.push_str(": ");
        output.push_str(&typescript_type(property, context, depth + 1));
        output.push_str(";\n");
    }
    if additional.is_some_and(|value| value != &Value::Bool(false)) {
        output.push_str(&child_indent);
        output.push_str("[key: string]: unknown;\n");
    }
    output.push_str(&indent);
    output.push('}');
    output
}

fn typescript_property_name(name: &str) -> String {
    if is_identifier(name) {
        name.to_owned()
    } else {
        serde_json::to_string(name).unwrap_or_else(|_| "\"field\"".to_owned())
    }
}

fn join_types(types: impl Iterator<Item = String>, separator: &str) -> String {
    let types = types.collect::<Vec<_>>();
    match types.as_slice() {
        [] => "never".to_owned(),
        [single] => single.clone(),
        _ => format!("({})", types.join(separator)),
    }
}

fn json_literal(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "unknown".to_owned())
}

#[derive(Clone)]
struct PythonType {
    name: String,
    schema: Value,
}

fn python_contracts(contracts: &[(&'static str, Value)]) -> Result<String, String> {
    let mut output = String::from(
        "# Generated by `cargo run -p xtask -- codegen`. Do not edit.\n\n\
         from typing import Literal, TypeAlias, TypedDict\n\n",
    );
    for &(root_name, ref schema) in contracts {
        let context = PythonContext::new(root_name, schema)?;
        for definition in &context.types {
            output.push_str(&python_definition(definition, &context)?);
            output.push('\n');
        }
    }
    for &(alias, root, definition) in PUBLIC_ALIASES {
        output.push_str(alias);
        output.push_str(": TypeAlias = ");
        output.push_str(root);
        output.push_str(definition);
        output.push('\n');
    }
    Ok(output)
}

struct PythonContext {
    root_name: &'static str,
    types: Vec<PythonType>,
    references: BTreeMap<String, String>,
    anonymous: BTreeMap<String, String>,
}

impl PythonContext {
    fn new(root_name: &'static str, schema: &Value) -> Result<Self, String> {
        let mut context = Self {
            root_name,
            types: Vec::new(),
            references: BTreeMap::from([("#".to_owned(), root_name.to_owned())]),
            anonymous: BTreeMap::new(),
        };
        if let Some(definitions) = schema.get("$defs").and_then(Value::as_object) {
            for name in definitions.keys() {
                context
                    .references
                    .insert(format!("#/$defs/{name}"), format!("{root_name}{name}"));
            }
            for (name, value) in definitions {
                context.register_named(format!("{root_name}{name}"), value.clone())?;
            }
        }
        context.register_named(root_name.to_owned(), schema.clone())?;
        Ok(context)
    }

    fn register_named(&mut self, name: String, schema: Value) -> Result<(), String> {
        if self.types.iter().any(|existing| existing.name == name) {
            return Ok(());
        }
        self.collect_anonymous(&name, &schema)?;
        self.types.push(PythonType { name, schema });
        Ok(())
    }

    fn collect_anonymous(&mut self, owner: &str, schema: &Value) -> Result<(), String> {
        let Some(object) = schema.as_object() else {
            return Ok(());
        };
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(variants) = object.get(key).and_then(Value::as_array) {
                for (index, variant) in variants.iter().enumerate() {
                    self.register_anonymous(
                        format!("{owner}{}{}", pascal_case(key), index + 1),
                        variant,
                    )?;
                }
            }
        }
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            for (name, property) in properties {
                self.register_anonymous(format!("{owner}{}", pascal_case(name)), property)?;
            }
        }
        if let Some(items) = object.get("items") {
            self.register_anonymous(format!("{owner}Item"), items)?;
        }
        Ok(())
    }

    fn register_anonymous(&mut self, name: String, schema: &Value) -> Result<(), String> {
        if is_inline_object(schema) {
            let key = schema_pointer_key(schema);
            self.anonymous.insert(key, name.clone());
            self.register_named(name, schema.clone())?;
        } else {
            self.collect_anonymous(&name, schema)?;
        }
        Ok(())
    }

    fn reference_name(&self, reference: &str) -> Result<String, String> {
        self.references.get(reference).cloned().ok_or_else(|| {
            format!(
                "unsupported schema reference {reference} in {}",
                self.root_name
            )
        })
    }

    fn anonymous_name(&self, schema: &Value) -> Option<&str> {
        self.anonymous
            .get(&schema_pointer_key(schema))
            .map(String::as_str)
    }
}

fn python_definition(definition: &PythonType, context: &PythonContext) -> Result<String, String> {
    if is_object_schema(&definition.schema) {
        return python_typed_dict(definition, context);
    }
    Ok(format!(
        "{}: TypeAlias = {}\n",
        definition.name,
        python_type(&definition.schema, context, Some(&definition.name))?
    ))
}

fn python_typed_dict(definition: &PythonType, context: &PythonContext) -> Result<String, String> {
    let object = definition
        .schema
        .as_object()
        .ok_or_else(|| format!("{} is not an object schema", definition.name))?;
    let Some(properties) = object.get("properties").and_then(Value::as_object) else {
        return Ok(format!(
            "{}: TypeAlias = {}\n",
            definition.name,
            python_object_type(object, context)?
        ));
    };
    if properties.is_empty() {
        return Ok(format!(
            "{}: TypeAlias = {}\n",
            definition.name,
            python_object_type(object, context)?
        ));
    }
    let required = object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let required_fields = properties
        .iter()
        .filter(|(name, _)| required.contains(name.as_str()))
        .collect::<Vec<_>>();
    let optional_fields = properties
        .iter()
        .filter(|(name, _)| !required.contains(name.as_str()))
        .collect::<Vec<_>>();
    let all_identifiers = properties.keys().all(|name| is_python_identifier(name));
    if !all_identifiers {
        if !required_fields.is_empty() && !optional_fields.is_empty() {
            return Err(format!(
                "{} mixes required and optional fields with a Python keyword",
                definition.name
            ));
        }
        let fields = properties
            .iter()
            .map(|(name, schema)| {
                Ok(format!(
                    "{}: {}",
                    serde_json::to_string(name).map_err(|error| error.to_string())?,
                    python_type(schema, context, None)?
                ))
            })
            .collect::<Result<Vec<_>, String>>()?
            .join(", ");
        let total = if optional_fields.is_empty() {
            ""
        } else {
            ", total=False"
        };
        return Ok(format!(
            "{} = TypedDict({}, {{{fields}}}{total})\n",
            definition.name,
            serde_json::to_string(&definition.name).map_err(|error| error.to_string())?
        ));
    }
    let mut output = String::new();
    if !optional_fields.is_empty() {
        output.push_str("class _");
        output.push_str(&definition.name);
        output.push_str("Optional(TypedDict, total=False):\n");
        for (name, schema) in &optional_fields {
            output.push_str("    ");
            output.push_str(name);
            output.push_str(": ");
            output.push_str(&python_type(schema, context, None)?);
            output.push('\n');
        }
        output.push('\n');
    }
    output.push_str("class ");
    output.push_str(&definition.name);
    if optional_fields.is_empty() {
        output.push_str("(TypedDict):\n");
    } else {
        output.push_str("(_");
        output.push_str(&definition.name);
        output.push_str("Optional):\n");
    }
    if required_fields.is_empty() {
        output.push_str("    pass\n");
    } else {
        for (name, schema) in required_fields {
            output.push_str("    ");
            output.push_str(name);
            output.push_str(": ");
            output.push_str(&python_type(schema, context, None)?);
            output.push('\n');
        }
    }
    Ok(output)
}

fn python_type(
    schema: &Value,
    context: &PythonContext,
    current: Option<&str>,
) -> Result<String, String> {
    let Some(object) = schema.as_object() else {
        return Ok("object".to_owned());
    };
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        return context.reference_name(reference);
    }
    if let Some(value) = object.get("const") {
        return Ok(format!("Literal[{}]", python_literal(value)));
    }
    if let Some(values) = object.get("enum").and_then(Value::as_array) {
        return Ok(format!(
            "Literal[{}]",
            values
                .iter()
                .map(python_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(values) = object.get(key).and_then(Value::as_array) {
            let variants = values
                .iter()
                .map(|value| {
                    if let Some(name) = context.anonymous_name(value) {
                        Ok(name.to_owned())
                    } else {
                        python_type(value, context, current)
                    }
                })
                .collect::<Result<Vec<_>, String>>()?;
            return Ok(match variants.as_slice() {
                [] => "object".to_owned(),
                [single] => single.clone(),
                _ => variants.join(" | "),
            });
        }
    }
    if let Some(values) = object.get("allOf").and_then(Value::as_array)
        && let Some(single) = values.first()
    {
        return python_type(single, context, current);
    }
    if is_inline_object(schema)
        && let Some(name) = context.anonymous_name(schema)
        && Some(name) != current
    {
        return Ok(name.to_owned());
    }
    if let Some(kinds) = object.get("type").and_then(Value::as_array) {
        let variants = kinds
            .iter()
            .map(|kind| python_primitive(kind, object, context))
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(variants.join(" | "));
    }
    if let Some(kind) = object.get("type") {
        return python_primitive(kind, object, context);
    }
    if object.contains_key("properties") {
        return context
            .anonymous_name(schema)
            .map(str::to_owned)
            .or_else(|| current.map(str::to_owned))
            .ok_or_else(|| format!("unnamed object schema in {}", context.root_name));
    }
    Ok("object".to_owned())
}

fn python_primitive(
    kind: &Value,
    schema: &Map<String, Value>,
    context: &PythonContext,
) -> Result<String, String> {
    Ok(match kind.as_str() {
        Some("null") => "None".to_owned(),
        Some("boolean") => "bool".to_owned(),
        Some("integer") => "int".to_owned(),
        Some("number") => "float".to_owned(),
        Some("string") => "str".to_owned(),
        Some("array") => {
            let item = schema.get("items").map_or_else(
                || Ok("object".to_owned()),
                |value| python_type(value, context, None),
            )?;
            format!("list[{item}]")
        }
        Some("object") => python_object_type(schema, context)?,
        _ => "object".to_owned(),
    })
}

fn python_object_type(
    schema: &Map<String, Value>,
    context: &PythonContext,
) -> Result<String, String> {
    match schema.get("additionalProperties") {
        Some(Value::Object(value)) => Ok(format!(
            "dict[str, {}]",
            python_type(&Value::Object(value.clone()), context, None)?
        )),
        _ => Ok("dict[str, object]".to_owned()),
    }
}

fn python_literal(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(value) => format!("{value:?}"),
        _ => value.to_string(),
    }
}

fn is_object_schema(schema: &Value) -> bool {
    schema.as_object().is_some_and(|object| {
        object.contains_key("properties")
            || object.get("type").and_then(Value::as_str) == Some("object")
    })
}

fn is_inline_object(schema: &Value) -> bool {
    is_object_schema(schema) && schema.get("$ref").is_none()
}

fn schema_pointer_key(schema: &Value) -> String {
    serde_json::to_string(schema).unwrap_or_else(|_| "schema".to_owned())
}

fn pascal_case(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if !character.is_ascii_alphanumeric() {
            uppercase = true;
            continue;
        }
        if uppercase {
            result.push(character.to_ascii_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|first| {
        (first.is_ascii_alphabetic() || first == '_')
            && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

fn is_python_identifier(value: &str) -> bool {
    is_identifier(value)
        && !matches!(
            value,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}
