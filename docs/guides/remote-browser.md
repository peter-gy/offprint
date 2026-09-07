# Use a remote Chromium browser

Offprint can capture through a browser running on another host. It connects
through the [Chrome DevTools Protocol (CDP)](https://chromedevtools.github.io/devtools-protocol/),
the browser's automation interface. The caller owns the browser process and
network controls. Remote captures use static verification.

```console
offprint capture https://example.com \
  --cdp-url https://browser.example/cdp \
  --network-policy unrestricted \
  --verification static \
  --output example.html
```

The endpoint can use HTTP, HTTPS, WebSocket, or secure WebSocket. HTTP discovery
tries the standard version endpoint, then the target list, then a direct
WebSocket URL. This connection bypasses capture `NetworkPolicy` and is trusted
coordinator egress. Never accept an endpoint from an untrusted tenant.

Endpoint user information and common secret query keys are redacted. Unknown
query names can remain visible, so keep credentials in recognized secret fields
and review reports before sharing them.

## Required contract

Remote capture requires:

- HTTP or HTTPS page input
- `NetworkPolicy::Unrestricted`
- `VerificationMode::Static`
- Permission to create and dispose browser contexts
- Runtime and page-domain access required by the collector

The `unrestricted` policy permits public, private, link-local, loopback, and
otherwise non-routable address classes. Offprint still blocks page-created
WebSocket, EventSource, and WebRTC transports in managed page and frame targets.
This is not a whole-browser firewall.

## Ownership

Offprint creates and disposes the contexts and pages used for its captures. The
caller owns:

- Endpoint authentication and transport trust
- Browser process startup and shutdown
- Browser-side persistent state
- Network enforcement outside Offprint's page containment
- Availability and concurrency limits of the remote service

The remote browser operator can observe source URLs, injected headers, cookies,
browser requests, and rendered private content through CDP. Use an endpoint
whose operator is permitted to receive the captured data.

Closing Offprint closes its protocol connection. It leaves the remote browser
process running.

## Supported operations

One-page capture and batch can use a remote browser when each affected request
satisfies the remote contract. A batch-level endpoint replaces `auto` browser
selection and preserves explicit per-request choices. One service owns one
active browser identity. Mixing endpoints or local and remote selections while
that identity is active can fail with `offprint.browser.selection_conflict`.

Crawl, coordinator-local `file:` input, and offline verification require a
local browser. Use the resulting static verification report as the supported
evidence boundary.

`doctor` can connect to the endpoint and create a temporary context while
probing collector compatibility. Its `ready` field describes the default local
offline path and can remain false for a working remote static configuration.
