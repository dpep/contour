#!/usr/bin/env bash
# The commit gate: format, lint, test. CI runs the same three.
#
# Why a script rather than a chain of cargo commands: a shell pipeline's exit
# status is its LAST command's, so `cargo clippy … | tail -1` reports success
# even when clippy failed. Don't filter cargo through head/tail when you're
# gating on the result.
set -euo pipefail

cd "$(dirname "$0")/.."

# Homebrew's rustup is keg-only, so cargo may not be on PATH. Only go looking
# if it isn't already — otherwise this would shadow a perfectly good toolchain.
if ! command -v cargo >/dev/null 2>&1; then
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
fi
command -v cargo >/dev/null 2>&1 || {
  echo "check: cargo not found (tried /opt/homebrew/opt/rustup/bin)" >&2
  exit 1
}

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

# The ONNX embedder is behind a feature, so the default build above never
# compiles it. `semantic-dynamic` dlopens ONNX Runtime instead of downloading
# and linking it, so this proves the code path builds without fetching a
# runtime in CI — which is why it, and not `semantic`, is the gated one.
cargo clippy --all-targets --features semantic-dynamic -- -D warnings

printf '\nall green\n'
