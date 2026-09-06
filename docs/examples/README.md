# Executable capture scenarios

Offprint's hermetic browser fixtures are small rendered pages with named
artifact expectations. Each scenario maps input state to a capture decision and
observable result.

These are repository evidence scenarios. Install the contributor tools through
[Contributor setup](../../development_docs/setup.md) before running `just`.
Use the [quickstart](../start/quickstart.md) to produce an artifact through the
end-user CLI path.

Run one scenario from the repository root:

```console
just test-fixture script-rendered-document
```

The fixture selector resolves exactly one test owner and runs it with one test
thread.

## Script-rendered content

Starting page state:

```html
<h1 id="state">waiting</h1>
<script>
  document.querySelector("#state").textContent = "rendered";
</script>
```

After capture, the artifact contains the rendered heading text and excludes the
captured page script. Static verification accepts only the Offprint-owned
restoration programs. Offline verification reopens the artifact with zero
observed requests.

Evidence: `script-rendered-document` in
[`fixtures.json`](../../fixtures/manifest/fixtures.json) and the browser fixture
matrix in [`fixture_matrix.rs`](../../crates/offprint/tests/fixture_matrix.rs).

## Selected content

Starting page state contains content inside and outside a target element. A CSS
selector capture keeps the document head, matching element, and ancestor chain.
Run the hermetic selection scenario:

```console
just test-fixture selection-scope
```

The resulting artifact retains target styles and omits sibling body content.
The same fixture also checks active document selection and empty-selection
failure.

Evidence: `selection-scope` in the fixture catalog and its executable owner in
[`fixture_matrix.rs`](../../crates/offprint/tests/fixture_matrix.rs).

## Frames and shadow roots

These scenarios vary one browser boundary at a time:

| Scenario | Preserved state |
| --- | --- |
| `same-origin-iframe` | Child document and resource references |
| `cross-origin-oopif` | Out-of-process frame ownership and content |
| `srcdoc-frame` | Inline frame content |
| `open-shadow-root` | Declarative shadow content |
| `closed-shadow-root` | Closed root observed by the document-start hook |
| `adopted-stylesheet` | Stylesheet content in its owning shadow scope |

```console
just test-fixture cross-origin-oopif
just test-fixture closed-shadow-root
```

## Resource fidelity

`external-stylesheet-resources`, `css-imports`, `font-loading`, `blob-resource`,
`data-resource`, and `svg-resource-graph` verify recursive resource discovery
and embedding.

`missing-resource-deduplication` shows the warning path. Each failed reference
receives a failed resource record and inert fallback. Repeated attempts for the
same unavailable resource stop after the first acquisition batch.

```console
just test-fixture missing-resource-deduplication
```

## Browser state and visual fallbacks

| Scenario | Observable artifact state |
| --- | --- |
| `form-state` | Current values, checks, selections, and disclosure state |
| `rendered-view-state` | Document and element scroll positions |
| `canvas-2d` | Canvas pixels encoded as image data |
| `tainted-canvas` | Clipped browser screenshot fallback |
| `webgl-canvas` | Composited WebGL pixels |
| `video-poster` | Media poster or current-frame fallback |

These scenarios use DOM assertions and controlled pixel comparison where visual
state carries the contract.

## Commit and verification

- `explicit-output-conflict` preserves an existing destination.
- `atomic-replacement` exposes the new file after verification.
- `cancellation-every-stage` rolls back each pipeline stage.
- `network-denied-reopen` requires zero requests from the reopened artifact.
- `cli-committed-path` checks the command output boundary.

```console
just test-fixture atomic-replacement
just test-fixture network-denied-reopen
```

Use the [capture model](../concepts/capture-model.md) to interpret the lifecycle
and [resources and fidelity](../concepts/resources-and-fidelity.md) to interpret
resource records and warnings.
