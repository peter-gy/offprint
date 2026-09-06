# Offprint review guide

Review Offprint from the user boundary toward the implementation.

## Contract review

1. Identify the affected command, service method, public record, artifact
   field, or binding method.
2. Confirm Rust, CLI, Node.js, Python, schemas, examples, and docs use the same
   names and defaults.
3. Check that error codes, retryability, exit status, stdout, and stderr remain
   stable at the affected boundary.
4. Confirm every discovered resource receives one terminal outcome.

## Lifecycle review

Trace success, failure, cancellation, dropped consumers, and service shutdown.
Each browser lease, process, context, target, event producer, temporary store,
and staging output needs one terminal release path.

Pay particular attention to awaits between ownership acquisition and guard
construction. Confirm browser disconnects fail pending commands promptly.

## Security review

Check URL resolution, redirect revalidation, private-address policy, secret
redaction, file roots, archive extraction, symlinks, output replacement, CSP,
script removal, and offline verification. Confirm limits apply before
allocation and before body accumulation.

## Generated contract review

Run:

```console
just codegen-check
```

Generated changes need an owning source change. Review the public JSON examples
alongside schema changes.

## Validation

Use the smallest focused check during development, then run:

```console
just release-check
```

Changes to browser behavior also run the affected fixture IDs and one complete
fixture group. Changes to a binding run its clean package install test. Changes
to parsers, rewriting, manifests, or protocol messages run fuzz smoke.

The review result names the validated contract, commands, platform, browser
revision, and any environment limit.
