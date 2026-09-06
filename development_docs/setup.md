# Contributor setup

The repository pins Rust 1.97 and keeps JavaScript and Python development
dependencies in package-local lockfiles.

## Required tools

- Rustup with the pinned toolchain
- `just` for repository recipes
- Bun for the collector and Node.js binding
- Node.js 22 or newer for package checks
- uv and Python 3.14 for Python development
- `actionlint`, Taplo, cargo-deny, cargo-machete, and cargo-semver-checks for the
  complete release gate

The pinned installer action and Rust toolchain in
[`publish.yml`](../.github/workflows/publish.yml) are the CI source of truth.
The repository does not currently pin separate local binary versions for every
release tool, so use the workflow when an exact local reproduction is not
available.

Install dependencies without changing lockfiles:

```console
just setup
bun install --cwd collector --frozen-lockfile
bun install --cwd bindings/node --frozen-lockfile
uv sync --directory bindings/python --frozen
```

Keep the checked-in lockfiles unchanged during setup. Review dependency and
license changes through the checks in [Dependency policy](./dependencies.md).

## Focused development loop

Run formatting and the nearest owner test while editing:

```console
just fmt-check
just check offprint-model
just test offprint-model
```

Use the boundary table in [Testing](./testing.md) before handoff. Browser-heavy
work starts with one fixture ID, then one serialized group.

## Generated output

Run `just codegen` after changing canonical records, fixture metadata, CDP
selection, or collector source. Review generated diffs beside their owner and
finish with `just codegen-check`.

## Clean the workspace

`just clean` removes Cargo targets, local JavaScript dependencies, Python
virtual environments and caches, native addons, and package output. It leaves
tracked source unchanged.
