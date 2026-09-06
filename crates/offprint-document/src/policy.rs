use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};

const REQUEST_TRIGGERING_LINK_REL_TOKENS: [&str; 12] = [
    "compression-dictionary",
    "dns-prefetch",
    "import",
    "manifest",
    "modulepreload",
    "preconnect",
    "prefetch",
    "preload",
    "prerender",
    "serviceworker",
    "subresource",
    "webbundle",
];

const ICON_LINK_REL_TOKENS: [&str; 4] = [
    "icon",
    "apple-touch-icon",
    "apple-touch-icon-precomposed",
    "mask-icon",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkRelPolicy {
    Other,
    Stylesheet,
    Icon,
    RequestTrigger,
}

impl LinkRelPolicy {
    #[must_use]
    pub const fn triggers_unsupported_request(self) -> bool {
        matches!(self, Self::RequestTrigger)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SafeStaticPolicy;

impl SafeStaticPolicy {
    #[must_use]
    pub fn classify_link_rel(value: &str) -> LinkRelPolicy {
        let mut stylesheet = false;
        let mut icon = false;
        for token in value.split_ascii_whitespace() {
            if is_unsupported_request_token(token) {
                return LinkRelPolicy::RequestTrigger;
            }
            stylesheet |= token.eq_ignore_ascii_case("stylesheet");
            icon |= is_icon_token(token);
        }
        if stylesheet {
            LinkRelPolicy::Stylesheet
        } else if icon {
            LinkRelPolicy::Icon
        } else {
            LinkRelPolicy::Other
        }
    }

    #[must_use]
    pub fn link_rel(attrs: &[Attribute]) -> LinkRelPolicy {
        attribute_value(attrs, "rel")
            .map(Self::classify_link_rel)
            .unwrap_or(LinkRelPolicy::Other)
    }

    #[must_use]
    pub fn is_request_triggering_link(name: &QualName, attrs: &[Attribute]) -> bool {
        name.ns == ns!(html)
            && name.local == local_name!("link")
            && Self::link_rel(attrs).triggers_unsupported_request()
    }

    #[must_use]
    pub fn is_script_element(name: &QualName) -> bool {
        matches!(name.ns, ns!(html) | ns!(svg)) && name.local == local_name!("script")
    }

    #[must_use]
    pub fn is_structured_metadata_script(attrs: &[Attribute]) -> bool {
        attribute_value(attrs, "type").is_some_and(|value| {
            matches!(
                media_type(value).as_str(),
                "application/json"
                    | "application/ld+json"
                    | "application/vnd.offprint.manifest+json"
                    | "application/vnd.offprint.repair+json"
            )
        })
    }

    #[must_use]
    pub fn is_event_attribute(attribute: &Attribute) -> bool {
        attribute.name.ns == ns!()
            && attribute
                .name
                .local
                .as_ref()
                .to_ascii_lowercase()
                .starts_with("on")
    }

    #[must_use]
    pub fn is_javascript_url_attribute(attribute: &Attribute) -> bool {
        is_url_attribute(attribute) && is_javascript_url(attribute.value.as_ref())
    }

    #[must_use]
    pub fn is_base_element(name: &QualName) -> bool {
        name.ns == ns!(html) && name.local == local_name!("base")
    }

    #[must_use]
    pub fn is_meta_refresh(name: &QualName, attrs: &[Attribute]) -> bool {
        name.ns == ns!(html)
            && name.local == local_name!("meta")
            && attribute_value(attrs, "http-equiv")
                .is_some_and(|value| value.eq_ignore_ascii_case("refresh"))
    }
}

fn is_unsupported_request_token(token: &str) -> bool {
    REQUEST_TRIGGERING_LINK_REL_TOKENS
        .into_iter()
        .any(|candidate| token.eq_ignore_ascii_case(candidate))
}

fn is_icon_token(token: &str) -> bool {
    ICON_LINK_REL_TOKENS
        .into_iter()
        .any(|candidate| token.eq_ignore_ascii_case(candidate))
}

fn is_url_attribute(attribute: &Attribute) -> bool {
    (attribute.name.ns == ns!()
        && matches!(
            attribute.name.local.as_ref(),
            "action" | "cite" | "data" | "formaction" | "href" | "poster" | "src"
        ))
        || (attribute.name.ns == ns!(xlink) && attribute.name.local.as_ref() == "href")
}

fn is_javascript_url(value: &str) -> bool {
    let compact = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && !character.is_control())
        .collect::<String>();
    compact
        .get(.."javascript:".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("javascript:"))
}

fn media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn attribute_value<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_ref())
}

#[cfg(test)]
mod tests {
    use super::{LinkRelPolicy, REQUEST_TRIGGERING_LINK_REL_TOKENS, SafeStaticPolicy};

    #[test]
    fn link_rel_tokens_are_ascii_case_insensitive_and_whitespace_separated() {
        assert_eq!(
            SafeStaticPolicy::classify_link_rel("  alternate\tStyleSheet\n"),
            LinkRelPolicy::Stylesheet
        );
        assert_eq!(
            SafeStaticPolicy::classify_link_rel("shortcut\r\nICON"),
            LinkRelPolicy::Icon
        );
        assert_eq!(
            SafeStaticPolicy::classify_link_rel("alternate\x0cMANIFEST stylesheet"),
            LinkRelPolicy::RequestTrigger
        );
        assert_eq!(
            SafeStaticPolicy::classify_link_rel("license external"),
            LinkRelPolicy::Other
        );
    }

    #[test]
    fn every_unsupported_request_token_has_precedence() {
        for token in REQUEST_TRIGGERING_LINK_REL_TOKENS {
            assert_eq!(
                SafeStaticPolicy::classify_link_rel(&format!("icon {token} stylesheet")),
                LinkRelPolicy::RequestTrigger,
                "{token}"
            );
        }
    }
}
