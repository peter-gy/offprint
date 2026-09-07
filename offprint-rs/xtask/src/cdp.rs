// The schema mapping follows agent-browser cli/build.rs at revision
// 3cc7022271235694b5b5ce8aaea8bbfaa66e8cd5. Offprint adds upstream pin
// verification, domain selection, command markers, and checked-in output.

mod selection;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

const MAXIMUM_PROTOCOL_BYTES: u64 = 16 * 1024 * 1024;
const IMPORTED_DOMAINS: [&str; 2] = ["Browser", "Target"];
const MAXIMUM_GENERATED_LINES: usize = 990;
const GENERATED_DIRECTORY: &str = "offprint-rs/chromium/src/cdp/generated";
const LEGACY_GENERATED_FILE: &str = "offprint-rs/chromium/src/cdp_generated.rs";

#[derive(Debug, Deserialize)]
struct Versions {
    chromium: ChromiumVersions,
}

#[derive(Debug, Deserialize)]
struct ChromiumVersions {
    cdp_revision: String,
    cdp_chromium_revision: String,
    browser_protocol_sha256: String,
    js_protocol_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ProtocolSpec {
    domains: Vec<Domain>,
}

#[derive(Clone, Debug, Deserialize)]
struct Domain {
    domain: String,
    #[serde(default)]
    types: Vec<TypeDef>,
    #[serde(default)]
    commands: Vec<Command>,
    #[serde(default)]
    events: Vec<Event>,
}

#[derive(Clone, Debug, Deserialize)]
struct TypeDef {
    id: String,
    #[serde(rename = "type", default)]
    type_kind: String,
    #[serde(rename = "$ref", default)]
    ref_type: Option<String>,
    #[serde(default)]
    properties: Vec<Property>,
    #[serde(rename = "enum", default)]
    enum_values: Vec<String>,
    #[serde(default)]
    items: Option<Box<ItemType>>,
}

#[derive(Clone, Debug, Deserialize)]
struct Command {
    name: String,
    #[serde(default)]
    parameters: Vec<Property>,
    #[serde(default)]
    returns: Vec<Property>,
}

#[derive(Clone, Debug, Deserialize)]
struct Event {
    name: String,
    #[serde(default)]
    parameters: Vec<Property>,
}

#[derive(Clone, Debug, Deserialize)]
struct Property {
    name: String,
    #[serde(rename = "type", default)]
    type_kind: Option<String>,
    #[serde(rename = "$ref", default)]
    ref_type: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    items: Option<Box<ItemType>>,
}

#[derive(Clone, Debug, Deserialize)]
struct ItemType {
    #[serde(rename = "type", default)]
    type_kind: Option<String>,
    #[serde(rename = "$ref", default)]
    ref_type: Option<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum SelectedItem {
    Type(String),
    Command(String),
    Event(String),
}

pub(crate) fn generate(root: &Path, check: bool) -> Result<(), String> {
    let versions = read_versions(root)?;
    let browser = fetch_protocol(
        &versions.cdp_revision,
        "browser_protocol.json",
        &versions.browser_protocol_sha256,
    )?;
    let javascript = fetch_protocol(
        &versions.cdp_revision,
        "js_protocol.json",
        &versions.js_protocol_sha256,
    )?;
    let mut domains = parse_protocol(&browser, "browser_protocol.json")?.domains;
    domains.extend(parse_protocol(&javascript, "js_protocol.json")?.domains);
    validate_domains(&domains)?;
    let imports = selection::imported_protocol_items(&root.join("offprint-rs/chromium/src"))?;
    let selected = selection::select_protocol_items(&domains, &imports)?;
    let generated = format_generated(&versions, &domains, &selected)?;
    update_generated_files(root, &generated, check)
}

fn read_versions(root: &Path) -> Result<ChromiumVersions, String> {
    let path = root.join("versions.toml");
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    toml::from_str::<Versions>(&content)
        .map(|versions| versions.chromium)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn fetch_protocol(revision: &str, name: &str, expected_sha256: &str) -> Result<Vec<u8>, String> {
    let url = format!(
        "https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/{revision}/json/{name}"
    );
    let response = reqwest::blocking::get(&url)
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("failed to download pinned CDP input `{name}`: {error}"))?;
    if response
        .content_length()
        .is_some_and(|bytes| bytes > MAXIMUM_PROTOCOL_BYTES)
    {
        return Err(format!("pinned CDP input `{name}` exceeds the byte limit"));
    }
    let mut bytes = Vec::new();
    response
        .take(MAXIMUM_PROTOCOL_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read pinned CDP input `{name}`: {error}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_PROTOCOL_BYTES {
        return Err(format!("pinned CDP input `{name}` exceeds the byte limit"));
    }
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected_sha256 {
        return Err(format!(
            "pinned CDP input `{name}` has sha256 {actual}, expected {expected_sha256}"
        ));
    }
    Ok(bytes)
}

fn parse_protocol(bytes: &[u8], name: &str) -> Result<ProtocolSpec, String> {
    serde_json::from_slice(bytes)
        .map_err(|error| format!("failed to parse pinned CDP input `{name}`: {error}"))
}

fn validate_domains(domains: &[Domain]) -> Result<(), String> {
    let available = domains
        .iter()
        .map(|domain| domain.domain.as_str())
        .collect::<BTreeSet<_>>();
    let missing = IMPORTED_DOMAINS
        .iter()
        .copied()
        .filter(|domain| !available.contains(domain))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "pinned CDP inputs omit required domains: {}",
            missing.join(", ")
        ))
    }
}

fn format_generated(
    versions: &ChromiumVersions,
    domains: &[Domain],
    selected: &BTreeMap<String, BTreeSet<SelectedItem>>,
) -> Result<BTreeMap<PathBuf, String>, String> {
    let selected_domains = domains
        .iter()
        .filter_map(|domain| {
            let items = selected.get(&domain.domain)?;
            Some(Domain {
                domain: domain.domain.clone(),
                types: domain
                    .types
                    .iter()
                    .filter(|type_def| items.contains(&SelectedItem::Type(type_def.id.clone())))
                    .cloned()
                    .collect(),
                commands: domain
                    .commands
                    .iter()
                    .filter(|command| items.contains(&SelectedItem::Command(command.name.clone())))
                    .cloned()
                    .collect(),
                events: domain
                    .events
                    .iter()
                    .filter(|event| items.contains(&SelectedItem::Event(event.name.clone())))
                    .cloned()
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    let domain_types = domains
        .iter()
        .map(|domain| {
            (
                domain.domain.clone(),
                domain
                    .types
                    .iter()
                    .map(|type_def| type_def.id.clone())
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let recursive_fields = [
        ("DOM", "Node", "contentDocument"),
        ("DOM", "Node", "templateContent"),
        ("DOM", "Node", "importedDocument"),
        ("Runtime", "StackTrace", "parent"),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let header = format!(
        "// @generated by `cargo xtask codegen-cdp` from devtools-protocol {}.\n\
         // Chromium protocol revision r{}. Browser SHA-256 {}. JavaScript SHA-256 {}.\n",
        versions.cdp_revision,
        versions.cdp_chromium_revision,
        versions.browser_protocol_sha256,
        versions.js_protocol_sha256
    );
    let mut files = BTreeMap::new();
    let mut module = header.clone();
    line(
        &mut module,
        format_args!("use serde::{{Deserialize, Serialize}};"),
    );
    module.push('\n');
    for domain in &selected_domains {
        let file_name = format!("cdp_{}.rs", to_snake_case(&domain.domain));
        line(
            &mut module,
            format_args!("include!({});", rust_string(&file_name)),
        );
        let mut output = header.clone();
        generate_domain(domain, &domain_types, &recursive_fields, &mut output)?;
        files.insert(PathBuf::from(file_name), rustfmt(&output)?);
    }
    files.insert(PathBuf::from("mod.rs"), rustfmt(&module)?);
    for (path, content) in &files {
        let lines = content.lines().count();
        if lines > MAXIMUM_GENERATED_LINES {
            return Err(format!(
                "generated CDP source {} has {lines} lines, maximum is {MAXIMUM_GENERATED_LINES}",
                path.display()
            ));
        }
    }
    Ok(files)
}

fn generate_domain(
    domain: &Domain,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
    recursive_fields: &BTreeSet<(&str, &str, &str)>,
    output: &mut String,
) -> Result<(), String> {
    line(
        output,
        format_args!(
            "#[allow(dead_code, unused_imports, non_snake_case, non_camel_case_types, clippy::enum_variant_names, clippy::too_many_arguments, clippy::upper_case_acronyms)]"
        ),
    );
    line(
        output,
        format_args!("pub mod cdp_{} {{", to_snake_case(&domain.domain)),
    );
    line(output, format_args!("    use super::*;"));
    output.push('\n');

    for type_def in &domain.types {
        generate_type(domain, type_def, domain_types, recursive_fields, output)?;
    }
    for command in &domain.commands {
        generate_command(domain, command, domain_types, output);
    }
    for event in &domain.events {
        generate_event(domain, event, domain_types, output);
    }
    line(output, format_args!("}}"));
    output.push('\n');
    Ok(())
}

fn generate_type(
    domain: &Domain,
    type_def: &TypeDef,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
    recursive_fields: &BTreeSet<(&str, &str, &str)>,
    output: &mut String,
) -> Result<(), String> {
    if !type_def.enum_values.is_empty() {
        line(
            output,
            format_args!("    #[derive(Debug, Clone, Serialize, Deserialize)]"),
        );
        line(output, format_args!("    pub enum {} {{", type_def.id));
        let mut seen = BTreeSet::new();
        for value in &type_def.enum_values {
            let mut variant = to_pascal_case(value);
            if variant == "Self" {
                variant = "SelfValue".to_owned();
            }
            if variant
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
            {
                variant.insert(0, 'V');
            }
            if variant.is_empty() {
                variant = "Value".to_owned();
            }
            if seen.insert(variant.clone()) {
                line(
                    output,
                    format_args!("        #[serde(rename = {})]", rust_string(value)),
                );
                line(output, format_args!("        {variant},"));
            }
        }
        line(output, format_args!("    }}"));
        output.push('\n');
        return Ok(());
    }
    if type_def.type_kind == "object" && !type_def.properties.is_empty() {
        line(
            output,
            format_args!("    #[derive(Debug, Clone, Serialize, Deserialize)]"),
        );
        line(
            output,
            format_args!("    #[serde(rename_all = \"camelCase\")]"),
        );
        line(output, format_args!("    pub struct {} {{", type_def.id));
        for property in &type_def.properties {
            generate_field(
                &domain.domain,
                Some(&type_def.id),
                property,
                domain_types,
                recursive_fields,
                8,
                output,
            );
        }
        line(output, format_args!("    }}"));
        output.push('\n');
        return Ok(());
    }
    let alias = alias_type(type_def, &domain.domain, domain_types);
    line(
        output,
        format_args!("    pub type {} = {alias};", type_def.id),
    );
    output.push('\n');
    Ok(())
}

fn generate_command(
    domain: &Domain,
    command: &Command,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
    output: &mut String,
) {
    let name = to_pascal_case(&command.name);
    generate_record(
        &domain.domain,
        &format!("{name}Params"),
        &command.parameters,
        domain_types,
        output,
    );
    generate_record(
        &domain.domain,
        &format!("{name}Result"),
        &command.returns,
        domain_types,
        output,
    );
    line(output, format_args!("    #[derive(Debug, Clone, Copy)]"));
    line(output, format_args!("    pub struct {name}Command;"));
    line(
        output,
        format_args!("    impl crate::cdp::CdpCommand for {name}Command {{"),
    );
    line(output, format_args!("        type Params = {name}Params;"));
    line(
        output,
        format_args!("        type Response = {name}Result;"),
    );
    line(
        output,
        format_args!(
            "        const METHOD: &'static str = {};",
            rust_string(&format!("{}.{}", domain.domain, command.name))
        ),
    );
    line(output, format_args!("    }}"));
    output.push('\n');
}

fn generate_event(
    domain: &Domain,
    event: &Event,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
    output: &mut String,
) {
    let name = format!("{}Event", to_pascal_case(&event.name));
    generate_record(
        &domain.domain,
        &name,
        &event.parameters,
        domain_types,
        output,
    );
    line(
        output,
        format_args!("    impl crate::cdp::CdpEventMessage for {name} {{"),
    );
    line(
        output,
        format_args!(
            "        const METHOD: &'static str = {};",
            rust_string(&format!("{}.{}", domain.domain, event.name))
        ),
    );
    line(output, format_args!("    }}"));
    output.push('\n');
}

fn generate_record(
    domain: &str,
    name: &str,
    properties: &[Property],
    domain_types: &BTreeMap<String, BTreeSet<String>>,
    output: &mut String,
) {
    line(
        output,
        format_args!("    #[derive(Debug, Clone, Serialize, Deserialize)]"),
    );
    line(
        output,
        format_args!("    #[serde(rename_all = \"camelCase\")]"),
    );
    line(output, format_args!("    pub struct {name} {{"));
    for property in properties {
        generate_field(
            domain,
            None,
            property,
            domain_types,
            &BTreeSet::new(),
            8,
            output,
        );
    }
    line(output, format_args!("    }}"));
    let required = properties
        .iter()
        .filter(|property| !property.optional)
        .collect::<Vec<_>>();
    line(output, format_args!("    impl {name} {{"));
    if required.is_empty() {
        line(
            output,
            format_args!("        pub const fn new() -> Self {{"),
        );
        line(output, format_args!("            Self {{"));
    } else {
        let parameters = required
            .iter()
            .map(|property| {
                format!(
                    "{}: {}",
                    field_name(&property.name),
                    map_property_type(property, domain, domain_types)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        line(
            output,
            format_args!("        pub fn new({parameters}) -> Self {{"),
        );
        line(output, format_args!("            Self {{"));
    }
    for property in properties {
        let field = field_name(&property.name);
        if property.optional {
            line(output, format_args!("                {field}: None,"));
        } else {
            line(output, format_args!("                {field},"));
        }
    }
    line(output, format_args!("            }}"));
    line(output, format_args!("        }}"));
    line(output, format_args!("    }}"));
    output.push('\n');
}

fn generate_field(
    domain: &str,
    owner: Option<&str>,
    property: &Property,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
    recursive_fields: &BTreeSet<(&str, &str, &str)>,
    indent: usize,
    output: &mut String,
) {
    let padding = " ".repeat(indent);
    if property.optional {
        line(
            output,
            format_args!("{padding}#[serde(skip_serializing_if = \"Option::is_none\")]"),
        );
    }
    let mut rust_type = map_property_type(property, domain, domain_types);
    if owner
        .is_some_and(|owner| recursive_fields.contains(&(domain, owner, property.name.as_str())))
    {
        rust_type = if let Some(inner) = rust_type
            .strip_prefix("Option<")
            .and_then(|value| value.strip_suffix('>'))
        {
            format!("Option<Box<{inner}>>")
        } else {
            format!("Box<{rust_type}>")
        };
    }
    line(
        output,
        format_args!("{padding}pub {}: {rust_type},", field_name(&property.name)),
    );
}

fn alias_type(
    type_def: &TypeDef,
    domain: &str,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    if let Some(reference) = &type_def.ref_type {
        return resolve_ref(reference, domain, domain_types);
    }
    match type_def.type_kind.as_str() {
        "string" | "binary" => "String".to_owned(),
        "integer" => "i64".to_owned(),
        "number" => "f64".to_owned(),
        "boolean" => "bool".to_owned(),
        "array" => type_def.items.as_deref().map_or_else(
            || "Vec<serde_json::Value>".to_owned(),
            |item| format!("Vec<{}>", map_item_type(item, domain, domain_types)),
        ),
        _ => "serde_json::Value".to_owned(),
    }
}

fn map_property_type(
    property: &Property,
    domain: &str,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    let base = if let Some(reference) = &property.ref_type {
        resolve_ref(reference, domain, domain_types)
    } else {
        match property.type_kind.as_deref().unwrap_or("any") {
            "string" | "binary" => "String".to_owned(),
            "integer" => "i64".to_owned(),
            "number" => "f64".to_owned(),
            "boolean" => "bool".to_owned(),
            "array" => property.items.as_deref().map_or_else(
                || "Vec<serde_json::Value>".to_owned(),
                |item| format!("Vec<{}>", map_item_type(item, domain, domain_types)),
            ),
            _ => "serde_json::Value".to_owned(),
        }
    };
    if property.optional {
        format!("Option<{base}>")
    } else {
        base
    }
}

fn map_item_type(
    item: &ItemType,
    domain: &str,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    if let Some(reference) = &item.ref_type {
        return resolve_ref(reference, domain, domain_types);
    }
    match item.type_kind.as_deref().unwrap_or("any") {
        "string" | "binary" => "String".to_owned(),
        "integer" => "i64".to_owned(),
        "number" => "f64".to_owned(),
        "boolean" => "bool".to_owned(),
        _ => "serde_json::Value".to_owned(),
    }
}

fn resolve_ref(
    reference: &str,
    domain: &str,
    domain_types: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    let mut parts = reference.split('.');
    let Some(first) = parts.next() else {
        return "serde_json::Value".to_owned();
    };
    let second = parts.next();
    if let Some(type_name) = second {
        if first == domain {
            return to_pascal_case(type_name);
        }
        if domain_types
            .get(first)
            .is_some_and(|types| types.contains(type_name))
        {
            return format!(
                "super::cdp_{}::{}",
                to_snake_case(first),
                to_pascal_case(type_name)
            );
        }
        return "serde_json::Value".to_owned();
    }
    to_pascal_case(first)
}

fn field_name(value: &str) -> String {
    let name = to_snake_case(value);
    if is_rust_keyword(&name) {
        format!("r#{name}")
    } else {
        name
    }
}

fn to_pascal_case(value: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for character in value.chars() {
        if matches!(character, '_' | '-' | '.') {
            capitalize = true;
        } else if capitalize {
            result.push(character.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(character);
        }
    }
    result
}

fn to_snake_case(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    let mut result = String::new();
    for (index, character) in characters.iter().copied().enumerate() {
        if character.is_uppercase() && index > 0 {
            let previous_is_upper = characters[index - 1].is_uppercase();
            let next_is_lower = characters
                .get(index + 1)
                .is_some_and(|next| next.is_lowercase());
            if !previous_is_upper || next_is_lower {
                result.push('_');
            }
        }
        result.push(character.to_ascii_lowercase());
    }
    result
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "async"
            | "await"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "union"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "yield"
    )
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn line(output: &mut String, arguments: std::fmt::Arguments<'_>) {
    let _ignored = output.write_fmt(arguments);
    output.push('\n');
}

fn rustfmt(content: &str) -> Result<String, String> {
    let mut temporary = tempfile::Builder::new()
        .prefix("offprint-cdp-")
        .suffix(".rs")
        .tempfile()
        .map_err(|error| format!("failed to create generated CDP staging file: {error}"))?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|error| format!("failed to stage generated CDP source: {error}"))?;
    let status = ProcessCommand::new("rustfmt")
        .args(["--edition", "2024"])
        .arg(temporary.path())
        .status()
        .map_err(|error| format!("failed to start rustfmt for generated CDP source: {error}"))?;
    if !status.success() {
        return Err(format!(
            "rustfmt failed for generated CDP source with {status}"
        ));
    }
    fs::read_to_string(temporary.path())
        .map_err(|error| format!("failed to read formatted CDP source: {error}"))
}

fn update_generated_files(
    root: &Path,
    files: &BTreeMap<PathBuf, String>,
    check: bool,
) -> Result<(), String> {
    let directory = root.join(GENERATED_DIRECTORY);
    let expected = files.keys().cloned().collect::<BTreeSet<_>>();
    let actual = if directory.is_dir() {
        fs::read_dir(&directory)
            .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
            .map(|entry| {
                entry
                    .map_err(|error| format!("failed to inspect {}: {error}", directory.display()))
                    .map(|entry| PathBuf::from(entry.file_name()))
            })
            .collect::<Result<BTreeSet<_>, _>>()?
    } else {
        BTreeSet::new()
    };
    let unexpected = actual.difference(&expected).cloned().collect::<Vec<_>>();
    let legacy = root.join(LEGACY_GENERATED_FILE);
    if check && (legacy.exists() || !unexpected.is_empty()) {
        let mut stale = unexpected
            .iter()
            .map(|path| directory.join(path).display().to_string())
            .collect::<Vec<_>>();
        if legacy.exists() {
            stale.push(legacy.display().to_string());
        }
        return Err(format!(
            "stale generated CDP source must be removed: {}",
            stale.join(", ")
        ));
    }
    if !check {
        fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to create {}: {error}", directory.display()))?;
        for path in unexpected {
            let path = directory.join(path);
            if path.is_file() {
                fs::remove_file(&path)
                    .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
            } else {
                return Err(format!(
                    "generated CDP directory contains unsupported entry {}",
                    path.display()
                ));
            }
        }
        if legacy.exists() {
            fs::remove_file(&legacy)
                .map_err(|error| format!("failed to remove {}: {error}", legacy.display()))?;
        }
    }
    for (relative, content) in files {
        update_file(&directory.join(relative), content.as_bytes(), check)?;
    }
    Ok(())
}

fn update_file(path: &Path, content: &[u8], check: bool) -> Result<(), String> {
    if fs::read(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if check {
        return Err(format!("generated CDP source is stale: {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(path, content).map_err(|error| format!("failed to write {}: {error}", path.display()))
}
