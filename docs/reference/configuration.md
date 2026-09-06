# Configuration reference

Offprint resolves browser settings and a named capture profile before command
dispatch. Use an explicit [TOML](https://toml.io/en/) configuration file when a
repository or service owns the settings.

```toml
default_profile = "research"

[browser]
source = "auto"
installation = "install-managed"
headless = true
maximum_contexts = 4

[profile.research]
verification = "offline"
missing_resources = "fail"
network_policy = "standard"

[profile.research.environment]
viewport = { width = 1440, height = 900, scale = 1 }
locale = "en-US"
timezone = "UTC"
color_scheme = "light"
reduced_motion = "reduce"

[profile.research.readiness]
mode = "render-idle"
network_quiet = "500ms"
mutation_quiet = "300ms"
lazy_load = "viewport-sweep"

[profile.research.limits]
duration = "2m"
frames = 256
resources = 10000
resource_bytes = "64MiB"
total_resource_bytes = "512MiB"
concurrent_resources = 8
artifact_bytes = "64MiB"
```

```console
offprint doctor --config offprint.toml
offprint capture https://example.com \
  --config offprint.toml \
  --profile research \
  --output example.html
```

## Precedence

Later sources override earlier sources:

1. Built-in defaults and built-in profile
2. User configuration in the platform configuration directory
3. File selected by `--config` or `OFFPRINT_CONFIG`
4. `OFFPRINT_*` environment variables
5. Command flags

Profile selection follows `--profile`, `OFFPRINT_PROFILE`, explicit
`default_profile`, user `default_profile`, then `default`.

The user configuration file is `config.toml` under the platform configuration
directory:

| Platform | Default path |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/offprint/config.toml`, or `$HOME/.config/offprint/config.toml` when `XDG_CONFIG_HOME` is unset |
| macOS | `$HOME/Library/Application Support/offprint/config.toml` |
| Windows | `%APPDATA%\offprint\config\config.toml` |

Use `--config PATH` or `OFFPRINT_CONFIG=PATH` when the owning application needs
an explicit, portable location.

Configuration files must be regular UTF-8 TOML files no larger than 1 MiB.
Unknown keys and invalid values fail before command dispatch. Offprint does not
load project-directory configuration automatically, which prevents an
untrusted checkout from changing browser or network policy.

`doctor --json` reports selected operational values with provenance. The report
is not a complete round-trip representation of every default, and its field
paths are diagnostic names rather than canonical TOML keys.

## Built-in capture profiles

| Profile | Missing resources | Network policy | Verification |
| --- | --- | --- | --- |
| `default` | `warn` | `standard` | `offline` |
| `strict` | `fail` | `standard` | `offline` |
| `server` | `fail` | `server` | `offline` |

Define `[profile.NAME]` to add a profile. A custom profile begins with the
normal defaults, then receives user and explicit file patches. In fluent Rust
APIs, applying a profile replaces environment, readiness, content, network,
limits, and verification values already set on the pending capture. Set
per-capture overrides after applying the profile.

## Browser settings

| Key | Type or values | Default |
| --- | --- | --- |
| `browser.source` | `auto`, `managed`, `system` | `auto` |
| `browser.installation` | `existing-only`, `install-managed` | `install-managed` |
| `browser.path` | Executable path | Discovery |
| `browser.cdp_url` | HTTP, HTTPS, WS, or WSS URL | Unset |
| `browser.cache_dir` | Directory | Platform cache |
| `browser.headless` | Boolean | `true` |
| `browser.maximum_contexts` | Positive integer | `4` |

Remote CDP requires `network_policy = "unrestricted"` and
`verification = "static"`. The caller owns remote endpoint and process trust.

## Capture profile

| Key | Type or values | Default |
| --- | --- | --- |
| `verification` | `static`, `offline` | `offline` |
| `missing_resources` | `warn`, `fail` | `warn` |
| `network_policy` | `standard`, `server`, `unrestricted` | `standard` |
| `preserve_password_values` | Boolean | `false` |
| `scope` | `page`, `selection` | `page` |
| `selector` | Top-level CSS selector | Unset |
| `allowed_file_roots` | Array of absolute roots | Empty |

Top-level `headers` and `cookies` keys can reference credential JSON files.
Credential values remain outside TOML.

Optimization keys under `[profile.NAME.optimizations]` default to `false`:

- `remove_unused_css`
- `remove_unused_fonts`
- `remove_hidden_elements`

## Browser environment

Keys under `[profile.NAME.environment]`:

| Key | Default |
| --- | --- |
| `viewport` | `{ width = 1440, height = 900, scale = 1 }` |
| `locale` | `"en-US"` |
| `timezone` | `"UTC"` |
| `color_scheme` | `"light"` |
| `reduced_motion` | `"reduce"` |

The full Rust request also supports browser-default or overridden user-agent
policy.

## Readiness

Keys under `[profile.NAME.readiness]`:

| Key | Values or default |
| --- | --- |
| `mode` | `render-idle`, `network-idle`, `load`, `dom-content-loaded`. Default `render-idle` |
| `network_quiet` | `500ms` |
| `mutation_quiet` | `300ms` |
| `delay` | `0ms` |
| `lazy_load` | `viewport-sweep`, `disabled`. Default `viewport-sweep` |

Integer durations use milliseconds. String durations use `ms`, `s`, `m`, or
`h`. A suffixless string is interpreted as seconds.

## Limits

Keys under `[profile.NAME.limits]`:

| Key | Default |
| --- | ---: |
| `duration` | `2m` |
| `redirects` | `20` |
| `frames` | `256` |
| `nodes` | `1000000` |
| `resources` | `10000` |
| `resource_bytes` | `64MiB` |
| `total_resource_bytes` | `512MiB` |
| `collector_chunk_bytes` | `1MiB` |
| `concurrent_resources` | `8` |
| `artifact_bytes` | `64MiB` |
| `resource_recursion_depth` | `64` |
| `frame_depth` | `64` |

Byte values accept integers or strings using `B`, `KiB`, `MiB`, or `GiB`.
Every limit except `redirects` must be greater than zero. Set `redirects = 0`
to reject the first redirect. `total_resource_bytes` must be at least
`resource_bytes`. The compiled node maximum is one million.

## Environment variables

| Area | Variables |
| --- | --- |
| Configuration | `OFFPRINT_CONFIG`, `OFFPRINT_PROFILE` |
| Browser | `OFFPRINT_BROWSER_PATH`, `OFFPRINT_CDP_URL`, `OFFPRINT_CACHE_DIR`, `OFFPRINT_BROWSER_SOURCE`, `OFFPRINT_BROWSER_INSTALLATION`, `OFFPRINT_HEADLESS` |
| Browser environment | `OFFPRINT_VIEWPORT`, `OFFPRINT_LOCALE`, `OFFPRINT_TIMEZONE`, `OFFPRINT_COLOR_SCHEME` |
| Readiness | `OFFPRINT_TIMEOUT`, `OFFPRINT_WAIT_UNTIL`, `OFFPRINT_DELAY` |
| Content | `OFFPRINT_MISSING_RESOURCES`, `OFFPRINT_SCOPE`, `OFFPRINT_SELECTOR`, `OFFPRINT_REMOVE_UNUSED_CSS`, `OFFPRINT_REMOVE_UNUSED_FONTS`, `OFFPRINT_REMOVE_HIDDEN_ELEMENTS` |
| Trust | `OFFPRINT_VERIFY`, `OFFPRINT_NETWORK_POLICY`, `OFFPRINT_HEADERS`, `OFFPRINT_COOKIES` |

Boolean values accept `1`, `true`, or `yes`, and `0`, `false`, or `no`.
Unknown `OFFPRINT_*` names return `offprint.config.field` so automation cannot
silently fall back to defaults.

Batch requests own complete capture settings. Most capture-profile environment
variables do not rewrite requests loaded from a batch file.
