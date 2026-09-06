# Artifact format reference

Offprint HTML is the canonical capture artifact. PDF, Markdown, ZIP,
self-extracting HTML, and Multipurpose Internet Mail Extensions HTML (MHTML)
are export formats derived from a verified HTML source.

## Choose a format

| Format | Output shape | Open with | Executable content | Provenance |
| --- | --- | --- | --- | --- |
| Offprint HTML | One `.html` file | Modern browser | Exact Offprint restoration programs | Full embedded artifact manifest |
| PDF | One `.pdf` file | PDF reader | Rejected by verifier | Selected document metadata, source provenance, and resource summary |
| Markdown | Directory with `index.md` and assets | Markdown reader | Raw HTML rejected | Source, capture time, and policy digest front matter by default |
| ZIP | One `.zip` file | Archive tool | Contained Offprint HTML owns its normal restoration programs | Full embedded and sidecar manifests |
| Self-extracting HTML | One `.compressed.html` file | Browser with `DecompressionStream` | Inline Offprint decompression loader | Full manifest inside compressed Offprint HTML |
| MHTML | One `.mhtml` file | Browser with MHTML import | Restoration execution removed | Full embedded and sidecar manifests |

## Offprint HTML

The HTML artifact embeds render dependencies, the artifact manifest, exact
state-restoration programs, and a matching content security policy. Use
`artifact verify` for static or offline verification. HTML cannot be requested
through `FormatSpec` or `verify_format`.

## PDF

PDF export prints the offline-verified page through Chromium. It preserves
selectable text, printable HTTP, HTTPS, mail, and telephone links, tagged
structure, document outline, and selected source metadata.

The verifier rejects encryption, embedded files, rich media, local file links,
automatic actions, executable actions, malformed content streams, and metadata
that disagrees with the Offprint provenance packet.

Options:

- `landscape` prints in landscape orientation.
- `preferCssPageSize` uses the captured document's CSS page size when present.

## Markdown

Markdown export preserves a semantic subset of the document, including
headings, paragraphs, emphasis, code, links, images, blockquotes, lists,
tables, and inline frame content.

Images become content-addressed files under `assets/`. The verifier rejects raw
HTML, unsafe link schemes, missing assets, extra assets, and invalid asset
digests. Front matter contains source, capture time, and policy digest unless
`frontMatter` is false. The Markdown verifier validates syntax, links, and asset
integrity. It does not validate front-matter provenance values.

The export transaction accepts at most 10,000 Markdown assets.

## ZIP

ZIP contains exactly two ordered deflated members:

```text
index.html
offprint-manifest.json
```

The verifier checks the archive structure, statically verifies `index.html`,
and requires the sidecar manifest to match the embedded manifest exactly.

## Self-extracting HTML

Self-extracting HTML stores gzip-compressed Offprint HTML in a base64 payload.
An inline loader uses the browser's `DecompressionStream` API and
`document.write` to replace the shell with the decompressed artifact.

This export executes the known loader and uses `script-src 'unsafe-inline'` in
its shell. Treat it differently from canonical safe-static HTML. The verifier
checks the exact shell bytes, compressed payload digest, decompression limit,
and static validity of the contained Offprint HTML.

## MHTML

MHTML is a deterministic `multipart/related` message with two base64 parts:

- Sandboxed HTML
- `offprint-manifest.json`

The encoder removes Offprint restoration execution before packaging. An
artifact that requires structural repair cannot be represented as MHTML. The
verifier checks the exact two-part structure, manifest equality, content
locations, deterministic boundary, and static sandboxed HTML.

## Fixed verification limits

- HTML inspection and source loading: 64 MiB
- Decoded HTML inside container formats: 64 MiB
- Decoded manifest sidecar: 16 MiB
- Each exported payload: 256 MiB
- Markdown assets: 10,000 files

Format verifiers establish the representation-specific structure and content
contract. ZIP and MHTML verify full manifest sidecars, PDF verifies its selected
provenance projection, and self-extracting HTML verifies the contained Offprint
HTML. Markdown provenance remains emitted metadata. Format verification does
not perform a network-denied browser reopen.
