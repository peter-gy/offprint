# Configure Offprint

Offprint combines built-in defaults, TOML configuration, environment
variables, and command flags into one capture profile. Use an explicit
configuration file when a repository or service owns the settings.

```toml
default_profile = "research"

[browser]
channel = "auto"
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

Apply and inspect the profile:

```console
offprint doctor --config offprint.toml
offprint capture https://example.com \
  --config offprint.toml \
  --profile research \
  --output example.html
```

`doctor --json` shows each reported value with its provenance.

## Precedence

Later sources override earlier sources:

1. Built-in defaults and built-in profile
2. User configuration at `offprint/config.toml` in the platform configuration
   directory
3. The file selected by `--config` or `OFFPRINT_CONFIG`
4. `OFFPRINT_*` environment variables
5. Command flags

Profile selection follows `--profile`, `OFFPRINT_PROFILE`, the explicit
configuration `default_profile`, the user configuration `default_profile`,
then `default`.

Configuration files must be regular UTF-8 TOML files no larger than 1 MiB.
Unknown keys and invalid values fail before browser startup.

## Built-in profiles

| Profile | Missing resources | Network policy | Verification |
| --- | --- | --- | --- |
| `default` | `warn` | `standard` | `offline` |
| `strict` | `fail` | `standard` | `offline` |
| `server` | `fail` | `server` | `offline` |

Define `[profile.NAME]` to add a profile. A custom profile starts from the
normal defaults, then applies the matching user and explicit configuration
sections.

## Browser settings

| Key | Values or shape | Default |
| --- | --- | --- |
| `browser.channel` | `auto`, `managed`, `system` | `auto` |
| `browser.installation` | `explicit`, `install-managed` | `install-managed` |
| `browser.path` | Chrome or Chromium executable path | discovery |
| `browser.cdp_url` | HTTP or WebSocket CDP endpoint | unset |
| `browser.cache_dir` | Managed browser cache directory | platform cache |
| `browser.headless` | Boolean | `true` |
| `browser.maximum_contexts` | Positive integer | `4` |

Use `channel = "managed"` to require the trusted managed build. Use
`installation = "explicit"` when provisioning belongs to the surrounding
system. A remote CDP endpoint requires `network_policy = "unrestricted"` and
`verification = "static"`. The caller then owns endpoint trust, browser state,
network controls, and browser lifecycle.

## Capture profile settings

| Key | Values or shape | Default |
| --- | --- | --- |
| `verification` | `static`, `offline` | `offline` |
| `missing_resources` | `warn`, `fail` | `warn` |
| `network_policy` | `standard`, `server`, `unrestricted` | `standard` |
| `preserve_password_values` | Boolean | `false` |
| `scope` | `page`, `selection` | `page` |
| `selector` | CSS selector for the first top-level match | unset |
| `allowed_file_roots` | Array of permitted roots for `file:` capture | empty |

`selector` selects page scope and replaces an active selection request. Set
`missing_resources = "fail"` when every render-affecting resource is required
for a successful capture.

Optimization keys live under `[profile.NAME.optimizations]`:

| Key | Default |
| --- | --- |
| `remove_unused_css` | `false` |
| `remove_unused_fonts` | `false` |
| `remove_hidden_elements` | `false` |

## Browser environment and readiness

Environment keys live under `[profile.NAME.environment]`:

| Key | Default |
| --- | --- |
| `viewport` | `{ width = 1440, height = 900, scale = 1 }` |
| `locale` | `"en-US"` |
| `timezone` | `"UTC"` |
| `color_scheme` | `"light"` |
| `reduced_motion` | `"reduce"` |

Readiness keys live under `[profile.NAME.readiness]`:

| Key | Values or default |
| --- | --- |
| `mode` | `render-idle`, `network-idle`, `load`, `dom-content-loaded`. Default `render-idle` |
| `network_quiet` | Default `500ms` |
| `mutation_quiet` | Default `300ms` |
| `delay` | Default `0` milliseconds |
| `lazy_load` | `viewport-sweep` or `disabled`. Default `viewport-sweep` |

Duration values accept an integer number of milliseconds or a positive string
with `ms`, `s`, `m`, or `h`.

## Resource limits

Limit keys live under `[profile.NAME.limits]`:

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
| `css_import_depth` | `64` |
| `frame_depth` | `64` |

Byte values accept an integer number of bytes or a string with `B`, `KiB`,
`MiB`, or `GiB`. Lower `concurrent_resources` and
`browser.maximum_contexts` when a service must operate under a small file
descriptor budget.

## Environment variables

| Area | Variables |
| --- | --- |
| Config and profile | `OFFPRINT_CONFIG`, `OFFPRINT_PROFILE` |
| Browser | `OFFPRINT_BROWSER_PATH`, `OFFPRINT_CDP_URL`, `OFFPRINT_CACHE_DIR`, `OFFPRINT_BROWSER_CHANNEL`, `OFFPRINT_BROWSER_INSTALLATION`, `OFFPRINT_HEADLESS` |
| Environment | `OFFPRINT_VIEWPORT`, `OFFPRINT_LOCALE`, `OFFPRINT_TIMEZONE`, `OFFPRINT_COLOR_SCHEME` |
| Readiness | `OFFPRINT_TIMEOUT`, `OFFPRINT_WAIT_UNTIL`, `OFFPRINT_DELAY` |
| Capture | `OFFPRINT_MISSING_RESOURCES`, `OFFPRINT_SCOPE`, `OFFPRINT_SELECTOR`, `OFFPRINT_REMOVE_UNUSED_CSS`, `OFFPRINT_REMOVE_UNUSED_FONTS`, `OFFPRINT_REMOVE_HIDDEN_ELEMENTS` |
| Security and verification | `OFFPRINT_VERIFY`, `OFFPRINT_NETWORK_POLICY`, `OFFPRINT_HEADERS`, `OFFPRINT_COOKIES` |

Boolean environment values accept `1`, `true`, or `yes` and `0`, `false`, or
`no`.

Offprint validates every `OFFPRINT_*` name before command dispatch. A misspelled
or unknown name returns `offprint.config.field` so automation cannot silently
fall back to a default.

## Credential inputs

Headers accept a JSON object or a list of `{ "name", "value" }` objects.
Cookies accept a list or an object with a `cookies` list:

```json
{
  "cookies": [
    {
      "name": "session",
      "value": "secret",
      "url": "https://example.com/",
      "secure": true,
      "httpOnly": true,
      "sameSite": "lax"
    }
  ]
}
```

Credential files must be regular files no larger than 1 MiB. Protect them
before capture:

```console
chmod 600 cookies.json
offprint capture https://example.com/account \
  --cookies cookies.json \
  --output account.html
```

Header and cookie inputs cannot both consume stdin. Raw artifact output cannot
share stream mode with credential input. Effective configuration records
credential paths as redacted values.

Authenticated artifacts can contain private source data. Apply the source
system's storage, retention, and sharing controls to every derived artifact.
