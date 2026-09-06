# Resources and fidelity

A page can refer to the same URL from several document locations, under
different request headers, or from different frames. Offprint records each
render-affecting location as a resource reference instead of treating URL
equality as content identity.

## Reference, record, and outcome

A **resource reference** is one HTML, CSS, SVG, or embedded-document location
that addresses render-affecting content. Each reference receives a stable ID
and one **resource record** in the artifact manifest. The public record carries
the resource ID, frame ID, URL, outcome, and retrieval provenance. It does not
carry the exact node, attribute, stylesheet range, or rendering role.

The record contains:

- Resource and frame IDs
- Redacted requested URL and full-URL digest
- Terminal resource outcome
- Retrieval provenance when bytes were obtained

The current capture pipeline produces two outcomes:

- `embedded` records a content digest, media type, and decoded byte count.
- `failed` records a stable error code, redacted message, and retryability.

The public schema also contains `external` and `omitted` variants for format
evolution. Canonical safe-static HTML rejects external rendering resources, and
the current production capture path does not emit either reserved variant.

## Missing-resource policy

The default `warn` policy keeps a failed resource record, emits a structured
warning, and rewrites the reference to an inert fallback for its rendering
role. The artifact can remain self-contained while visual fidelity is reduced.

Use `fail` when every discovered reference must be embedded:

```console
offprint capture https://example.com \
  --missing-resources fail \
  --output example.html
```

This distinction separates two properties:

- **Self-contained** means reopening the artifact needs no external rendering
  resource.
- **Complete resource acquisition** means every discovered resource was
  embedded.

Read the receipt's aggregate summary for a quick decision. Inspect the artifact
manifest to identify the failed resource ID, frame, URL, and error. Repeated
references to one URL in a frame cannot be mapped back to their exact source
locations from the public artifact record.

## Retrieval provenance

An embedded resource records how Offprint obtained its bytes:

- Inline `data:` decoding
- Response observed during navigation
- Fetch inside the owning browser context
- Read from the owning frame for `blob:` data
- Read from an explicitly allowed local file

Provenance can also include the final redacted URL, final URL digest, response
status, redirect chain, and number of received bytes. Observed responses
preserve authenticated or service-worker-produced bytes when request identity
is unambiguous.

## Recursive discovery

Offprint discovers references in rendered HTML and continues into stylesheets,
SVG resources, and embedded HTML. Resource-recursion depth bounds nested CSS
and SVG retrieval. Frame depth bounds frames and recursively embedded HTML.
Resource limits count references rather than unique URLs.

The content store hashes bytes while receiving them and deduplicates stored
content by digest. `totalResourceBytes` counts bytes accepted before digest
deduplication. The receipt's `embeddedBytes` value counts unique embedded
digests. Encoding can still place the same data URL at every reference, so
`embeddedBytes` is not an artifact-size estimate.

Use [capture limits](../operations/limits-and-performance.md) to control these
budgets and [authenticated pages](../guides/authenticated-pages.md) when
resource identity depends on headers or cookies.
