set positional-arguments

default:
    @just --list

setup:
    rustup show active-toolchain
    cargo fetch --locked

clean:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo clean
    cargo clean --manifest-path fuzz/Cargo.toml
    rm -rf -- \
      .cargo-deny \
      .nextest \
      .mypy_cache \
      .pytest_cache \
      .ruff_cache \
      collector/.bun \
      collector/node_modules \
      bindings/node/coverage \
      bindings/node/node_modules \
      bindings/python/.mypy_cache \
      bindings/python/.pytest_cache \
      bindings/python/.ruff_cache \
      bindings/python/.venv \
      bindings/python/dist \
      bindings/python/target \
      dist
    find bindings/node -maxdepth 1 -type f -name '*.node' -delete
    find bindings/python/python/offprint -maxdepth 1 -type f \
      \( -name '_native*.so' -o -name '_native*.dylib' -o -name '_native*.pyd' \) \
      -delete
    find bindings/python -type d -name '__pycache__' -prune -exec rm -rf -- {} +

fmt:
    cargo fmt --all
    taplo format Cargo.toml rust-toolchain.toml deny.toml versions.toml
    cd collector && bun run format

fmt-check:
    cargo fmt --all -- --check
    taplo format --check Cargo.toml rust-toolchain.toml deny.toml versions.toml
    cd collector && bun run format:check

# Lint one Cargo package, or the workspace when omitted.
lint package="":
    #!/usr/bin/env bash
    set -euo pipefail
    selection=(--workspace)
    if [[ -n "$1" ]]; then
      selection=(-p "$1")
    fi
    cargo clippy --locked "${selection[@]}" --all-targets --all-features -- -D warnings

# Typecheck one Cargo package, including tests and examples, or the workspace.
check package="":
    #!/usr/bin/env bash
    set -euo pipefail
    selection=(--workspace)
    if [[ -n "$1" ]]; then
      selection=(-p "$1")
    fi
    cargo check --locked "${selection[@]}" --all-targets --all-features

# Test one Cargo package or the workspace, optionally matching a test name.
test package="" filter="":
    #!/usr/bin/env bash
    set -euo pipefail
    selection=(--workspace)
    if [[ -n "$1" ]]; then
      selection=(-p "$1")
    fi
    cargo test --locked "${selection[@]}" "$2"

# Check formatting, lint, and tests for one Rust package.
quick package:
    cargo fmt -p "$1" -- --check
    just lint "$1"
    just test "$1"

test-fixture fixture_id:
    cargo run --locked -p xtask -- test-fixture "{{fixture_id}}"

e2e group:
    cargo run --locked -p xtask -- e2e "{{group}}"

differential singlefile:
    OFFPRINT_SINGLEFILE_EXECUTABLE="{{singlefile}}" cargo test --release --locked -p offprint --test differential -- --ignored --test-threads=1

exploratory-corpus manifest="fixtures/corpora/datawrapper.json" output="target/benchmark-evidence/datawrapper-corpus.json":
    cargo run --release --locked -p xtask -- exploratory-corpus \
      --manifest "{{manifest}}" \
      --output "{{output}}"

benchmark-micro output="target/benchmark-evidence/performance-micro.json":
    cargo run --release --locked -p offprint-bench -- \
      --suite micro \
      --output "{{output}}"

benchmark-browser output="target/benchmark-evidence/performance-browser.json" browser_path="":
    #!/usr/bin/env bash
    set -euo pipefail
    arguments=(--suite browser --output "{{output}}")
    if [[ -n "{{browser_path}}" ]]; then
      arguments+=(--browser-path "{{browser_path}}")
    fi
    cargo run --release --locked -p offprint-bench -- "${arguments[@]}"

benchmark output="target/benchmark-evidence/performance.json":
    cargo run --release --locked -p offprint-bench -- \
      --suite all \
      --output "{{output}}"

benchmark-compare baseline output="target/benchmark-evidence/performance.json" allow_environment_mismatch="false":
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
    cargo run --release --locked -p offprint-bench -- "${arguments[@]}"

codegen:
    cargo run --locked -p xtask -- codegen
    cargo run --locked -p xtask -- codegen-cdp
    cd collector && bun run build

codegen-check:
    cargo run --locked -p xtask -- codegen --check
    cargo run --locked -p xtask -- codegen-cdp --check
    cd collector && bun run build:check

codegen-cdp:
    cargo run --locked -p xtask -- codegen-cdp

codegen-cdp-check:
    cargo run --locked -p xtask -- codegen-cdp --check

repo-check:
    cargo run --locked -p xtask -- check-repository

workflow-check:
    actionlint .github/workflows/*.yml

dependency-check:
    cargo deny check advisories bans licenses sources
    cargo machete

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
    cargo semver-checks check-release \
      --workspace \
      --exclude offprint-node \
      --exclude offprint-python \
      --exclude xtask \
      --baseline-rev "$baseline"

collector-check:
    cd collector && bun install --frozen-lockfile
    cd collector && bun run format:check
    cd collector && bun run typecheck
    cd collector && bun run check
    cd collector && bun test
    cd collector && bun run build:check

node-check:
    cd bindings/node && bun install --frozen-lockfile
    cd bindings/node && bun run build:test
    cd bindings/node && bun run typecheck
    cd bindings/node && bun test

node-package-check:
    #!/usr/bin/env bash
    set -euo pipefail
    workspace="$(pwd)"
    package_tmp="$(mktemp -d)"
    cd bindings/node
    cleanup() {
      node "$workspace/bindings/node/scripts/package-license.mjs" clean
      rm -rf "$package_tmp"
    }
    trap cleanup EXIT
    node scripts/package-license.mjs sync
    bun run build
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
    cp "$workspace/bindings/node/test/package-smoke.mjs" smoke.mjs
    node smoke.mjs

python-check:
    cd bindings/python && uv sync --frozen
    cd bindings/python && uv run --frozen maturin develop \
      --features extension-module,binding-test-hooks
    cd bindings/python && uv run --frozen pytest
    cd bindings/python && uv run --frozen mypy
    cd bindings/python && uv run --frozen mypy --strict \
      python/offprint/__init__.pyi python/offprint/contracts.py
    cd bindings/python && uv run --frozen mypy --config-file=/dev/null \
      --no-incremental --strict tests/typing_contract.py
    cd bindings/python && uv run --frozen mypy --config-file=/dev/null \
      --no-incremental --strict examples/capture_memory.py

python-wheel-check:
    #!/usr/bin/env bash
    set -euo pipefail
    workspace="$(pwd)"
    wheel_tmp="$(mktemp -d)"
    trap 'rm -rf "$wheel_tmp"' EXIT
    cd bindings/python
    uv run --frozen maturin build --release --sdist --out "$wheel_tmp/dist"
    uv run --frozen python tests/package_contents.py \
      "$workspace/LICENSE" "$wheel_tmp"/dist/*
    uv venv "$wheel_tmp/venv"
    uv pip install --python "$wheel_tmp/venv/bin/python" "$wheel_tmp"/dist/*.whl
    "$wheel_tmp/venv/bin/python" -c \
      "import offprint; assert offprint.Offprint"
    "$wheel_tmp/venv/bin/python" tests/wheel_smoke.py

fuzz-check:
    cd fuzz && nightly_cargo="$(rustup which cargo --toolchain nightly)" && PATH="$(dirname "$nightly_cargo"):$PATH" rustup run nightly "$nightly_cargo" fuzz build

fuzz-smoke seconds="3":
    #!/usr/bin/env bash
    set -euo pipefail
    nightly_cargo="$(rustup which cargo --toolchain nightly)"
    export PATH="$(dirname "$nightly_cargo"):$PATH"
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
      rustup run nightly "$nightly_cargo" fuzz run --fuzz-dir fuzz "$target" -- \
        -max_total_time="{{seconds}}" -timeout=10 -rss_limit_mb=2048
    done

fuzz-target target seconds="300":
    nightly_cargo="$(rustup which cargo --toolchain nightly)" && PATH="$(dirname "$nightly_cargo"):$PATH" rustup run nightly "$nightly_cargo" fuzz run --fuzz-dir fuzz "{{target}}" -- \
      -max_total_time="{{seconds}}" -timeout=10 -rss_limit_mb=2048

miri:
    #!/usr/bin/env bash
    set -euo pipefail
    nightly_cargo="$(rustup which cargo --toolchain nightly)"
    export PATH="$(dirname "$nightly_cargo"):$PATH"
    export MIRIFLAGS="${MIRIFLAGS:+$MIRIFLAGS }-Zmiri-disable-isolation"
    export PROPTEST_CASES="${PROPTEST_CASES:-16}"
    rustup run nightly "$nightly_cargo" miri setup
    rustup run nightly "$nightly_cargo" miri test --locked \
      -p offprint-artifact \
      -p offprint-browser \
      -p offprint-capture \
      -p offprint-model \
      -p offprint-protocol

docs-check:
    cargo run --locked -p xtask -- check-repository
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps --lib
    cargo test --locked --workspace --doc
    cargo check --locked -p offprint --examples
    cd bindings/node && bun run typecheck
    cd bindings/python && uv run --frozen mypy --config-file=/dev/null \
      --no-incremental --strict examples/capture_memory.py

package target binary output="dist":
    cargo run --locked -p xtask -- package \
      --target "{{target}}" \
      --binary "{{binary}}" \
      --output "{{output}}"

package-check:
    #!/usr/bin/env bash
    set -euo pipefail
    package_tmp="$(mktemp -d)"
    trap 'rm -rf "$package_tmp"' EXIT
    target="$(rustc -vV | sed -n 's/^host: //p')"
    cargo build --release --locked -p offprint-cli
    cargo run --locked -p xtask -- package \
      --target "$target" \
      --binary target/release/offprint \
      --output "$package_tmp"
    archive="$(find "$package_tmp" -maxdepth 1 -name '*.tar.gz' -print -quit)"
    tar -xzf "$archive" -C "$package_tmp"
    root="$(find "$package_tmp" -mindepth 1 -maxdepth 1 -type d -name 'offprint-*' -print -quit)"
    (cd "$root" && shasum -a 256 -c SHA256SUMS)
    "$root/offprint" --version
    test -s "$root/completions/offprint.bash"
    test -s "$root/completions/_offprint"

crate-package-check:
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
      cargo package \
        "${package_selection[@]}" \
        --locked \
        --allow-dirty \
        --no-verify
    CARGO_TARGET_DIR="$package_target" \
      cargo run --locked -p xtask -- verify-crate-packages \
        --directory "$package_target/package"
    CARGO_TARGET_DIR="$package_target" \
      cargo publish \
        "${package_selection[@]}" \
        --locked \
        --dry-run \
        --allow-dirty \
        --no-verify
    CARGO_TARGET_DIR="$package_target" \
      cargo run --locked -p xtask -- verify-crate-packages \
        --directory "$package_target/package"

release-check:
    just fmt-check
    just repo-check
    just workflow-check
    just lint
    just test
    just codegen-check
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
