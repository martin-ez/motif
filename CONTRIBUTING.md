# Contributing

Everything — the design invariants, the documentation and testing rules, and the
branching model — lives in [AGENTS.md](AGENTS.md).

It is written for agents, but the rules are the same for everyone, and keeping
them in one file is what stops the two versions from drifting apart. This file
exists only because GitHub links it from the pull request and issue pages.

Before opening a pull request:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
scripts/check-style.sh
cargo check --target aarch64-unknown-linux-gnu
```
