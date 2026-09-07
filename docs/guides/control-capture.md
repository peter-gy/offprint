# Control what Offprint captures

Start from the default capture, then change the browser condition, document
scope, or fidelity policy that matches the page.

```console
offprint capture https://example.com --output example.html
```

## Choose when collection begins

The readiness mode controls when Offprint stops waiting and begins collecting
the rendered state.

| Mode | Condition before collection |
| --- | --- |
| `render-idle` | Document completion, bounded lazy-load sweep, bounded font and DOM-mutation observations, and stable finite network activity |
| `network-idle` | DOM content loaded, then zero finite requests for the network quiet window |
| `load` | Browser load event |
| `dom-content-loaded` | Browser DOM content loaded event |

The [Document Object Model (DOM)](https://dom.spec.whatwg.org/) is the browser's
live tree for the page.
`network-idle` excludes WebSocket, EventSource, `blob:`, and `data:` lifetimes
from its finite request count.

The font and mutation probes have internal deadlines. `render-idle` records
`fontsReady` and `mutationQuiet` in readiness evidence and can continue when a
probe reports false. The overall capture deadline still bounds the operation.

Use `render-idle` for the broad default. Use `network-idle` when all relevant
requests finish. Add a bounded delay when a worker updates the DOM after the
chosen condition:

```console
offprint capture https://example.com/app \
  --wait-until network-idle \
  --delay 1s \
  --timeout 2m \
  --output app.html
```

Readiness work and delay share the total timeout. Navigation does not retry
automatically.

## Choose document scope

The default `page` scope captures the complete top-level page and its frames.

Use a CSS selector to keep the document head, the first matching top-level
element, and its ancestor chain:

```console
offprint capture https://example.com/article \
  --selector "main article" \
  --output article.html
```

Invalid syntax returns `offprint.selector.invalid`. No match returns
`offprint.selector.not_found`. A selector does not search inside a frame.

Use `--scope selection` to capture the browser's active, non-collapsed text or
element selection in the top-level document. An empty selection returns
`offprint.selection.empty`. Selector capture and active-selection capture are
mutually exclusive. In fluent APIs, the setter called last replaces the other
choice.

## Choose missing-resource behavior

The default `warn` policy commits a self-contained artifact with an inert
fallback and a failed resource record. Choose `fail` when every reference is
required:

```console
offprint capture https://example.com \
  --missing-resources fail \
  --output complete.html
```

See [resources and fidelity](../concepts/resources-and-fidelity.md) before
interpreting self-containment as complete acquisition.

## Reduce captured content

Three opt-in transformations can reduce output:

| Option | Action | Fidelity risk |
| --- | --- | --- |
| `--remove-unused-css` | Removes rules that cannot match the captured state | Later state changes cannot use removed rules |
| `--remove-unused-fonts` | Removes font faces unused by the captured state | Later text or style changes may fall back |
| `--remove-hidden-elements` | Removes elements whose computed display is `none` | Hidden content cannot be revealed from the artifact |

These transformations use the observed capture state. Compare the result with
the default artifact before applying them to an archival workflow.

## Capture local files

`file:` capture requires an allowed root in a capture profile:

```toml
[profile.local]
allowed_file_roots = ["/absolute/path/to/site"]
```

```console
offprint capture file:///absolute/path/to/site/index.html \
  --config offprint.toml \
  --profile local \
  --output site.html
```

Offprint resolves paths and rejects files outside the configured roots.
Remote-browser capture cannot address files on the coordinator host.

## Diagnose the rendered state

Pass `--headed` to display a locally launched browser while diagnosing
readiness or selection. A shared service cannot mix incompatible headed state
or browser selections while its process is active. Close the idle browser or
use separate services when those choices must differ.

Use [configuration](../reference/configuration.md) to preserve these decisions
in a named capture profile.
