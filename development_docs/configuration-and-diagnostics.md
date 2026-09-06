# Configuration and diagnostics

`offprint-cli` resolves runtime browser settings and a selected capture profile
before dispatch. The model stays independent of terminal and platform config
discovery.

## Resolution order

```text
built-in defaults
    -> user configuration
    -> explicit configuration
    -> environment variables
    -> command flags
```

Profile selection resolves flag, environment, explicit default, user default,
then `default`. A profile patch updates verification, resource policy, network,
password handling, scope, selector, file roots, optimizations, browser
environment, readiness, and limits.

Browser path and remote endpoint are mutually exclusive. Later browser sources
replace earlier selection as one unit.

## Trust boundary

The CLI does not load configuration from the current project. User and explicit
files must be regular UTF-8 TOML files within 1 MiB. Unknown keys and unknown
`OFFPRINT_*` environment variables fail before dispatch.

Credential files are resolved separately, validated for direct-file identity,
size, and platform permissions, then parsed into secret wrappers.

## Command scope

Capture, batch, crawl, browser, and HTML-verification commands resolve runtime
configuration. Artifact inspection, shell completion, and format-specific
verification dispatch without the full profile resolution path. Keep command
preflight aligned with the settings each command consumes.

Batch requests own complete capture policy. Runtime resolution supplies browser
defaults and service settings. Crawl applies the selected profile to seed-derived
requests but forces offline verification.

## Effective configuration records

`BrowserDoctorReport.configuration` contains selected values, JSON values,
provenance tier, and a redaction flag. It currently omits many defaults and
records profile-patch provenance as `profile`, losing whether the value came
from user or explicit configuration.

Diagnostic field paths use camelCase while TOML keys use snake_case. They are
not round-trip keys. A future contract should expose canonical TOML paths or a
separate configuration key.

## Diagnostic bundles

Capture diagnostics begin after request validation and diagnostic-directory
setup. Pipeline failures then write a protected unique JSON bundle and attach
its path to `OffprintError`. Validation and diagnostic setup failures have no
capture bundle. Verification diagnostics record failure context when the CLI
verification path has initialized them.

The current capture bundle contains redacted request data, retained events,
dropped-event count, and the structured failure. Verification diagnostics
contain the available verification failure context. Do not promise frame dumps,
resource tables, staged artifacts, protocol logs, or screenshots until the
implementation writes them.

## CLI output ownership

- Command data uses standard output.
- Progress, warnings, and errors use standard error.
- Capture JSON suppresses human progress.
- Batch, crawl, and doctor can emit a value before returning status `1`.
- Verification rejection maps to status `3`, except artifact read and size
  failures, which map to runtime status `1`.

Preserve these boundaries in parser, renderer, integration, and package tests.
