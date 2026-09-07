use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::{
    Domain, IMPORTED_DOMAINS, Property, SelectedItem, TypeDef, to_pascal_case, to_snake_case,
};

pub(super) fn imported_protocol_items(
    source_root: &Path,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    fn visit(
        directory: &Path,
        imports: &mut BTreeMap<String, BTreeSet<String>>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(directory)
            .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("failed to inspect {}: {error}", directory.display()))?;
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("generated") {
                    visit(&path, imports)?;
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                let source = fs::read_to_string(&path)
                    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
                for domain in IMPORTED_DOMAINS {
                    collect_imports(
                        &source,
                        domain,
                        imports.entry(domain.to_owned()).or_default(),
                    )?;
                }
            }
        }
        Ok(())
    }

    let mut imports = BTreeMap::new();
    visit(source_root, &mut imports)?;
    for domain in IMPORTED_DOMAINS {
        if imports.get(domain).is_none_or(BTreeSet::is_empty) {
            return Err(format!(
                "Chromium source imports no generated {domain} protocol items"
            ));
        }
    }
    Ok(imports)
}

fn collect_imports(
    source: &str,
    domain: &str,
    imports: &mut BTreeSet<String>,
) -> Result<(), String> {
    let marker = format!("crate::cdp::generated::cdp_{}::", to_snake_case(domain));
    let mut remaining = source;
    while let Some(index) = remaining.find(&marker) {
        remaining = &remaining[index + marker.len()..];
        if let Some(group) = remaining.strip_prefix('{') {
            let Some(end) = group.find('}') else {
                return Err(format!("unterminated generated CDP import for {domain}"));
            };
            for item in group[..end]
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
            {
                if item == "*" || item.contains(" as ") {
                    return Err(format!(
                        "generated CDP imports must name {domain} items directly"
                    ));
                }
                imports.insert(item.to_owned());
            }
            remaining = &group[end + 1..];
        } else {
            if remaining.starts_with('*') {
                return Err(format!(
                    "generated CDP imports must name {domain} items directly"
                ));
            }
            let end = remaining
                .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
                .unwrap_or(remaining.len());
            if end == 0 {
                return Err(format!("generated CDP import for {domain} omits an item"));
            }
            if remaining[end..].trim_start().starts_with("as ") {
                return Err(format!(
                    "generated CDP imports must name {domain} items directly"
                ));
            }
            imports.insert(remaining[..end].to_owned());
            remaining = &remaining[end..];
        }
    }
    Ok(())
}

pub(super) fn select_protocol_items(
    domains: &[Domain],
    imports: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeMap<String, BTreeSet<SelectedItem>>, String> {
    let domain_index = domains
        .iter()
        .map(|domain| (domain.domain.as_str(), domain))
        .collect::<BTreeMap<_, _>>();
    let mut selected = BTreeMap::<String, BTreeSet<SelectedItem>>::new();
    let mut pending_types = Vec::<(String, String)>::new();

    for (domain_name, symbols) in imports {
        let domain = domain_index
            .get(domain_name.as_str())
            .ok_or_else(|| format!("pinned CDP inputs omit imported domain {domain_name}"))?;
        for symbol in symbols {
            let item = find_protocol_item(domain, symbol).ok_or_else(|| {
                format!("generated CDP import {domain_name}::{symbol} has no protocol definition")
            })?;
            if selected
                .entry(domain_name.clone())
                .or_default()
                .insert(item.clone())
            {
                collect_item_references(domain, &item, &mut pending_types);
            }
        }
    }

    while let Some((domain_name, type_name)) = pending_types.pop() {
        let domain = domain_index
            .get(domain_name.as_str())
            .ok_or_else(|| format!("CDP type reference uses missing domain {domain_name}"))?;
        let type_def = domain
            .types
            .iter()
            .find(|type_def| type_def.id == type_name)
            .ok_or_else(|| {
                format!("CDP type reference uses missing type {domain_name}.{type_name}")
            })?;
        if selected
            .entry(domain_name.clone())
            .or_default()
            .insert(SelectedItem::Type(type_name))
        {
            collect_type_references(&domain.domain, type_def, &mut pending_types);
        }
    }
    Ok(selected)
}

fn find_protocol_item(domain: &Domain, symbol: &str) -> Option<SelectedItem> {
    domain
        .types
        .iter()
        .find(|type_def| type_def.id == symbol)
        .map(|type_def| SelectedItem::Type(type_def.id.clone()))
        .or_else(|| {
            domain.commands.iter().find_map(|command| {
                let base = to_pascal_case(&command.name);
                ["Command", "Params", "Result"]
                    .iter()
                    .any(|suffix| symbol == format!("{base}{suffix}"))
                    .then(|| SelectedItem::Command(command.name.clone()))
            })
        })
        .or_else(|| {
            domain.events.iter().find_map(|event| {
                (symbol == format!("{}Event", to_pascal_case(&event.name)))
                    .then(|| SelectedItem::Event(event.name.clone()))
            })
        })
}

fn collect_item_references(
    domain: &Domain,
    item: &SelectedItem,
    references: &mut Vec<(String, String)>,
) {
    let properties = match item {
        SelectedItem::Command(name) => domain
            .commands
            .iter()
            .find(|command| command.name == *name)
            .map(|command| {
                command
                    .parameters
                    .iter()
                    .chain(&command.returns)
                    .collect::<Vec<_>>()
            }),
        SelectedItem::Event(name) => domain
            .events
            .iter()
            .find(|event| event.name == *name)
            .map(|event| event.parameters.iter().collect()),
        SelectedItem::Type(_) => None,
    };
    for property in properties.into_iter().flatten() {
        collect_property_references(&domain.domain, property, references);
    }
}

fn collect_type_references(
    domain: &str,
    type_def: &TypeDef,
    references: &mut Vec<(String, String)>,
) {
    if let Some(reference) = &type_def.ref_type {
        references.push(split_reference(domain, reference));
    }
    if let Some(reference) = type_def
        .items
        .as_ref()
        .and_then(|item| item.ref_type.as_ref())
    {
        references.push(split_reference(domain, reference));
    }
    for property in &type_def.properties {
        collect_property_references(domain, property, references);
    }
}

fn collect_property_references(
    domain: &str,
    property: &Property,
    references: &mut Vec<(String, String)>,
) {
    if let Some(reference) = &property.ref_type {
        references.push(split_reference(domain, reference));
    }
    if let Some(reference) = property
        .items
        .as_ref()
        .and_then(|item| item.ref_type.as_ref())
    {
        references.push(split_reference(domain, reference));
    }
}

fn split_reference(domain: &str, reference: &str) -> (String, String) {
    reference.split_once('.').map_or_else(
        || (domain.to_owned(), reference.to_owned()),
        |(domain, name)| (domain.to_owned(), name.to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdp::Event;

    #[test]
    fn grouped_and_direct_imports_select_named_items() -> Result<(), String> {
        let source = "
            use crate::cdp::generated::cdp_browser::{GetVersionCommand, GetVersionParams};
            use crate::cdp::generated::cdp_browser::GetVersionResult;
        ";
        let mut imports = BTreeSet::new();
        collect_imports(source, "Browser", &mut imports)?;
        assert_eq!(
            imports,
            BTreeSet::from([
                "GetVersionCommand".to_owned(),
                "GetVersionParams".to_owned(),
                "GetVersionResult".to_owned(),
            ])
        );
        Ok(())
    }

    #[test]
    fn wildcard_imports_are_rejected() -> Result<(), String> {
        let mut imports = BTreeSet::new();
        let Err(error) = collect_imports(
            "use crate::cdp::generated::cdp_target::*;",
            "Target",
            &mut imports,
        ) else {
            return Err("a wildcard generated CDP import was accepted".to_owned());
        };
        assert!(error.contains("must name Target items directly"));
        Ok(())
    }

    #[test]
    fn aliased_imports_are_rejected() -> Result<(), String> {
        let mut imports = BTreeSet::new();
        let Err(error) = collect_imports(
            "use crate::cdp::generated::cdp_target::TargetID as Id;",
            "Target",
            &mut imports,
        ) else {
            return Err("an aliased generated CDP import was accepted".to_owned());
        };
        assert!(error.contains("must name Target items directly"));
        Ok(())
    }

    #[test]
    fn selection_includes_transitive_cross_domain_types() -> Result<(), String> {
        let browser = Domain {
            domain: "Browser".to_owned(),
            types: vec![TypeDef {
                id: "BrowserContextID".to_owned(),
                type_kind: "string".to_owned(),
                ref_type: None,
                properties: Vec::new(),
                enum_values: Vec::new(),
                items: None,
            }],
            commands: Vec::new(),
            events: Vec::new(),
        };
        let target = Domain {
            domain: "Target".to_owned(),
            types: vec![TypeDef {
                id: "TargetInfo".to_owned(),
                type_kind: "object".to_owned(),
                ref_type: None,
                properties: vec![Property {
                    name: "browserContextId".to_owned(),
                    type_kind: None,
                    ref_type: Some("Browser.BrowserContextID".to_owned()),
                    optional: false,
                    items: None,
                }],
                enum_values: Vec::new(),
                items: None,
            }],
            commands: Vec::new(),
            events: vec![Event {
                name: "attachedToTarget".to_owned(),
                parameters: vec![Property {
                    name: "targetInfo".to_owned(),
                    type_kind: None,
                    ref_type: Some("TargetInfo".to_owned()),
                    optional: false,
                    items: None,
                }],
            }],
        };
        let imports = BTreeMap::from([(
            "Target".to_owned(),
            BTreeSet::from(["AttachedToTargetEvent".to_owned()]),
        )]);

        let selected = select_protocol_items(&[browser, target], &imports)?;

        assert!(selected["Target"].contains(&SelectedItem::Event("attachedToTarget".to_owned())));
        assert!(selected["Target"].contains(&SelectedItem::Type("TargetInfo".to_owned())));
        assert!(selected["Browser"].contains(&SelectedItem::Type("BrowserContextID".to_owned())));
        Ok(())
    }
}
