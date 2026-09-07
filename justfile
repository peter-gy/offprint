set positional-arguments

default:
    @just --list

setup:
    rustup show active-toolchain
    cargo fetch --manifest-path offprint-rs/Cargo.toml --locked

clean:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clean --manifest-path offprint-rs/Cargo.toml
    cargo clean --manifest-path offprint-rs/fuzz/Cargo.toml
    rm -rf -- \
      offprint-rs/.cargo-deny \
      offprint-rs/.nextest \
      .pytest_cache \
      .ruff_cache \
      node_modules \
      collector/node_modules \
      docs/node_modules \
      docs/.vitepress/cache \
      docs/.vitepress/dist \
      sdk/node/coverage \
      sdk/node/node_modules \
      sdk/python/.pytest_cache \
      sdk/python/.ruff_cache \
      sdk/python/.venv \
      sdk/python/dist \
      sdk/python/target \
      dist
    find sdk/node -maxdepth 1 -type f -name '*.node' -delete
    find sdk/python/src/offprint -maxdepth 1 -type f \
      \( -name '_native*.so' -o -name '_native*.dylib' -o -name '_native*.pyd' \) \
      -delete
    find sdk/python -type d -name '__pycache__' -prune -exec rm -rf -- {} +

fmt:
    cargo fmt --manifest-path offprint-rs/Cargo.toml --all
    taplo format offprint-rs/Cargo.toml rust-toolchain.toml offprint-rs/deny.toml offprint-rs/about.toml versions.toml
    pnpm run format

fmt-check:
    cargo fmt --manifest-path offprint-rs/Cargo.toml --all -- --check
    taplo format --check offprint-rs/Cargo.toml rust-toolchain.toml offprint-rs/deny.toml offprint-rs/about.toml versions.toml
    pnpm run format:check

# Lint one Cargo package, or the workspace when omitted.
lint package="":
    #!/usr/bin/env bash
    set -euo pipefail
    selection=(--workspace)
    if [[ -n "$1" ]]; then
      selection=(-p "$1")
    fi
    cargo clippy --manifest-path offprint-rs/Cargo.toml --locked "${selection[@]}" --all-targets --all-features -- -D warnings

# Typecheck one Cargo package, including tests and examples, or the workspace.
check package="":
    #!/usr/bin/env bash
    set -euo pipefail
    selection=(--workspace)
    if [[ -n "$1" ]]; then
      selection=(-p "$1")
    fi
    cargo check --manifest-path offprint-rs/Cargo.toml --locked "${selection[@]}" --all-targets --all-features

# Test one Cargo package or the workspace, optionally matching a test name.
test package="" filter="":
    #!/usr/bin/env bash
    set -euo pipefail
    selection=(--workspace)
    if [[ -n "$1" ]]; then
      selection=(-p "$1")
    fi
    cargo test --manifest-path offprint-rs/Cargo.toml --locked "${selection[@]}" "$2"

# Check formatting, lint, and tests for one Rust package.
quick package:
    cargo fmt --manifest-path offprint-rs/Cargo.toml -p "$1" -- --check
    just lint "$1"
    just test "$1"

test-fixture fixture_id:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- test-fixture "{{fixture_id}}"

e2e group:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- e2e "{{group}}"

differential singlefile:
    OFFPRINT_SINGLEFILE_EXECUTABLE="{{singlefile}}" cargo test --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint --test differential -- --ignored --test-threads=1

exploratory-corpus manifest="fixtures/corpora/datawrapper.json" output="offprint-rs/target/benchmark-evidence/datawrapper-corpus.json":
    cargo run --manifest-path offprint-rs/Cargo.toml --release --locked -p xtask -- exploratory-corpus \
      --manifest "{{manifest}}" \
      --output "{{output}}"

benchmark-micro output="offprint-rs/target/benchmark-evidence/performance-micro.json":
    cargo run --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-bench -- \
      --suite micro \
      --output "{{output}}"

benchmark-browser output="offprint-rs/target/benchmark-evidence/performance-browser.json" browser_path="":
    #!/usr/bin/env bash
    set -euo pipefail
    arguments=(--suite browser --output "{{output}}")
    if [[ -n "{{browser_path}}" ]]; then
      arguments+=(--browser-path "{{browser_path}}")
    fi
    cargo run --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-bench -- "${arguments[@]}"

benchmark output="offprint-rs/target/benchmark-evidence/performance.json":
    cargo run --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-bench -- \
      --suite all \
      --output "{{output}}"

benchmark-compare baseline output="offprint-rs/target/benchmark-evidence/performance.json" allow_environment_mismatch="false":
    #!/usr/bin/env bash
    set -euo pipefail
    arguments=(
      --suite all
      --output "{{output}}"
      --baseline "{{baseline}}"
    )
    if [[ "{{allow_environment_mismatch}}" == "true" ]]; then
      arguments+=(--allow-environment-mismatch)
    fi
    cargo run --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-bench -- "${arguments[@]}"

codegen:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen-cdp
    pnpm --filter @offprint/collector run build

codegen-check:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen --check
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen-cdp --check
    pnpm --filter @offprint/collector run build:check

codegen-cdp:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen-cdp

codegen-cdp-check:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- codegen-cdp --check

repo-check:
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- check-repository

workflow-check:
    actionlint .github/workflows/*.yml

dependency-check:
    cd offprint-rs && cargo deny check advisories bans licenses sources
    cargo machete offprint-rs
    just licenses-check

licenses:
    just _generate-notices THIRD_PARTY_NOTICES.txt
    cp LICENSE sdk/python/LICENSE
    cp THIRD_PARTY_NOTICES.txt sdk/python/THIRD_PARTY_NOTICES.txt

_generate-notices output:
    #!/usr/bin/env bash
    set -euo pipefail
    test "$(cargo about --version)" = "cargo-about 0.9.2"
    notice_tmp="$(mktemp)"
    trap 'rm -f "$notice_tmp"' EXIT
    cargo about generate --manifest-path offprint-rs/Cargo.toml \
      --workspace --locked --fail offprint-rs/about.hbs --output-file "$notice_tmp"
    cat offprint-rs/chromium/NOTICE offprint-rs/xtask/NOTICE "$notice_tmp" > "$1"

licenses-check:
    #!/usr/bin/env bash
    set -euo pipefail
    notice_tmp="$(mktemp)"
    trap 'rm -f "$notice_tmp"' EXIT
    just _generate-notices "$notice_tmp"
    if ! cmp -s THIRD_PARTY_NOTICES.txt "$notice_tmp"; then
      echo "Dependency notices are stale. Run just licenses." >&2
      exit 1
    fi
    cmp LICENSE sdk/python/LICENSE
    cmp THIRD_PARTY_NOTICES.txt sdk/python/THIRD_PARTY_NOTICES.txt

semver-check:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! git rev-parse --verify HEAD >/dev/null 2>&1; then
      echo "No repository history is available for SemVer comparison."
      exit 0
    fi
    current_tag="${GITHUB_REF_NAME:-}"
    baseline=""
    while IFS= read -r candidate; do
      if [[ "$candidate" != "$current_tag" ]]; then
        baseline="$candidate"
        break
      fi
    done < <(git tag --merged HEAD --list 'v[0-9]*' --sort=-v:refname)
    if [[ -z "$baseline" ]]; then
      echo "No prior release tag is available for SemVer comparison."
      exit 0
    fi
    cd offprint-rs
    cargo semver-checks check-release \
      --workspace \
      --exclude offprint-node \
      --exclude offprint-python \
      --exclude xtask \
      --baseline-rev "$baseline"

# Shared JavaScript formatting, linting, and type checks.
js-check:
    pnpm run check

collector-check:
    pnpm --filter @offprint/collector run check
    pnpm --filter @offprint/collector test
    pnpm --filter @offprint/collector run build:check

node-check:
    pnpm --filter offprint run check
    pnpm --filter offprint run build:test
    pnpm --filter offprint test

node-package-check:
    #!/usr/bin/env bash
    set -euo pipefail
    workspace="$(pwd)"
    package_tmp="$(mktemp -d)"
    cd sdk/node
    cleanup() {
      node "$workspace/sdk/node/scripts/package-license.mjs" clean
      rm -rf "$package_tmp"
    }
    trap cleanup EXIT
    node scripts/package-license.mjs sync
    pnpm run build
    suffix="$(node -e '
      const suffix =
        process.platform === "darwin" &&
          ["arm64", "x64"].includes(process.arch)
          ? `darwin-${process.arch}`
          : process.platform === "win32" && process.arch === "x64"
            ? "win32-x64-msvc"
            : process.platform === "linux" && process.arch === "x64"
              ? "linux-x64-gnu"
              : "";
      if (!suffix) process.exit(1);
      process.stdout.write(suffix);
    ')"
    root_addon="offprint-native.$suffix.node"
    test -f "$root_addon"
    if [[ "$(uname -s)" == "Darwin" ]]; then
      codesign --verify "$root_addon"
    fi
    root_package="$package_tmp/$(
      npm pack --ignore-scripts --silent --pack-destination "$package_tmp"
    )"
    node test/package-contents.mjs \
      "$root_package" "$workspace/LICENSE" "$root_addon"
    mkdir "$package_tmp/install"
    cd "$package_tmp/install"
    npm init --yes >/dev/null
    npm install --ignore-scripts --no-audit --no-fund "$root_package"
    node -e "if (!require('offprint').Offprint) process.exit(1)"
    node --input-type=module -e \
      "import { Offprint } from 'offprint'; if (!Offprint) process.exit(1)"
    cp "$workspace/sdk/node/test/package-smoke.mjs" smoke.mjs
    node smoke.mjs

# Check Python source and public types without compiling the native adapter.
python-qa:
    cd sdk/python && uv run --frozen --no-sync ruff format --check .
    cd sdk/python && uv run --frozen --no-sync ruff check .
    cd sdk/python && uv run --frozen --no-sync ty check
    cd sdk/python && uv run --frozen --no-sync pyrefly check

python-format:
    cd sdk/python && uv run --frozen --no-sync ruff format .

python-check:
    cd sdk/python && uv sync --frozen --no-install-project
    just python-qa
    cd sdk/python && uv run --frozen --no-sync maturin develop \
      --features extension-module,binding-test-hooks
    cd sdk/python && uv run --frozen --no-sync pytest

python-wheel-check:
    #!/usr/bin/env bash
    set -euo pipefail
    workspace="$(pwd)"
    wheel_tmp="$(mktemp -d)"
    trap 'rm -rf "$wheel_tmp"' EXIT
    cd sdk/python
    uv run --frozen --no-sync maturin sdist --out "$wheel_tmp/dist"
    uv run --frozen --no-sync python scripts/prepare_sdist.py "$wheel_tmp"/dist/*.tar.gz
    mkdir "$wheel_tmp/source"
    tar -xzf "$wheel_tmp"/dist/*.tar.gz -C "$wheel_tmp/source" --strip-components=1
    cd "$wheel_tmp/source"
    CARGO_TARGET_DIR="$wheel_tmp/target" \
      "$workspace/sdk/python/.venv/bin/maturin" build --release --locked --offline \
        --out "$wheel_tmp/dist"
    cd "$workspace/sdk/python"
    uv run --frozen --no-sync python tests/package_contents.py \
      "$workspace/LICENSE" "$wheel_tmp"/dist/*
    uv venv "$wheel_tmp/venv"
    uv pip install --python "$wheel_tmp/venv/bin/python" "$wheel_tmp"/dist/*.whl
    "$wheel_tmp/venv/bin/python" tests/wheel_smoke.py

fuzz-check:
    #!/usr/bin/env bash
    set -euo pipefail
    nightly_cargo="$(rustup which cargo --toolchain nightly)"
    export PATH="$(dirname "$nightly_cargo"):$PATH"
    fuzz_host="$(rustup run nightly rustc -vV | sed -n 's/^host: //p')"
    rustup run nightly "$nightly_cargo" fuzz build --fuzz-dir offprint-rs/fuzz --target "$fuzz_host"

fuzz-smoke seconds="3":
    #!/usr/bin/env bash
    set -euo pipefail
    nightly_cargo="$(rustup which cargo --toolchain nightly)"
    export PATH="$(dirname "$nightly_cargo"):$PATH"
    fuzz_host="$(rustup run nightly rustc -vV | sed -n 's/^host: //p')"
    targets=(
      collector_protocol
      css_url_rewriting
      data_url
      filename
      html_arena
      manifest
      mime
      resource_graph
      srcset
    )
    for target in "${targets[@]}"; do
      rustup run nightly "$nightly_cargo" fuzz run --fuzz-dir offprint-rs/fuzz --target "$fuzz_host" "$target" -- \
        -max_total_time="{{seconds}}" -timeout=10 -rss_limit_mb=2048
    done

fuzz-target target seconds="300":
    #!/usr/bin/env bash
    set -euo pipefail
    nightly_cargo="$(rustup which cargo --toolchain nightly)"
    export PATH="$(dirname "$nightly_cargo"):$PATH"
    fuzz_host="$(rustup run nightly rustc -vV | sed -n 's/^host: //p')"
    rustup run nightly "$nightly_cargo" fuzz run --fuzz-dir offprint-rs/fuzz --target "$fuzz_host" "{{target}}" -- \
      -max_total_time="{{seconds}}" -timeout=10 -rss_limit_mb=2048

miri:
    #!/usr/bin/env bash
    set -euo pipefail
    nightly_cargo="$(rustup which cargo --toolchain nightly)"
    export PATH="$(dirname "$nightly_cargo"):$PATH"
    export MIRIFLAGS="${MIRIFLAGS:+$MIRIFLAGS }-Zmiri-disable-isolation"
    export PROPTEST_CASES="${PROPTEST_CASES:-16}"
    rustup run nightly "$nightly_cargo" miri setup
    rustup run nightly "$nightly_cargo" miri test --manifest-path offprint-rs/Cargo.toml --locked \
      -p offprint-artifact \
      -p offprint-browser \
      -p offprint-capture \
      -p offprint-model \
      -p offprint-protocol

site-check:
    pnpm --filter @offprint/docs run check
    pnpm --filter @offprint/docs run build

docs-dev:
    pnpm --filter @offprint/docs dev

docs-check: repo-check site-check
    RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path offprint-rs/Cargo.toml --locked --workspace --no-deps --lib
    cargo test --manifest-path offprint-rs/Cargo.toml --locked --workspace --doc
    cargo check --manifest-path offprint-rs/Cargo.toml --locked -p offprint --examples

package target binary output="dist":
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- package \
      --target "{{target}}" \
      --binary "{{binary}}" \
      --output "{{output}}"

package-check:
    #!/usr/bin/env bash
    set -euo pipefail
    package_tmp="$(mktemp -d)"
    trap 'rm -rf "$package_tmp"' EXIT
    target="$(rustc -vV | sed -n 's/^host: //p')"
    cargo build --manifest-path offprint-rs/Cargo.toml --release --locked -p offprint-cli
    cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- package \
      --target "$target" \
      --binary offprint-rs/target/release/offprint \
      --output "$package_tmp"
    archive="$(find "$package_tmp" -maxdepth 1 -name '*.tar.gz' -print -quit)"
    tar -xzf "$archive" -C "$package_tmp"
    root="$(find "$package_tmp" -mindepth 1 -maxdepth 1 -type d -name 'offprint-*' -print -quit)"
    cmp THIRD_PARTY_NOTICES.txt "$root/THIRD_PARTY_NOTICES.txt"
    (cd "$root" && shasum -a 256 -c SHA256SUMS)
    "$root/offprint" --version
    test -s "$root/completions/offprint.bash"
    test -s "$root/completions/_offprint"

crate-package output="dist/crates":
    #!/usr/bin/env bash
    set -euo pipefail
    package_target="$(mktemp -d)"
    trap 'rm -rf "$package_target"' EXIT
    package_selection=(
      --workspace
      --exclude offprint-bench
      --exclude offprint-node
      --exclude offprint-python
      --exclude offprint-test-support
      --exclude xtask
    )
    CARGO_TARGET_DIR="$package_target" \
      cargo package --manifest-path offprint-rs/Cargo.toml \
        "${package_selection[@]}" \
        --locked \
        --allow-dirty \
        --no-verify
    CARGO_TARGET_DIR="$package_target" \
      cargo run --manifest-path offprint-rs/Cargo.toml --locked -p xtask -- verify-crate-packages \
        --directory "$package_target/package"
    mkdir -p "{{output}}"
    cp "$package_target"/package/*.crate "{{output}}/"

crate-package-check:
    #!/usr/bin/env bash
    set -euo pipefail
    package_output="$(mktemp -d)"
    trap 'rm -rf "$package_output"' EXIT
    just crate-package "$package_output"

release-check:
    just fmt-check
    just repo-check
    just workflow-check
    just lint
    just test
    just codegen-check
    just js-check
    just collector-check
    just node-check
    just node-package-check
    just python-check
    just python-wheel-check
    just dependency-check
    just semver-check
    just docs-check
    just package-check
    just crate-package-check
