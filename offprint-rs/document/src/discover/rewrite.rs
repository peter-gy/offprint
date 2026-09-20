use std::ops::Range;

use offprint_model::Result;

use super::resource_error;

pub(super) fn rewrite_ranges(
    source: &str,
    mut changes: Vec<(Range<usize>, String)>,
) -> Result<String> {
    changes.sort_by_key(|change| change.0.start);
    let mut end = 0;
    let mut bytes = source.len();
    for (range, replacement) in &changes {
        if range.start < end || range.start > range.end || source.get(range.clone()).is_none() {
            return Err(resource_error(
                "offprint.resource.rewrite",
                "resource rewrite ranges overlap or are outside their source text",
            ));
        }
        bytes = bytes
            .checked_sub(range.len())
            .and_then(|bytes| bytes.checked_add(replacement.len()))
            .ok_or_else(|| {
                resource_error(
                    "offprint.resource.rewrite",
                    "resource rewrite size overflow",
                )
            })?;
        end = range.end;
    }
    let mut rewritten = String::new();
    rewritten.try_reserve_exact(bytes).map_err(|_| {
        resource_error(
            "offprint.resource.rewrite",
            "resource rewrite allocation failed",
        )
    })?;
    end = 0;
    for (range, replacement) in changes {
        rewritten.push_str(&source[end..range.start]);
        rewritten.push_str(&replacement);
        end = range.end;
    }
    rewritten.push_str(&source[end..]);
    Ok(rewritten)
}

#[cfg(test)]
mod tests {
    use super::rewrite_ranges;

    #[test]
    fn replacement_offsets_refer_to_original_utf8_source() -> Result<(), Box<dyn std::error::Error>>
    {
        assert_eq!(
            rewrite_ranges("é:a:b:終", vec![(5..6, "longer".into()), (3..4, "".into())])?,
            "é::longer:終"
        );
        assert_eq!(
            rewrite_ranges("ab", vec![(1..2, "y".into()), (0..1, "x".into())])?,
            "xy"
        );
        assert_eq!(rewrite_ranges("é", Vec::new())?, "é");
        Ok(())
    }

    #[test]
    fn invalid_ranges_fail_before_rewriting() {
        for changes in [
            vec![(0..2, "x".into()), (1..3, "y".into())],
            vec![(1..2, "x".into())],
            vec![(0..9, "x".into())],
        ] {
            assert!(rewrite_ranges("é:a", changes).is_err());
        }
    }
}
