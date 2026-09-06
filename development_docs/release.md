# Release process

Push a signed `v<version>` tag to run
[`publish.yml`](../.github/workflows/publish.yml). The tag must match
`versions.toml`, Cargo, npm, and Python package metadata.

## Release order

1. Validate formatting, architecture, code generation, dependencies, tests,
   documentation, and package graphs.
2. Run serialized browser fixture groups and lifecycle lanes.
3. Build native archives, four npm addons, four Python stable-ABI wheels, and one
   Python source distribution.
4. Capture through native archives, the assembled npm package, and installed
   Python wheels.
5. Build the dependency license report, software bill of materials, checksums,
   and request hosted GitHub build provenance attestations.
6. Publish Rust crates, the `offprint` npm package, and `offprint` Python
   distributions.
7. Build a crates.io consumer, then install exact public npm and PyPI versions
   and run capture lifecycle smoke tests.
8. Create the GitHub release with the verified artifact set.

Registry publication is not atomic across crates.io, npm, and PyPI. A retry
checks each crates.io and npm version and skips an immutable version that is
already present. `uv publish --check-url` skips matching files already present
on PyPI and uploads the remaining distribution set. The workflow then verifies
npm and PyPI installations and builds a fresh crates.io facade consumer. It
builds but does not install the source distribution separately from wheels.

Before retrying a partial publication, inspect registry state:

```console
cargo info offprint@VERSION
npm view offprint@VERSION version dist-tags
curl --fail --silent --show-error \
  https://pypi.org/pypi/offprint/VERSION/json > /dev/null
```

Rerun the workflow for the same signed tag when every published file belongs to
that source revision and the remaining registries are empty or incomplete. The
publish jobs roll forward missing immutable versions and distributions. Create
a corrected source commit and new version when published bytes, metadata,
dependencies, or tags do not match the intended release. Registry versions are
immutable and must not be overwritten.

[npm trusted publishing](https://docs.npmjs.com/trusted-publishers/)
authenticates `npm publish`, not `npm dist-tag`. When an
existing exact version has a stale `latest` or `next` tag, the workflow fails
closed. Repair the tag through an authorized human npm session after verifying
the published package, or publish a corrected version when the package itself
is wrong.

Prerelease product versions use the npm `next` tag and create a GitHub
prerelease. Stable versions use `latest`.

## Trusted publishing

The npm job uses the `npm` GitHub environment and npm's OpenID Connect trusted
publisher. The PyPI job uses the `pypi` environment and its
[trusted publisher](https://docs.pypi.org/trusted-publishers/) with
`uv publish --trusted-publishing always`. Both jobs receive prebuilt artifacts,
request only job-scoped identity permission, and contain no registry token.

The npm package contains all four native addons. The Python release contains
four stable-ABI wheels and one source distribution.

## Native archives

Targets:

- `x86_64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Each archive contains the executable, README, license, generated completions,
checksums, and build metadata. Build metadata records the source revision,
target, compiler, public schema, artifact format, collector protocol, CDP
revision, and managed browser identity.

## Local gate

```console
just release-check
```

The CI validation job installs every required repository tool before running
the gate. Run the corresponding package check after changing a release shape.
