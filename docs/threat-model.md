# Security threat model

PageKnot executes a web page in Chromium, reads browser state through CDP,
retrieves render-affecting resources, and writes a portable artifact. The
default workflow assumes the page and every value derived from it are
untrusted.

## Assets

- Local files and services reachable from the capture host
- Authentication headers, cookies, CDP endpoint credentials, and URL secrets
- Requested output files and neighboring filesystem entries
- Browser cache, managed browser installations, and process ownership
- Host memory, disk, CPU, file descriptors, and network capacity
- Artifact integrity, provenance, and offline behavior

## Trust boundaries

1. The CLI and language bindings convert caller input into canonical records.
2. `NetworkGuard` approves the initial URL, each redirect, and each resolved
   address.
3. Chromium runs page code inside an isolated browser context.
4. The collector sends versioned, checksummed, bounded chunks through CDP.
5. Resource bytes enter a bounded content store.
6. The document transformer removes active content and embeds controlled
   resource outcomes.
7. The verifier reopens the staged artifact with external network access
   denied.
8. The artifact transaction commits the verified staging file.

## Threats and controls

| Threat | Control | Release evidence |
| --- | --- | --- |
| Server-side request forgery | Scheme policy, DNS result classification, redirect revalidation, private-address policy | network policy tests and DNS fixtures |
| DNS rebinding | Address validation immediately before navigation and resource fetch | resolved-address tests |
| Credential disclosure | Typed secret wrappers, URL redaction, header filtering, sanitized diagnostics | redaction and diagnostic tests |
| Captured script execution | Script and event-handler sanitization, safe-static CSP, offline browser verification | static verifier and execution fixtures |
| Artifact network access | Controlled URL rewriting, data embedding, denied-network reopen | resource and offline fixtures |
| Path replacement or symlink attack | Direct-parent symlink rejection, same-directory staging, conflict policy, atomic rename | transaction failure matrix |
| Managed browser substitution | Trusted catalog, exact SHA-256 verification, version probe after extraction | managed installer tests |
| Archive traversal | Normalized entry validation and extraction under a private staging directory | managed archive tests |
| Browser process leak | Process-tree guard, owned leases, deterministic close, crash recovery | lifecycle and repeated-capture tests |
| Unbounded page data | Duration, frame, node, chunk, resource, total-byte, and artifact limits | oversized and budget tests |
| Protocol confusion | Version handshake, capture and frame ownership, sequence checks, CRC32 and SHA-256 | protocol tests and fuzz targets |
| Diagnostic persistence of secrets | Bounded structured payload built from redacted fields | diagnostic secret audit |
| Malformed artifact acceptance | Manifest, resource digest, CSP, structure, URL, and network checks | verifier tests and manifest fuzzing |

## Browser and host assumptions

The Chromium sandbox, operating-system process isolation, and TLS stack remain
part of the trusted computing base. A caller that supplies a remote CDP
endpoint grants PageKnot control of that browser and owns its network controls,
process lifecycle, and browser-side state. Remote capture requires an
unrestricted network policy and static verification. A locally modified
browser binary outside the managed catalog inherits the trust decision of the
caller.

Artifacts can reproduce deceptive visual content. Verification proves
self-containment and structural policy compliance. It does not certify the
truth or safety of the captured page.

## Security response

Security-sensitive failures use stable `pageknot.*` error codes and avoid
embedding attacker-controlled secrets in diagnostics. Reports should include
the PageKnot version, platform, managed browser revision, error code, and a
minimal local fixture.
