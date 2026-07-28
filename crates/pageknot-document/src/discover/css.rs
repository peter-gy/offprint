use std::collections::BTreeMap;
use std::ops::Range;

use cssparser::{Parser, ParserInput, Token};
use lightningcss::dependencies::Dependency;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use pageknot_model::{ResourceId, Result};
use url::Url;

use super::{Reference, resource_error};

#[derive(Clone, Debug)]
pub struct DiscoveredCssResource {
    pub id: ResourceId,
    pub original: String,
    pub resolved_url: Url,
}

#[derive(Clone, Debug)]
pub struct CssResources {
    source: String,
    resources: Vec<DiscoveredCssResource>,
    ranges: BTreeMap<ResourceId, Range<usize>>,
    parse_mode: CssParseMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CssParseMode {
    Typed,
    PreservationFallback,
}

impl CssResources {
    #[must_use]
    pub fn resources(&self) -> &[DiscoveredCssResource] {
        &self.resources
    }

    #[must_use]
    pub const fn parse_mode(&self) -> CssParseMode {
        self.parse_mode
    }

    pub fn rewrite(&self, replacements: &BTreeMap<ResourceId, String>) -> Result<String> {
        let mut changes = Vec::new();
        for (id, replacement) in replacements {
            let range = self.ranges.get(id).ok_or_else(|| {
                resource_error(
                    "pageknot.resource.identifier",
                    format!("CSS resource {} has no rewrite range", id.get()),
                )
            })?;
            changes.push((
                range.clone(),
                format!("url(\"{}\")", escape_css_string(replacement)),
            ));
        }
        changes.sort_by_key(|change| std::cmp::Reverse(change.0.start));
        let mut rewritten = self.source.clone();
        for (range, replacement) in changes {
            if range.start > range.end
                || range.end > rewritten.len()
                || !rewritten.is_char_boundary(range.start)
                || !rewritten.is_char_boundary(range.end)
            {
                return Err(resource_error(
                    "pageknot.resource.rewrite",
                    "CSS resource rewrite range is outside its source text",
                ));
            }
            rewritten.replace_range(range, &replacement);
        }
        Ok(rewritten)
    }
}

pub fn discover_css_resources(css: &str, base_url: &Url) -> Result<CssResources> {
    discover_css_resources_bounded(css, base_url, usize::MAX)
}

pub fn discover_css_resources_bounded(
    css: &str,
    base_url: &Url,
    maximum: usize,
) -> Result<CssResources> {
    let references = css_references_bounded(css, maximum)?;
    let parse_mode = typed_css_dependencies(css).map_or(
        CssParseMode::PreservationFallback,
        |typed_dependencies| {
            let mut token_dependencies = references
                .iter()
                .map(|reference| reference.value.as_str())
                .collect::<Vec<_>>();
            let mut typed_dependencies = typed_dependencies
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            token_dependencies.sort_unstable();
            typed_dependencies.sort_unstable();
            if token_dependencies == typed_dependencies {
                CssParseMode::Typed
            } else {
                CssParseMode::PreservationFallback
            }
        },
    );
    let mut resources = Vec::with_capacity(references.len().min(maximum));
    let mut ranges = BTreeMap::new();
    for reference in references {
        let trimmed = reference.value.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let resolved_url = base_url.join(trimmed).map_err(|error| {
            resource_error(
                "pageknot.resource.url",
                format!("CSS resource URL `{trimmed}` cannot be resolved: {error}"),
            )
        })?;
        let index = u32::try_from(resources.len()).map_err(|error| {
            resource_error(
                "pageknot.resource.limit",
                format!("CSS resource count exceeds the identifier range: {error}"),
            )
        })?;
        let id = ResourceId::new(index);
        resources.push(DiscoveredCssResource {
            id,
            original: reference.value,
            resolved_url,
        });
        ranges.insert(id, reference.range);
    }
    Ok(CssResources {
        source: css.to_owned(),
        resources,
        ranges,
        parse_mode,
    })
}

fn typed_css_dependencies(css: &str) -> Option<Vec<String>> {
    let stylesheet = StyleSheet::parse(css, ParserOptions::default()).ok()?;
    let printed = stylesheet
        .to_css(PrinterOptions {
            analyze_dependencies: Some(Default::default()),
            ..PrinterOptions::default()
        })
        .ok()?;
    Some(
        printed
            .dependencies
            .unwrap_or_default()
            .into_iter()
            .map(|dependency| match dependency {
                Dependency::Import(dependency) => dependency.url,
                Dependency::Url(dependency) => dependency.url,
            })
            .collect(),
    )
}

pub(super) fn css_references_bounded(css: &str, maximum: usize) -> Result<Vec<Reference>> {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut references = Vec::new();
    let mut resource_count = 0;
    let mut exceeded = false;
    collect_css_components(
        &mut parser,
        &mut references,
        false,
        maximum,
        &mut resource_count,
        &mut exceeded,
    );
    if exceeded {
        return Err(resource_error(
            "pageknot.resource.limit",
            "CSS resource count exceeds the configured limit",
        ));
    }
    Ok(references)
}

fn collect_css_components(
    parser: &mut Parser<'_, '_>,
    references: &mut Vec<Reference>,
    quoted_strings_are_urls: bool,
    maximum: usize,
    resource_count: &mut usize,
    exceeded: &mut bool,
) {
    let mut import_value = false;
    while !parser.is_exhausted() && !*exceeded {
        let start = parser.position();
        let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
            break;
        };
        match token {
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                import_value = true;
            }
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            Token::UnquotedUrl(value) => {
                add_css_reference(
                    references,
                    value.to_string(),
                    start.byte_index()..parser.position().byte_index(),
                    maximum,
                    resource_count,
                    exceeded,
                );
                import_value = false;
            }
            Token::QuotedString(value) if import_value || quoted_strings_are_urls => {
                add_css_reference(
                    references,
                    value.to_string(),
                    start.byte_index()..parser.position().byte_index(),
                    maximum,
                    resource_count,
                    exceeded,
                );
                import_value = false;
            }
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                let mut value = None;
                let parsed: std::result::Result<(), _> = parser.parse_nested_block(|nested| {
                    let token = nested.next().cloned()?;
                    if let Token::QuotedString(url) | Token::UnquotedUrl(url) = token {
                        value = Some(url.to_string());
                    }
                    while nested.next_including_whitespace_and_comments().is_ok() {}
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
                if parsed.is_ok()
                    && let Some(value) = value
                {
                    add_css_reference(
                        references,
                        value,
                        start.byte_index()..parser.position().byte_index(),
                        maximum,
                        resource_count,
                        exceeded,
                    );
                }
                import_value = false;
            }
            Token::Function(name) => {
                let nested_strings_are_urls = matches!(
                    name.to_ascii_lowercase().as_ref(),
                    "image-set" | "-webkit-image-set"
                );
                let _parsed: std::result::Result<(), _> = parser.parse_nested_block(|nested| {
                    collect_css_components(
                        nested,
                        references,
                        nested_strings_are_urls,
                        maximum,
                        resource_count,
                        exceeded,
                    );
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
                import_value = false;
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let _parsed: std::result::Result<(), _> = parser.parse_nested_block(|nested| {
                    collect_css_components(
                        nested,
                        references,
                        quoted_strings_are_urls,
                        maximum,
                        resource_count,
                        exceeded,
                    );
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
                import_value = false;
            }
            Token::Semicolon => import_value = false,
            _ => {
                if import_value {
                    import_value = false;
                }
            }
        }
    }
}

fn add_css_reference(
    references: &mut Vec<Reference>,
    value: String,
    range: Range<usize>,
    maximum: usize,
    resource_count: &mut usize,
    exceeded: &mut bool,
) {
    let trimmed = value.trim();
    if !trimmed.is_empty() && !trimmed.starts_with('#') {
        if *resource_count >= maximum {
            *exceeded = true;
            return;
        }
        *resource_count += 1;
    }
    references.push(Reference { value, range });
}

pub(super) fn escape_css_string(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\0' => "\u{fffd}".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\a ".chars().collect(),
            '\r' => "\\d ".chars().collect(),
            '\u{000c}' => "\\c ".chars().collect(),
            character if character.is_control() => {
                format!("\\{:x} ", u32::from(character)).chars().collect()
            }
            character => vec![character],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use url::Url;

    use super::{
        CssParseMode, css_references_bounded, discover_css_resources,
        discover_css_resources_bounded, escape_css_string,
    };

    #[test]
    fn typed_inventory_matches_source_preserving_ranges() {
        let base = Url::parse("https://example.test/assets/").ok();
        let discovered = base.as_ref().map(|base| {
            discover_css_resources(
                r#"@import "theme.css";.fixture{background:url("image.svg")}"#,
                base,
            )
        });

        assert_eq!(
            discovered
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(super::CssResources::parse_mode),
            Some(CssParseMode::Typed)
        );
    }

    #[test]
    fn unrecognized_css_keeps_a_source_preserving_inventory() {
        let base = Url::parse("https://example.test/assets/").ok();
        let discovered = base.as_ref().map(|base| {
            discover_css_resources(
                r#".fixture{unknown-property: something(;background:url("image.svg")}"#,
                base,
            )
        });

        assert_eq!(
            discovered
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(super::CssResources::parse_mode),
            Some(CssParseMode::PreservationFallback)
        );
        assert_eq!(
            discovered
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(|resources| resources.resources().len()),
            Some(1)
        );
    }

    #[test]
    fn bounded_discovery_stops_on_the_first_excess_reference() {
        let base = Url::parse("https://example.test/assets/").ok();
        let discovered = base.as_ref().map(|base| {
            discover_css_resources_bounded(
                ".fixture{background:url(one.png),url(two.png)}",
                base,
                1,
            )
        });

        assert_eq!(
            discovered
                .and_then(std::result::Result::err)
                .map(|error| error.code.as_str().to_owned()),
            Some("pageknot.resource.limit".to_owned())
        );
    }

    fn css_string_strategy() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                any::<char>()
                    .prop_filter("CSS strings exclude null", |character| *character != '\0'),
                Just('"'),
                Just('\\'),
                Just('\n'),
                Just('\r'),
                Just('\u{000c}'),
            ],
            0..128,
        )
        .prop_map(|characters| characters.into_iter().collect())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn css_string_escaping_round_trips_through_the_parser(value in css_string_strategy()) {
            let css = format!(r#".fixture{{background:url("{}")}}"#, escape_css_string(&value));
            let references = css_references_bounded(&css, usize::MAX)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;

            prop_assert_eq!(references.len(), 1);
            prop_assert_eq!(&references[0].value, &value);
        }

        #[test]
        fn relative_css_urls_resolve_against_the_document_base(
            directories in proptest::collection::vec("[a-z][a-z0-9_-]{0,12}", 0..8),
            leaf in "[a-z][a-z0-9_-]{0,12}\\.(css|png|woff2)",
        ) {
            let base_path = if directories.is_empty() {
                "/".to_owned()
            } else {
                format!("/{}/", directories.join("/"))
            };
            let base = Url::parse(&format!("https://example.test{base_path}"))
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let expected = base
                .join(&leaf)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let discovered = discover_css_resources(
                &format!(".fixture{{background:url({leaf})}}"),
                &base,
            )
            .map_err(|error| TestCaseError::fail(error.to_string()))?;

            prop_assert_eq!(discovered.resources().len(), 1);
            prop_assert_eq!(&discovered.resources()[0].resolved_url, &expected);
        }
    }
}
