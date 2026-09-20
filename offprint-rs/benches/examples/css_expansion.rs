use std::collections::BTreeMap;
use std::error::Error;
use std::hint::black_box;
use std::io::Write as _;
use std::time::Instant;

use offprint_document::discover_css_resources;
use offprint_model::ContentDigest;
use serde_json::json;
use url::Url;

fn main() -> Result<(), Box<dyn Error>> {
    let base = Url::parse("https://fixture.invalid/")?;
    let source = (0..2_000)
        .map(|index| format!(".image-{index}{{background:url(image-{index}.png)}}\n"))
        .collect::<String>();
    let resources = discover_css_resources(&source, &base)?;
    let data_url = format!("data:image/png;base64,{}", "A".repeat(4096));
    let replacements = resources
        .resources()
        .iter()
        .map(|resource| (resource.id, data_url.clone()))
        .collect::<BTreeMap<_, _>>();
    let expected = (0..2_000)
        .map(|index| format!(".image-{index}{{background:url(\"{data_url}\")}}\n"))
        .collect::<String>();
    let output = resources.rewrite(&replacements)?;
    if output != expected {
        return Err("CSS expansion changed source text outside the resource URLs".into());
    }
    for _ in 0..2 {
        black_box(resources.rewrite(&replacements)?);
    }
    let mut samples = Vec::new();
    for _ in 0..12 {
        let start = Instant::now();
        black_box(resources.rewrite(&replacements)?);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    writeln!(
        std::io::stdout().lock(),
        "{}",
        serde_json::to_string_pretty(&json!({
            "case": "css-inline-resource-expansion",
            "references": resources.resources().len(),
            "sourceBytes": source.len(),
            "outputBytes": output.len(),
            "outputSha256": ContentDigest::sha256(output.as_bytes()),
            "samplesMilliseconds": samples,
            "medianMilliseconds": samples[samples.len() / 2],
        }))?
    )?;
    Ok(())
}
