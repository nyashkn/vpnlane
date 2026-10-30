# wglane tasks. `just ci` is the single gate: what CI runs, run it before pushing.

default:
    @just --list

# The gate: format, lint, test, licenses/advisories, docs, slim build.
ci: fmt-check clippy test deny doc check-no-tui check-versions

# Format everything.
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run all tests (no network). New snapshots: INSTA_UPDATE=always just test
test:
    cargo test --workspace --all-features

deny:
    cargo deny check

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# The binary must still build without the optional TUI.
check-no-tui:
    cargo check -p wglane-cli --no-default-features

# Release-please manifest, workspace version and Cargo.lock must agree.
check-versions:
    cargo xtask check-versions

# Run wglane, e.g. `just run status --json`.
run *args:
    cargo run -p wglane-cli -- {{args}}

# Generate completions and man pages into target/assets/.
assets:
    cargo xtask completions
    cargo xtask man

# Build, install both binaries into cargo's bin dir, install the root helper, check.
# sudo's PATH excludes ~/.cargo/bin, so the full path is used.
install:
    cargo install --path crates/wglane-cli --locked
    sudo "${CARGO_HOME:-$HOME/.cargo}/bin/wglane" install-helper
    "${CARGO_HOME:-$HOME/.cargo}/bin/wglane" doctor

# Real-Mac smoke test. Needs sudo, a token and a live network, so it is NOT in `ci`.
# `just mac-test US`. Test `wglane watch` by hand afterwards (switch Wi-Fi).
mac-test cc="US":
    cargo build --release
    sudo ./target/release/wglane install-helper
    ./target/release/wglane doctor
    ./target/release/wglane up {{cc}}
    ./target/release/wglane status   # also pings a tailnet peer
    ./target/release/wglane down
