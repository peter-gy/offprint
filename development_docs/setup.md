# Contributor setup

Install [Pixi](https://pixi.prefix.dev/v0.81.0/installation/) 0.81.0, then run
from the repository root:

```console
curl -fsSL https://pixi.sh/install.sh | PIXI_VERSION=v0.81.0 sh
pixi run --locked setup
pixi run --locked dev-check
pixi shell --locked
just quick offprint-model
```

The first command installs Pixi; reopen your terminal if `pixi` is not yet on
`PATH`. The setup task fetches locked Cargo dependencies, installs the frozen
pnpm workspace, and creates `sdk/python/.venv` from its uv lockfile without
building the native adapter. Run setup again after changing branches or after
`just clean`.

`pixi.toml` and `pixi.lock` supply Rust 1.97.0 (including Clippy and Rustfmt),
Node.js 24.19.0, pnpm 12.3.4, Python 3.14, uv, repository check tools, C/C++
compilers, build tools, and Linux Chromium libraries and fonts. Normal feature
work needs no global Rustup, Node.js, Python, or compiler installation on Linux.
Cargo, pnpm, uv, and the managed-browser catalog retain their existing lock and
verification contracts; Pixi owns the tools that execute them.

The locked platforms are Linux x86-64 with glibc >=2.28 and macOS >=13 on Intel
and Apple Silicon. macOS also needs Apple's SDK and system tools from
`xcode-select --install`. Windows contributors can use x86-64 WSL2; the existing
POSIX recipes and this development environment do not support native Windows.
Release packaging retains its separate target-specific toolchains.

Use the shell for the `just` commands below, or prefix a single command with
`pixi run --locked`, such as `pixi run --locked just check offprint`.
Exit the shell with `exit`. Editor terminals must use the same environment;
`pixi run --locked rustc --version` and `pixi run --locked cargo --version`
confirm tool selection. The conda Rust package supplies the compiler directly,
so `rust-toolchain.toml` applies to Rustup users outside Pixi. Keep its version,
`versions.toml`, and the Pixi Rust pin aligned.

## Browser development

```console
pixi run --locked browser-setup
pixi run --locked just test-fixture static-article
```

The browser setup task installs the revision and verified archive digest in
`versions.toml`. Linux activation supplies `LD_LIBRARY_PATH` for the locked
libraries because managed Chromium lives outside the Pixi prefix. Chromium
still uses its sandbox. Linux must allow unprivileged user namespaces; Ubuntu's
AppArmor policy may need a profile for the managed cache. If startup reports
`No usable sandbox`, use the same profile as the CI Chromium action:

```sh
sudo tee /etc/apparmor.d/offprint-chromium >/dev/null <<PROFILE
abi <abi/4.0>,
include <tunables/global>
profile offprint-chromium "${HOME}/**/chromium-*/chrome-linux64/chrome" flags=(unconfined) {
  userns,
}
PROFILE
sudo apparmor_parser --replace /etc/apparmor.d/offprint-chromium
```

This one-time host configuration permits sandbox namespaces for managed
Chromium. See [Linux sandbox troubleshooting](../docs/operations/troubleshooting.md#chromium-reports-no-usable-sandbox-on-linux)
for other installation paths or Linux policies.
A headed run also needs a display server (or host Xvfb). Pixi does not configure
kernel policies, install a display server, or request administrator access.

## Updating the environment

Commit `pixi.toml` and `pixi.lock` together. Use `pixi update PACKAGE` for a
reviewed dependency update; use `pixi lock` after changing manifest constraints.
Routine setup and CI use `--locked` so stale locks fail instead of resolving
new dependencies. Do not copy `.pixi/` between checkouts: its prefixes contain
absolute paths. `pixi clean` removes the environment; the next locked run
recreates it. `just clean` preserves it and removes language build outputs and
installations.

Nightly fuzzing and Miri are opt-in checks outside the default Pixi toolchain.
Install Rustup, a nightly toolchain with the Miri component, and cargo-fuzz,
then run the existing `just fuzz-smoke` or `just miri`
recipes from the Pixi shell to retain its C/C++ tools. Those nightly tools are
not pinned by `pixi.lock`.

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
[Publish](../.github/workflows/publish.yml) records the repository tools and installation
steps required by the complete release gate.

`just clean` clears build outputs, package installations, and generated caches.
