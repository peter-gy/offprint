# Release contract

Offprint publishes Rust crates, native CLI archives, one npm package with four
native addons, and Python distributions from one signed version tag. The tag
version must match `versions.toml`, the Cargo workspace, npm package, and Python
package metadata.

The version gate also compares the pinned Rust toolchain, the Rust and
TypeScript collector protocol declarations, and every managed-browser catalog
revision and archive digest with `versions.toml`.

Push `v<version>` to run [`.github/workflows/publish.yml`](../.github/workflows/publish.yml).
The tag version must match the package metadata exactly.

Run the local release gate before creating the tag:

```console
just release-check
```

## Rust crates

The supported Rust API surface is `offprint`, `offprint-model`, and
`offprint-cli`. The `offprint` dependency graph also publishes its artifact,
browser, capture, Chromium, document, export, HTML, protocol, and transform
crates so Cargo can resolve the public packages from crates.io.

`cargo publish --workspace --locked` computes the dependency order and excludes
the workspace maintenance, binding, benchmark, and test-support packages.

## Native targets

| Target | Archive |
| --- | --- |
| `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `x86_64-apple-darwin` | `.tar.gz` |
| `aarch64-apple-darwin` | `.tar.gz` |
| `x86_64-pc-windows-msvc` | `.zip` |

Each archive contains `offprint`, `README.md`, `LICENSE`,
`build-metadata.json`, and `SHA256SUMS`. CI publishes an outer SHA-256 digest,
an SBOM, `dependency-licenses.json`, and GitHub build provenance.

Each native matrix job builds from the tagged checkout, verifies that
`build-metadata.json` names the tag commit and target, provisions the pinned
browser in a fresh cache, then captures and offline-verifies a fixture through
the extracted executable.

## npm

The `offprint` npm package contains the native addons for every supported
target. The package loader selects the addon for the current operating system
and CPU architecture.

The [npm trusted publisher](https://docs.npmjs.com/trusted-publishers/)
configuration names `publish.yml` and the `npm` GitHub environment. The
publish job receives the packaged tarball from an unprivileged build job,
requests an
[OpenID Connect](https://docs.github.com/en/actions/reference/security/oidc)
identity token, and publishes with npm 11.12.1. A fresh Linux installation
captures a fixture from the public registry before the GitHub release is
created. Prerelease versions use the `next` npm tag. Stable versions use
`latest`.

## PyPI

The `offprint` Python distributions contain four ABI3 wheels and one source
distribution. The
[PyPI trusted publisher](https://docs.pypi.org/trusted-publishers/)
configuration names `publish.yml` and the `pypi` GitHub environment. The
publish job downloads the verified distributions and uses
`uv publish --trusted-publishing always` with the PyPI index check. A fresh
Python 3.14 environment installs the public wheel and runs the package
lifecycle smoke tests.

## Release sequence

1. Run `just release-check`.
2. Run every required fixture group with the pinned managed browser.
3. Capture a hermetic page through each extracted CLI archive, packed npm
   package, and installed Python wheel.
4. Confirm `versions.toml`, package manifests, generated schemas, and binding
   versions agree.
5. Build the native matrix from the signed tag.
6. Generate the dependency license report, checksums, SBOM, and provenance.
7. Publish crates.io, npm, and PyPI packages.
8. Install npm and PyPI packages from their public registries and capture the
   static fixture.
9. Publish the verified artifact set as the GitHub release.

Versions with a prerelease suffix create a GitHub prerelease.

The release gate requires all platform, browser, binding, dependency, Miri,
fuzz, sanitizer, repeated-capture, package-capture, and offline-verification
lanes. Scheduled assurance adds Chromium canary, sustained fuzz, differential,
and normalized performance comparison runs.
