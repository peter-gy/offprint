# Review checklist

Review from the user boundary toward the implementation.

## Contract

1. Identify the affected command, service method, record, artifact field, or
   binding method.
2. Compare Rust, CLI, Node.js, Python, schemas, examples, and docs.
3. Check defaults, field names, enum values, stdout, stderr, JSON, and exit
   status.
4. Confirm every resource reference receives one terminal record.

## Lifecycle

Trace success, failure, cancellation, dropped consumers, shutdown, and host
exit. Inspect every await between acquisition and owner-guard construction.
Confirm disconnects fail pending commands promptly.

## Security

Check URL resolution, redirect and Domain Name System validation, secret
redaction, local file roots, archive extraction, symbolic links, output
replacement, content security policy, captured-script removal, exact owned
scripts, and offline verification.
Apply limits before allocation and accumulation.

## Generated contracts

```console
just codegen-check
```

Generated changes need an owning source change. Review JSON examples and host
declarations beside schema changes.

## Validation

Use the focused command in [Testing](./testing.md). Browser behavior also runs
the affected fixture and one complete group. Binding changes run the extracted
package capture. Parser, resource, serializer, manifest, and protocol changes
run fuzz smoke.

The handoff names the contract, commands, platform, browser revision, and any
environment limit.
