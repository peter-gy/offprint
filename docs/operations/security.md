# Security and trust boundaries

Offprint executes untrusted page code in Chromium, reads rendered state through
the Chrome DevTools Protocol, retrieves render dependencies, and writes a
portable artifact. Treat page input, browser responses, artifacts, credentials,
diagnostics, and remote endpoints as untrusted.

## Safe capture baseline

Use the default `standard` network policy and `offline` verification for public
pages:

```console
offprint capture https://example.com \
  --network-policy standard \
  --verification offline \
  --output example.html
```

`standard` permits public addresses. When the seed URL uses a literal loopback
address or exact `localhost`, it also permits loopback destinations with the
same host, including another port. It
blocks private, link-local, metadata, and other non-public destinations.
Redirects and resolved addresses are revalidated.

## Network policies

| Policy | Address behavior | Typical use |
| --- | --- | --- |
| `standard` | Public addresses, plus same-host loopback from a loopback seed | Interactive capture |
| `server` | Public addresses | Untrusted server workloads |
| `unrestricted` | Every address class | Explicit remote or private-network integration |
| Custom rules | Public plus named hosts, [CIDR](https://www.rfc-editor.org/rfc/rfc4632) address ranges, or enabled classes | Controlled service allowlist |

Complete requests use the same canonical camelCase record in Rust JSON,
Node.js, and Python:

```json
{
  "network": {
    "kind": "custom",
    "rules": {
      "allowedHosts": ["capture.internal"],
      "allowedCidrs": ["10.20.0.0/16"],
      "allowLoopback": false,
      "allowPrivate": false,
      "allowLinkLocal": false
    }
  }
}
```

Rust constructs a custom policy with `NetworkRules`:

```rust
use std::collections::BTreeSet;

use offprint::{NetworkPolicy, NetworkRules, Offprint};

async fn configure() -> offprint::Result<()> {
    let network = NetworkPolicy::Custom(NetworkRules {
        allowed_hosts: BTreeSet::from(["capture.internal".to_owned()]),
        allowed_cidrs: BTreeSet::from(["10.20.0.0/16".to_owned()]),
        allow_loopback: false,
        allow_private: false,
        allow_link_local: false,
    });

    let offprint = Offprint::new()?;
    let capture = offprint.capture("https://example.com")?.network(network);
    drop(capture);
    offprint.close().await?;
    Ok(())
}
```

Public addresses remain permitted. `allowed_hosts` permits every resolved
address for an exact, case-insensitive host-name match, including private and
loopback addresses. Trust the named host's DNS operator when using this option.
Use `allowed_cidrs` to permit specific address ranges. The three switches permit
every address in their respective loopback, private, or link-local class. All
answers for a resolved host must pass the resulting policy.
The compiling source is
[`custom_network.rs`](../../crates/offprint/examples/custom_network.rs).

Every [Domain Name System (DNS)](https://www.rfc-editor.org/rfc/rfc1034) address
answer must pass the selected policy. A mixed public and private
answer is rejected unless the policy permits both address classes. The
validating proxy connects to an address it already classified to reduce DNS
rebinding risk.

The network policy controls address and redirect classification. Local launch
also disables direct WebRTC UDP. Remote capture blocks WebSocket, EventSource,
and WebRTC constructors in managed page and frame targets. These controls are
page containment, not a whole-browser firewall.

## Browser boundary

Each capture uses a fresh isolated capture context. Offline verification uses a
separate verification context. Local launch keeps browser sandboxing enabled,
disables downloads, uses an ephemeral user-data directory, and owns the process
tree.

A caller-selected local binary inherits the caller's trust decision. A remote
endpoint gives Offprint control of that browser and transfers process,
transport, persistent-state, and surrounding network ownership to the caller.

## Credential boundary

Load headers and cookies from protected files or standard input. Offprint
redacts credential values and URL user information. Its URL policy redacts
common secret query keys, but an unfamiliar query key can remain visible in
logs, JSON errors, source summaries, manifests, and diagnostics. Keep secrets
in recognized credential fields and review reports before sharing them.
Same-origin header scoping and cookie domain rules still determine which
resources authenticate.

The artifact can contain private rendered content even though credentials are
redacted. Review storage and sharing separately from secret handling.

## Safe-static boundary

Canonical Offprint HTML removes captured page scripts, event handlers,
automatic refresh, and request-triggering references. It permits exact
Offprint-owned restoration programs when captured state or structural repair
requires them. Static verification checks those bytes and the matching content
security policy.

Offline verification opens the staged artifact with network access denied and
rejects requests, page errors, or unstable loading. Verification establishes
the Offprint artifact contract. It does not certify source truth, prevent
deceptive visual content, or make sensitive content public.

Offline verification observes initial loading. It does not click retained
links. User-activated links can navigate away from the local artifact, so review
destinations before interaction.

Self-extracting HTML is an export with an executable decompression loader. It
uses a different execution boundary from canonical safe-static HTML.

## Filesystem boundary

- Local `file:` capture requires explicit allowed roots.
- Managed-browser extraction rejects traversal and unsafe entry types.
- Direct artifact and credential paths reject symbolic-link ambiguity.
- File output stages beside the destination.
- Conflict checks and verification complete before commit.
- Multi-output export records a recovery journal when entry-wise mutation is
  required.

Preserve recovery paths reported by `offprint.output.*` errors.

## Resource bounds

Duration, redirects, frames, nodes, resource references, individual and total
resource bytes, collector chunks, concurrent streams, artifact bytes, CSS
depth, and frame depth are bounded before or during accumulation. See
[limits and performance](./limits-and-performance.md) for accounting semantics.

## Report a security issue

Include the Offprint version, platform, browser product and version, managed
revision when applicable, stable error code, and the smallest local fixture.
Review doctor reports and diagnostic bundles for private paths and captured
content before sharing them. Submit sensitive reports through a private
[GitHub security advisory](https://github.com/peter-gy/offprint/security/advisories/new).
