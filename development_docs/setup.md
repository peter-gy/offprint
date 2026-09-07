# Contributor setup

Install the workspace dependencies, then check the area you are changing:

```console
pnpm install --frozen-lockfile
uv sync --directory sdk/python --frozen --no-install-project
just quick offprint-model
```

[Rustup](https://rust-lang.github.io/rustup/) selects Rust 1.97 from the root
`rust-toolchain.toml`. Use [Node.js](https://nodejs.org/) 24.14.1 or newer,
[pnpm](https://pnpm.io/) 12.3.4, [uv](https://docs.astral.sh/uv/), and
[just](https://just.systems/). Ensure `cargo` and `rustc` resolve through Rustup.

## Choose the check loop

| Change             | Fast feedback                              | Runtime or package evidence                    |
| ------------------ | ------------------------------------------ | ---------------------------------------------- |
| Rust crate         | `just check PACKAGE`, `just quick PACKAGE` | Owning browser fixture or package check        |
| Shared JavaScript  | `just js-check`                            | `pnpm test`                                    |
| Collector          | `just collector-check`                     | `just test-fixture FIXTURE_ID`                 |
| Node.js SDK        | `pnpm --filter offprint check`             | `just node-check`, `just node-package-check`   |
| Python SDK         | `just python-qa`                           | `just python-check`, `just python-wheel-check` |
| User documentation | `just docs-dev`, `just site-check`         | `just docs-check` and browser inspection       |

`just quick PACKAGE` checks Rust formatting, lints every target and feature,
and runs the package's default-feature tests. `just test PACKAGE FILTER`
selects tests by name. Browser fixture selection requires exactly one owner.

## Repository layout

- `offprint-rs/` owns Cargo configuration, Rust source, native adapters,
  benchmarks, fuzz targets, and repository automation.
- `sdk/node/` owns the npm package and its JavaScript API.
- `sdk/python/` owns the Python package, its uv lockfile, and Python QA.
- `collector/` owns the browser collector and its generated JavaScript bundle.
- `docs/` owns the VitePress site and its authored pages.
- `schemas/` and `fixtures/` hold shared generated contracts and browser inputs.

Run Cargo directly with `--manifest-path offprint-rs/Cargo.toml`, or enter
`offprint-rs/` first. Cargo outputs belong to `offprint-rs/target/`.

## Shared tooling

The root pnpm workspace owns one lockfile and shared Oxfmt/Oxlint configuration.
Each JavaScript package declares the build and test tools it uses. Catalogs
keep shared TypeScript, Vite, and Vitest versions aligned.

`pnpm format` formats JavaScript, TypeScript, and site sources. `just fmt`
also formats Rust. Python uses `just python-format`. Generated declarations
and collector bundles are formatted by their owning generators.

`just python-qa` runs Ruff formatting/linting, ty, and Pyrefly across Python
source, stubs, examples, and tests. It uses the locked development environment without
changing the installed native adapter. `just python-check` then builds that adapter and
runs pytest.

Dependency installation uses exact pins, frozen lockfiles, a three-day pnpm
release-age gate, and disabled package lifecycle scripts. Review an upstream
package before changing those boundaries. See [dependency policy](./dependencies.md).

## Documentation site

```console
just docs-dev
just site-check
pnpm --filter @offprint/docs preview
```

`just docs-dev` runs [Portless](https://github.com/vercel-labs/portless), a local
reverse proxy, at `https://docs.offprint.localhost`. It assigns VitePress a free
upstream port and prefixes the hostname in linked Git worktrees. The printed
URL reflects the active proxy configuration.

The first Portless run may request administrator access to trust its local
certificate and bind the HTTPS proxy. Stop the docs process with Ctrl+C to
release its route. Use `PORTLESS=0 just docs-dev` to run VitePress directly.

The production site is written to `docs/.vitepress/dist`. To verify a host
subpath, pass VitePress's native base option to both commands:

```console
pnpm --filter @offprint/docs build --base /offprint/
pnpm --filter @offprint/docs preview --base /offprint/
```

The [Pages workflow](../.github/workflows/pages.yml) checks pull requests and
deploys `main` to GitHub Pages. It uses `configure-pages`'s `origin` as
`SITE_URL` and passes `base_path` through `BASE_PATH` to VitePress's `--base`
option. Project sites and custom domains use the URL configured by GitHub.

Set `SITE_URL` to the deployment origin to include absolute canonical and
social preview URLs. The base option supplies any path prefix:

```console
SITE_URL=https://example.com pnpm --filter @offprint/docs build --base /offprint/
```

This build points social previews to `https://example.com/offprint/og.png`.
The theme selects light and dark artwork from `docs/public/brand/`.

Edit the landing page through VitePress's `hero` and `features` frontmatter in
`docs/index.md`. Card icons are static SVGs from [Lucide](https://lucide.dev/),
distributed through [Iconify](https://iconify.design/), in each card's `icon`
field.

The site build checks that every documentation page appears in the sidebar
defined in `docs/.vitepress/config.ts`.

## Generated output and complete gates

Run `just codegen` after changing canonical records, fixture metadata, CDP
selection, or collector source, then run `just codegen-check`.

[Testing](./testing.md) maps changes to required evidence.
[CI](../.github/workflows/ci.yml) records the repository tools and installation
steps required by the complete release gate.

`just clean` clears build outputs, package installations, and generated caches.
