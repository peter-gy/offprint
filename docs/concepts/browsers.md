# Browsers and ownership

Offprint controls Chromium-based browsers through the
[Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/),
the browser's remote inspection and automation protocol. Browser selection,
browser source, discovery policy, and installation policy are separate
concepts.

## Browser selection

A capture request chooses one `BrowserSpec`:

| Selection    | Meaning                                                                  |
| ------------ | ------------------------------------------------------------------------ |
| `auto`       | Use the service's configured discovery path                              |
| `executable` | Launch the named local executable                                        |
| `remote`     | Attach to the named HTTP, HTTPS, WebSocket, or secure WebSocket endpoint |

For automatic local discovery, Offprint considers an explicit service path,
installed managed browsers, then compatible system browsers. The public
`BrowserSourcePolicy` limits discovery to `auto`, `managed`, or `system`.

`BrowserSource` records how the selected browser was obtained: `managed`,
`system`, `explicit`, or `remote`.

## Managed and system browsers

A **managed browser** is a pinned Chrome for Testing revision whose archive
digest appears in Offprint's browser catalog. Installation locks the cache,
downloads from the catalog URL, verifies the archive, validates extraction,
probes the executable version, and commits the staged installation.

`BrowserInstallationPolicy` controls automatic provisioning:

- `install-managed` allows first use to install the pinned managed browser.
- `existing-only` requires a compatible browser to be available already.

System discovery supports Google Chrome, Chromium, and Microsoft Edge based on
Chromium major version 120 or newer. Managed release packages target Linux
x86-64 with glibc, macOS arm64 and x86-64, and Windows x86-64.

## Process and context ownership

One `Offprint` service reuses one local browser process. Each capture receives a
fresh isolated capture context. Offline verification opens a separate
verification context in a compatible service-owned process. Offprint disables
browser downloads and closes each page and context on success, failure, or
cancellation.

A running local process must match the next capture's executable selection and
headed state. Switching those choices while the process is active returns
`offprint.browser.selection_conflict`. Close the idle process or use separate
services for incompatible browser choices.

## Remote browser

Remote capture requires:

- An HTTP or HTTPS page URL
- The `unrestricted` network policy
- Static verification
- A browser endpoint that permits Browser, Target, Page, Runtime, DOM, Log,
  Security, Network, Emulation, and Fetch operations used by capture

`unrestricted` permits every address class. Offprint blocks page-created
WebSocket, EventSource, and WebRTC transports in the managed page and frame
targets. It also rejects coordinator-local `file:` capture. Batch requests can
use a remote browser when each affected request satisfies the remote rules.
Crawl rejects a remote CDP selection and can use the local default or an
injected Rust backend.

Offprint owns the contexts it creates on a remote browser. The caller owns the
endpoint, browser process, browser-side state, credentials, and network
controls. Closing the service closes its protocol connection and leaves the
remote process running.

## Doctor report

`offprint doctor` discovers candidates, probes the selected browser and
collector, checks the managed cache and output directory, and returns recovery
actions. Its `ready` field describes the canonical local offline capture path.
A deliberately configured remote static capture can work even when that field
is false.

The probe can launch a short-lived local browser or create and dispose a remote
context. Review endpoint side effects before running `doctor` against a shared
remote service.

Continue with [managed browser workflows](../guides/manage-browsers.md) or the
[remote browser guide](../guides/remote-browser.md).
