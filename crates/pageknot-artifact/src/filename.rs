const MAXIMUM_STEM_BYTES: usize = 96;

#[must_use]
pub fn portable_file_stem(value: &str) -> String {
    let mut stem = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_') {
            if separator && !stem.is_empty() && stem.len() < MAXIMUM_STEM_BYTES {
                stem.push('-');
            }
            separator = false;
            for lowercase in character.to_lowercase() {
                if stem.len().saturating_add(lowercase.len_utf8()) > MAXIMUM_STEM_BYTES {
                    break;
                }
                stem.push(lowercase);
            }
        } else {
            separator = true;
        }
        if stem.len() >= MAXIMUM_STEM_BYTES {
            break;
        }
    }
    let stem = stem.trim_matches(['-', '.', ' ']);
    if stem.is_empty() || is_reserved_file_stem(stem) {
        "capture".to_owned()
    } else {
        stem.to_owned()
    }
}

fn is_reserved_file_stem(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{MAXIMUM_STEM_BYTES, portable_file_stem};

    #[test]
    fn reserved_device_name_uses_the_capture_fallback() {
        assert_eq!(portable_file_stem("CON"), "capture");
    }

    proptest! {
        #![proptest_config(ProptestConfig::default())]

        #[test]
        fn arbitrary_titles_produce_one_portable_component(value in any::<String>()) {
            let stem = portable_file_stem(&value);

            prop_assert!(!stem.is_empty());
            prop_assert!(stem.len() <= MAXIMUM_STEM_BYTES);
            prop_assert!(!stem.contains(['/', '\\', ':', '\0']));
            prop_assert!(!stem.ends_with(['.', ' ']));
        }
    }
}
