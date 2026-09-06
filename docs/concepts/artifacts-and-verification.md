# Artifacts and verification

An **artifact** is an encoded user-visible representation. An **Offprint HTML
artifact** is the canonical capture. An **export artifact** is a derived PDF,
Markdown, ZIP, self-extracting HTML, or MHTML representation.

Offprint separates the representation from the evidence used to accept it.
The canonical representation is Offprint HTML. Its artifact manifest
travels inside the file, while a capture receipt reports the result of the
operation that created it.

## Safe-static HTML

An **Offprint HTML artifact** is safe-static when it:

- Embeds render dependencies as controlled local data
- Removes captured executable page scripts and event handlers
- Removes automatic refresh and request-triggering references
- Permits only the exact Offprint restoration programs required by its format
- Carries the matching content security policy and artifact manifest
- Passes static verification

Captured form, disclosure, and shadow state is materialized into HTML before
encoding. The state restoration program restores document and element scroll
positions while traversing nested frames and shadow roots. A second owned
program appears only when serialized HTML needs structural repair after browser
parsing. Static verification checks the exact program bytes and their
content-policy hashes.

## Artifact manifest

The **artifact manifest** is the embedded `ArtifactManifest` record. It stores:

- Public schema and artifact format versions
- Generator version
- Redacted source URLs and full-source digests
- Capture time
- Browser source, product, version, protocol, and managed revision
- Browser environment and captured scroll position
- Capture-policy digest
- Frame count
- Aggregate resource counts and individual resource records
- Warning codes
- Structural-repair state
- Requested verification mode

**Structural repair** handles HTML whose parsed tree would change after
serialization. Offprint records a minimal repair tree and permits an exact
owned program that restores captured attribute placement after browser parsing.
`structuralRepair.applied` and `scriptSha256` expose that requirement. MHTML
cannot represent an artifact that requires repair.

`artifact inspect` parses and validates this record. It does not validate the
complete HTML structure, owned scripts, embedded resource bytes, or offline
behavior.

## Verification modes and methods

| Check | Browser reopen | What it establishes |
| --- | --- | --- |
| Inspect | No | The embedded artifact manifest is present, parseable, and internally valid |
| Static verification | No | Manifest, content policy, owned scripts, frames, resource records, embedded syntax, and digests satisfy the HTML format |
| Offline verification | Yes | Static checks pass and a separate network-denied browser context reaches a stable page with zero observed requests |
| Format-specific verification | No | The exported representation satisfies its format-owned structure and content checks |

A **verification mode** is the requested HTML check, `static` or `offline`.
`VerificationReport` records the HTML result. `FormatVerification` records an
export verifier result. CLI `artifact verify --json` projects either result to
the uniform `ArtifactVerification` record and identifies the performed method.

The `networkRequests` field is observed during offline verification. The CLI's
`ArtifactVerification` projection sets it to zero for static and format-specific
methods, which do not perform a network-observation run. `FormatVerification`
itself contains format, bytes, and digest. Check `method` before interpreting a
CLI zero.

## Capture receipt versus manifest

| Record | Lifetime | Main purpose |
| --- | --- | --- |
| `CaptureReceipt` | Returned by one successful capture | Operation result, delivery, verification evidence, summary, warnings, timings |
| `ArtifactManifest` | Embedded in Offprint HTML | Long-lived capture provenance and per-resource records |
| `VerificationReport` | Returned by HTML verification | Digest-bound static or offline evidence |
| `ArtifactVerification` | Emitted by CLI verification JSON | Uniform automation result for HTML and exported formats |

The manifest records the requested mode. It does not claim that a later
verification run already succeeded. The verification report binds its evidence
to the exact artifact digest and byte count.

## File visibility and durability

Single-file capture stages and synchronizes verified bytes before installing
the destination. The selected conflict operation makes the complete file
visible atomically where the platform supports that operation. Offprint then
requests parent-directory synchronization. A failure after the rename cannot be
reported as an uncommitted capture, so success establishes visibility but does
not guarantee survival of the directory entry across an immediate host crash.

`capturePolicySha256` covers browser environment, readiness, content policy,
network policy, capture limits, and verification mode. It excludes source URL,
credentials, browser selection, headed state, output, and diagnostics. Static
verification checks the recorded digest's shape but cannot reconstruct the
original request to prove that provenance independently.

## Export formats

Exports derive from a verified Offprint HTML artifact. They are alternate
representations rather than new captures. Each format preserves a different
subset of content and provenance.

HTML appears in `ArtifactFormat`, but it is not accepted by `FormatSpec` or
`verify_format`. Use the HTML artifact service or `artifact verify` for the
canonical format. Use the [format reference](../reference/formats.md) for PDF,
Markdown, ZIP, self-extracting HTML, and MHTML.
