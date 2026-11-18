#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
cargo build --release --locked
