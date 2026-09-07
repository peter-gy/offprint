# Schemas and versioning

Offprint has independent product, public schema, artifact format, collector,
browser, and protocol axes. `versions.toml` records the release-aligned values.

| Axis               | Owner                                     | Current value   |
| ------------------ | ----------------------------------------- | --------------- |
| Product            | Workspace and package manifests           | `0.0.1`         |
| Public schema      | `offprint_model::PUBLIC_SCHEMA_VERSION`   | `2`             |
| Artifact format    | `offprint_model::ARTIFACT_FORMAT_VERSION` | `2`             |
| Collector protocol | `offprint-protocol`                       | `1.5`           |
| Collector bundle   | `versions.toml` reservation               | `1`             |
| Browser catalog    | `offprint-chromium` managed catalog       | `2026-07-27`    |
| CDP input          | Pinned upstream revision and hashes       | `versions.toml` |

Repository and package checks compare `versions.toml` with public model and
collector constants before release metadata is written.

`collector_bundle` is currently recorded but not consumed by generation,
handshake validation, or release checks. Treat it as an unenforced reserved
axis until an owner uses it.

## Public schema

`offprint-model` owns canonical request, event, result, error, browser,
artifact, resource, export, batch, crawl, and resume records. Serde naming owns
the wire shape. Schemars derives JSON Schema.

Closed request and policy records use `deny_unknown_fields`. A compatible schema
version does not imply forward tolerance for unknown fields. Adding a field to
a closed record can break older decoders and requires explicit compatibility
review.

Generated contracts include:

- JSON Schema files
- JSON examples
- Binding contract inventory
- Node.js TypeScript aliases
- Python TypedDict declarations
- CLI JSON contract inventory

JSON Schemas describe serialization shape and encode scheduler collection,
non-whitespace identifier text, concurrency, page, depth, viewport, and capture
limit bounds. Runtime validation also checks identifier uniqueness, credentials,
output ownership, and cross-field rules that JSON Schema does not express
completely.

## Artifact format

The artifact format version covers canonical Offprint HTML structure, manifest
placement, exact owned programs, content security policy, resource embedding,
and static verification. Export representations have their own format-specific
verifiers but share the product release version.

An artifact verifier rejects a mismatched public schema or artifact format
version.

## Collector protocol

Collector protocol uses independent major and minor numbers. The handshake
carries capture ID, host and collector digests, requested and available
capabilities, and maximum chunk size. The host validates the capture ID, echoed
host digest, and requested capabilities. Collector digest enforcement remains
an implementation gap. Major mismatch fails. Minor negotiation requires the
requested capabilities.

## Change procedure

For a public record or artifact change:

1. Change the owning Rust type or format implementation.
2. Decide which version axis changes.
3. Update `versions.toml` when the public schema or artifact format changes.
4. Regenerate schemas, examples, and binding contracts.
5. Update CLI and host mappings.
6. Run freshness and compatibility checks.
7. Update user concepts, task guides, and exact reference.
8. Review old inputs, error behavior, and migration requirements.

Product Semantic Versioning review covers exported Rust APIs, package APIs,
defaults, field meaning, enum values, exit status, and lifecycle guarantees.
