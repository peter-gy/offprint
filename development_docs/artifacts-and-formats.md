# Artifacts, formats, and transactions

The service path is organized around typed proof boundaries. Lower export crates
also expose raw encoders and verifiers, so only service `export` and
`export_capture` enforce the complete offline-source workflow.

## Canonical HTML proof chain

```text
Document
    -> SafeStaticDocument
    -> encoded HTML
    -> VerifiedHtml
    -> VerifiedHtmlProof
    -> OfflineHtmlArtifact
```

Static verification owns `VerifiedHtmlProof`. `OfflineHtmlArtifact` combines
that proof with a digest-matching offline `VerificationReport`. Export requires
this offline source boundary.

`ArtifactService::export` obtains fresh offline evidence in screen media before
encoding any format. `export_capture` revalidates static bytes and reuses the
receipt's screen-media evidence when the schema, mode, byte count, digest, and
zero-request record agree. PDF rendering verifies a separate print-media
reopen. Its observation belongs to PDF preparation.

Browser verification selects `RenderingMedia::Screen` for HTML and
`RenderingMedia::Print` for PDF. Print verification activates print styles
before navigation, eagerly loads images across document, shadow, and frame
roots, and waits for image decoding and font readiness before printing.

## Safe-static invariant

Canonical HTML removes captured scripts, event handlers, JavaScript URLs,
automatic refresh, and uncontrolled request triggers. It can inject an exact
state-restoration program and an exact structural-repair program. The content
security policy pins their hashes, and static verification rejects every other
script.

The self-extracting export has a separate executable decompression shell and
must never be described as safe-static HTML itself.

## Format contracts

| Format               | Encoder and verifier boundary                                                                                                    |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| HTML                 | Embedded manifest, owned programs, content security policy, recursive resources, frames, static and offline verification         |
| PDF                  | Chromium print, semantic and action checks, selected provenance in document properties and Extensible Metadata Platform metadata |
| Markdown             | Semantic HTML subset, content-addressed assets, safe link schemes, optional front matter                                         |
| ZIP                  | Exact `index.html` and manifest sidecar with static HTML proof                                                                   |
| Self-extracting HTML | Deterministic gzip and base64 shell, exact loader, decoded static HTML proof                                                     |
| MHTML                | Exact two-part MIME message, sandboxed HTML, matching manifest sidecar                                                           |

The repository has per-format unit and integration tests. The common-harness
gap and its completion condition live in
[Testing](./testing.md#current-conformance-gap).

Adding a built-in format requires:

1. `ArtifactFormat` and `FormatSpec` model updates
2. Encoder and independent verifier
3. `VerifiedFormat<T>` preparation
4. Service dispatch and output naming
5. CLI mapping and format options
6. Node.js and Python mapping
7. Generated schemas and fixtures
8. Format and browser evidence
9. User decision table and exact reference

## Single-file transaction

File capture creates same-directory staging with restrictive permissions. It
hashes while writing, flushes, rereads, and rechecks length, digest, and staging
identity before commit.

Conflict modes are fail, replace, and uniquify. Replace uses the platform's
atomic replacement capability. Uniquify selects the first portable numbered
name. Drop removes uncommitted staging.

After installation, the writer requests parent-directory synchronization. A
sync failure occurs after the destination is visible and is represented
internally as `AppliedWithSyncWarning`. The current public receipt has no field
for that warning. Single-file success therefore establishes complete atomic
visibility, not unconditional crash durability for the directory entry.

## Multi-output transaction

The service owns a request-wide budget of 256 MiB and 10,001 files. It charges
each verified representation before retaining it for delivery and passes the
remaining allowance to Markdown preparation and PDF metadata encoding.
Markdown charges unique asset content and retained text buffers before growth.
Its temporary data-URL decoding buffer has the same byte ceiling. Document
parsing, transferred text buffers, and temporary escaping strings can allocate in proportion to source
input size. ZIP, self-extracting HTML, MHTML, and PDF can allocate
one representation before the service checks the request-wide total.

Source and format validation finish before output preparation. Output
preparation can create the requested directory. The transaction then validates
the destination path, name, entrypoint, file count, and byte total. An empty
output directory can commit by directory swap. A nonempty directory uses
entry-wise commit with a durable journal.

Journal phases are staging, prepared, mutating, rolling back, and committed.
Recovery records installed entries and backup locations. Startup on the same
output directory resolves a recoverable journal before new staging.

Do not claim universal atomicity for a nonempty export directory. The supported
contract is a recoverable set commit with explicit partial and indeterminate
error details.

## Fixed internal ceilings

- HTML artifact input: 64 MiB
- Decoded container HTML: 64 MiB
- Decoded manifest: 16 MiB
- All formats in one service export request: 256 MiB and 10,001 files combined
- Markdown assets: 10,000 unique files, within the request's file budget

`ArtifactTransactionLimits` remains a per-payload staging and recovery
contract. The service enforces the aggregate request budget before staging.

Review changes to these constants as public compatibility and denial-of-service
decisions.
