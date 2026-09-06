# Offprint concepts

Offprint turns one live page into a verified capture artifact. The capture
artifact is safe-static HTML that records what Chromium rendered, carries its
provenance, and can be reopened without network access.

```text
URL
  -> capture
  -> observation
  -> document
  -> capture artifact
  -> verification report
  -> published file or memory value
```

The terms below define the product surface, serialized contracts, public APIs,
and internal ownership boundaries.

## Capture

A **capture** is one attempt to turn one URL into one capture artifact. It owns
browser observation, resource acquisition, transformation, encoding,
verification, and publication.

A **capture request** describes the source URL, browser environment, readiness
condition, content selection, credentials, network policy, limits,
verification mode, and output.

A **capture job** is the live, cancellable handle for a capture. Its status and
events describe work in progress. Its result is a `CaptureReceipt` after the
artifact has been verified and published. Failure and cancellation return an
`OffprintError`.

## Observation and document

An **observation** contains provider-neutral browser facts collected before
transformation. Frame relationships, live document state, resource references,
and visual fallbacks belong to this boundary.

A **document** is the internal parsed page model used to materialize state,
rewrite resources, remove active content, and serialize safe-static HTML.

## Artifact and format

An **artifact** is an encoded, user-visible result delivered as a file,
directory bundle, or memory value.

The **capture artifact** is Offprint HTML. It is the canonical input for
inspection, repeat verification, and export.

An **artifact format** identifies an encoding:

- `html` for the canonical capture artifact
- `pdf` for Chromium print output with Offprint provenance
- `markdown` for a Markdown entrypoint and content-addressed assets
- `zip` for a packaged capture artifact
- `self-extracting-html` for compressed HTML with a passive loader
- `mhtml` for a browser-native multipart archive

An **export** derives one or more artifacts from a verified capture artifact.
Every exported artifact passes the verifier for its format before publication.

## Output and publication

An **output** selects file or memory delivery. A file output carries its path
and conflict behavior. A memory output carries its byte limit.

A **destination** is the resolved final filesystem location. A
**publication** installs verified output at that destination. A
**transaction** is the internal staging and recovery mechanism used by
publication.

## Manifest and verification

The **artifact manifest** is provenance embedded in an Offprint HTML artifact.
It records the source, browser environment, content policy digest, resource
outcomes, warnings, format version, and requested verification mode.

A **verification mode** selects `static` or `offline` checking. Static
verification validates the artifact structure and active-content policy.
Offline verification also reopens the artifact in Chromium with network access
denied.

A **verification report** is digest-bound evidence produced after verification
succeeds. The CLI projects HTML and format-specific evidence into one
`ArtifactVerification` record. Invalid artifacts return an `OffprintError` at the verification
stage. The manifest records the requested mode. It does not claim that a later
verification run has already succeeded.

## Resource outcomes

A **resource reference** is one render-affecting URL at one document location.
Each reference receives one **resource outcome**:

- `embedded` when the artifact contains the resource bytes
- `external` when policy permits the reference to remain external
- `omitted` when policy intentionally removes it
- `failed` when acquisition or transformation fails

The capture receipt summarizes these outcomes and keeps structured warnings for
permitted fidelity gaps.

## Scheduling and browser ownership

A **batch item** is one named capture request in a batch. A **checkpoint** is
persisted scheduler state used to resume batch or crawl work. Reuse of a prior
outcome is a scheduling disposition, while success and failure remain capture
outcomes.

A **managed browser** is a catalog-pinned Chromium archive installed and owned
by Offprint. A **browser backend** implements browser observation. The Offprint
service is the composition root that selects backends, format encoders, output
delivery, and runtime ownership.
