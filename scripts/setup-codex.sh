#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
source scripts/codex-env.sh

for tool in node npm curl cc; do
  command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; exit 1; }
done
node -e "if (Number(process.versions.node.split('.')[0]) < 20) process.exit(1)"

mkdir -p var/codex
if ! command -v rustup >/dev/null; then
  installer="$(mktemp "$repo_root/var/codex/rustup-init.XXXXXX")"
  trap 'rm -f "$installer"' EXIT
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
    https://sh.rustup.rs -o "$installer"
  sh "$installer" -y --no-modify-path --default-toolchain none
fi

# rust-toolchain.toml supplies the version, profile, and required components.
rustup toolchain install
rustc --version
cargo --version
cargo clippy --version
cargo fmt --version
node --version
npm --version
npm ci --no-audit --no-fund
cargo fetch --locked --manifest-path sovmem/Cargo.toml

# Write an activation file for explicit sourcing in each new Bash shell.
# Setup-process exports do not carry into later task shells.
printf 'export CARGO_HOME=%q\nexport RUSTUP_HOME=%q\nexport npm_config_cache=%q\nexport PATH="$CARGO_HOME/bin:$PATH"\n' \
  "$CARGO_HOME" "$RUSTUP_HOME" "$npm_config_cache" > var/codex/env.sh

echo 'Dependencies installed. In subsequent Bash shells, run: source var/codex/env.sh'
echo 'Validate with the commands documented in docs/codex-cloud-setup.md.'
