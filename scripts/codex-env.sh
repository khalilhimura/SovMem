#!/usr/bin/env bash
# Source this file in each shell that uses the workspace-local Rust toolchain.
_nous_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="${CARGO_HOME:-$_nous_root/var/codex/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$_nous_root/var/codex/rustup}"
export npm_config_cache="${npm_config_cache:-$_nous_root/var/codex/npm}"
export PATH="$CARGO_HOME/bin:$PATH"
unset _nous_root
