# Contributor setup

Start with the Rust package that owns your change:

```console
just setup
just quick offprint-model
```

`quick` checks that package's formatting, lints its targets with warnings denied,
and runs its tests. Find the owner in the
[architecture map](./architecture.md).

## Required tools

- [Rustup](https://rust-lang.github.io/rustup/) to select the Rust toolchain pinned
  in [`rust-toolchain.toml`](../rust-toolchain.toml)
- [just](https://just.systems/) to run repository recipes
- [Bun](https://bun.sh/docs) for collector and Node.js binding development
- [Node.js](https://nodejs.org/) 22 or newer for package checks
- [uv](https://docs.astral.sh/uv/) and Python 3.14 for Python development
- `actionlint`, Taplo, cargo-deny, cargo-machete, and cargo-semver-checks for the
  complete release gate

[`ci.yml`](../.github/workflows/ci.yml) records the continuous integration
toolchain and commands. Ensure `cargo` and `rustc` resolve through Rustup so the
repository's toolchain pin takes effect.

Install the dependencies for the surface you are editing:

```console
bun install --cwd collector --frozen-lockfile
bun install --cwd bindings/node --frozen-lockfile
uv sync --directory bindings/python --frozen
```

Keep the checked-in lockfiles unchanged during setup. Review dependency and
license changes through the checks in [Dependency policy](./dependencies.md).

## Focused development loop

Use individual steps while resolving a failure:

```console
just check offprint-model
just lint offprint-model
just test offprint-model
```

`check` and `lint` include tests, examples, and every package feature. `test`
runs the default-feature suite and accepts a test-name filter as its second
argument. Omit the package to check, lint, or test the workspace.

Use the [validation table](./testing.md#choose-the-checks-for-your-change)
before handoff. Browser changes start with one fixture, then one serialized
group. `just --list` shows the available recipes.

## Generated output

Run `just codegen` after changing canonical records, fixture metadata, CDP
selection, or collector source. Review generated diffs beside their owner and
finish with `just codegen-check`.

## Clean the workspace

`just clean` removes Cargo targets, local JavaScript dependencies, Python
virtual environments and caches, native addons, and package output. It leaves
tracked source unchanged.
