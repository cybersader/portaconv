# Contributing

Fork and clone the repository. Install [rustup](https://rustup.rs/);
[`rust-toolchain.toml`](rust-toolchain.toml) selects stable Rust with `rustfmt`
and `clippy`. No local conversation data or credentials are needed.

From the repository root, run the Rust CI checks in one command:

```sh
sh scripts/check.sh
```

The script runs the same commands as [GitHub Actions](.github/workflows/ci.yml),
with `RUSTFLAGS="-D warnings"`:

- Formatting: `cargo fmt --all -- --check`
- Linting: `cargo clippy --all-targets --all-features -- -D warnings`
- Tests: `cargo test --all-targets --locked`
- Release build: `cargo build --release --locked`

Push your branch to your fork and open a pull request against `main`. GitHub
runs the Rust checks on hosted runners, including tests and release builds on
Linux and macOS.
