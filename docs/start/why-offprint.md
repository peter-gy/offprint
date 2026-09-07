# Why Offprint?

Offprint preserves rendered content, packages its rendering dependencies, and
checks the result before saving it. These guarantees support research records,
reviewable reports, regression evidence, and page inputs to a larger archive.

## Preserve the rendered state

A page may start as an empty application shell, fetch data, and draw a chart
into a canvas. Its initial HTML response cannot represent that final state.
Saving the live [Document Object Model](https://dom.spec.whatwg.org/), the
browser's document tree, still leaves fonts, images, stylesheets, and frames on
the network.

Offprint observes the rendered page and resolves each rendering resource. It
materializes captured controls and disclosure state, embeds resource bytes,
and freezes canvas or media content into visual fallbacks. The saved page
represents the state at capture time. Continued application interaction or
live data updates require the original application.

## Check what was saved

A file can contain every expected element and still request a font, image, or
frame when reopened. Offprint's default offline verification checks its static
format and reopens the artifact with network access denied.

| Question                                          | Evidence                                                            |
| ------------------------------------------------- | ------------------------------------------------------------------- |
| Did the artifact pass the selected checks?        | Verification report bound to the exact file digest                  |
| Were rendering resources unavailable?             | Resource counts and warnings in the capture receipt                 |
| Which reference failed, and why?                  | Resource records in the embedded manifest                           |
| Which browser and capture conditions produced it? | Browser, environment, source, and policy provenance in the manifest |

Self-containment and complete acquisition are separate properties. A failed
resource becomes an inert fallback under the default warning policy, so the
artifact can pass verification while losing visual content. Use
`--missing-resources fail` when that loss should reject the capture.

## Save after verification

Offprint stages the artifact and verifies it before committing the destination.
The default conflict policy preserves an existing file. Explicit replacement
installs the new artifact after it passes verification. Cancellation before
commit leaves the destination unchanged.

The [quickstart](./quickstart.md) produces one artifact. The
[capture model](../concepts/capture-model.md) explains jobs, cancellation,
receipts, and service lifetime for application integrations.
