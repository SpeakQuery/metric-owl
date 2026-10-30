#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo test --workspace --locked
pnpm --dir web test
# Core tests regenerate TypeScript contracts; rebuild against those exact contracts.
pnpm --dir web build
cargo build --release --workspace --locked
mkdir -p artifacts/release
cp target/release/server-guard target/release/guard-relay artifacts/release/
cp LICENSE BINARY-LICENSE.txt artifacts/release/
python3 scripts/collect-licenses.py
(cd artifacts/release && shasum -a 256 server-guard guard-relay > SHA256SUMS)
printf 'Ready: artifacts/release/\n'
