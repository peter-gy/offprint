# Save a web page as PDF

Capture the rendered page, then export its verified HTML as a searchable PDF.
Keeping the HTML lets you inspect capture warnings and export again with
different print settings.

```console
offprint capture https://developer.apple.com/design/human-interface-guidelines/charts \
  --output charts.html
offprint artifact export charts.html --format pdf --output exports
```

Open `exports/charts.pdf`. PDF export uses Chromium's print layout, including
the page's print styles, background graphics, and embedded fonts. It loads
deferred images before printing. Export verifies the source HTML in screen
media and the print document in print media, with networking blocked for both. Source page scripts stay stripped from the captured HTML.

The PDF retains selectable text, heading outlines, accessibility tags, and
links. Links to captured element IDs navigate within the PDF. Links to other
pages resolve against the source URL. The page's HTML structure determines the
quality of its reading order and headings.

Offprint keeps headings with following content when the page leaves their
print-break behavior at its default. Explicit page-break rules remain in effect.

## Choose the content and layout

To capture a page's main content, select its `main` element:

```console
offprint capture https://developer.apple.com/design/human-interface-guidelines/charts \
  --selector main --output charts-main.html
offprint artifact export charts-main.html --format pdf --output exports
```

Selectors follow the source page's structure. Choose an element that contains
the complete article and its figures. A selector that matches nothing fails
the capture.

| Layout | Export option |
| --- | --- |
| Landscape pages | `--landscape` |
| Use the page's CSS `@page` size | `--prefer-css-page-size` |
| Replace an existing export after verification | `--on-exists replace` |

Keep print styles and print-only content when preparing the source. Capture
optimizers that remove hidden elements, unused CSS, or unused fonts evaluate
the captured screen state and can discard content used by the print layout.

## Check the result

```console
offprint artifact inspect charts.html --json > charts-manifest.json
offprint artifact verify exports/charts.pdf --format pdf --json
```

The HTML manifest records resource outcomes. A verified PDF can still reflect
missing source resources when capture used the default warning policy. Use
`--missing-resources fail` on capture when any missing resource should reject
the result.

PDF verification checks structure, links, and Offprint provenance. Inspect the
PDF visually for pagination, clipped figures, and the page's print styling.

For application integration, capture to memory and pass the receipt to Rust's
`artifacts().export_capture` with `FormatSpec::Pdf`. Node.js and Python can use
`artifacts.export` with a captured HTML path. See the
[service API](../reference/service-api.md) and
[PDF format contract](../reference/formats.md#pdf).
