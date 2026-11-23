#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
tool_manifest="$project_root/tools/rustc-analysis/Cargo.toml"
exporter_manifest="$project_root/tools/bevy-schedule-export/Cargo.toml"
cd "$project_root"

# Build and run the driver with the compiler pinned by this repository.
unset RUSTUP_TOOLCHAIN
compiler_root=$(rustc --print sysroot)
target_libraries=$(rustc --print target-libdir)
toolchain=$(rustup show active-toolchain)
if command -v cygpath > /dev/null; then
  compiler_root=$(cygpath -u "$compiler_root")
  target_libraries=$(cygpath -u "$target_libraries")
fi
export RUSTUP_TOOLCHAIN="${toolchain%% *}"
export PATH="$compiler_root/bin:$compiler_root/lib:$PATH"
export LD_LIBRARY_PATH="$compiler_root/lib:$target_libraries${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export CARGO_TARGET_DIR="$project_root/tmp/rustc-analysis/tool/target"
export CARGO_BUILD_BUILD_DIR="$project_root/tmp/rustc-analysis/tool/build"

build_tool() {
  cargo build --locked --manifest-path "$tool_manifest" --workspace --bins
}

case "${1-}" in
  prepare|prepare-architecture)
    package=$(cd -- "$2" && pwd -P)
    build_tool
    if [[ "$1" == prepare-architecture ]]; then
      cargo fetch --locked --manifest-path "$exporter_manifest"
    fi
    # The analyzer loads the complete locked dependency graph offline.
    cd "$package"
    cargo fetch --locked
    ;;
  check|extract|model)
    exec "$CARGO_TARGET_DIR/debug/rustc-analysis" "$@"
    ;;
  test)
    cargo fetch --locked --manifest-path "$project_root/examples/bevy-rules/Cargo.toml"
    cargo fetch --locked --manifest-path "$exporter_manifest"
    cargo fetch --locked --manifest-path "$project_root/tools/rustc-analysis/runner/tests/fixtures/schedule-app/Cargo.toml"
    build_tool
    cargo fmt --manifest-path "$tool_manifest" --all -- --check
    cargo test --locked --manifest-path "$tool_manifest" --workspace -- --test-threads=1

    for rule in entity-names named-fields; do
      manifest="$project_root/tools/check-$rule/Cargo.toml"
      cargo fmt --manifest-path "$manifest" --all -- --check
      cargo build --locked --manifest-path "$manifest" --bins
      cargo test --locked --manifest-path "$manifest" -- --test-threads=1 \
        --skip production_source_entities_have_names_at_creation
    done
    ;;
  *)
    echo 'Usage: rustc-analysis.sh {prepare|prepare-architecture} PACKAGE | {check|extract|model} [ARGS] | test' >&2
    exit 2
    ;;
esac
