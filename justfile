# vpnlane tasks. `just ci` is the single gate: what CI runs, run it before pushing.

default:
    @just --list

# The gate: format, lint, test, licenses/advisories, docs, slim build, helper surface.
ci: check-versions fmt-check clippy test deny doc check-no-tui check-helper

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

# Unused dependencies (cargo install cargo-machete). Not in `ci` until clean.
machete:
    cargo machete

# Check the code compiles on an older toolchain. --ignore-rust-version because
# Cargo otherwise refuses any toolchain below the declared rust-version.
msrv version="1.99":
    cargo +{{version}} check --workspace --all-features --ignore-rust-version

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# The binary must still build without the optional TUI.
check-no-tui:
    cargo check -p vpnlane-cli --no-default-features

# The root helper must stay a thin binary: no TUI linkage, bounded size.
# Ceiling is debug-only: 2.4 MB today, 617 KB stripped release.
check-helper:
    cargo build -p vpnlane-cli --bin vpnlane-helper
    @bytes=$(wc -c < target/debug/vpnlane-helper | tr -d ' '); test "$bytes" -le 4194304 || { echo "vpnlane-helper is $bytes bytes (budget 4 MiB); it must not pull in the TUI or an HTTP stack" >&2; exit 1; }
    @! nm -a target/debug/vpnlane-helper 2>/dev/null | grep -qi ratatui || { echo "vpnlane-helper links ratatui; root code must stay independent of the TUI" >&2; exit 1; }

# Release-please manifest, workspace version and Cargo.lock must agree.
# --locked: cargo must not quietly rewrite Cargo.lock before the check reads it.
check-versions:
    cargo run --locked -p xtask -- check-versions

# Report countries Nord lists that the TUI continent table lacks. Needs network, so NOT in `ci`.
check-continents:
    cargo run --locked -p xtask -- check-continents

# Run vpnlane, e.g. `just run status --json`.
run *args:
    cargo run -p vpnlane-cli -- {{args}}

# Generate completions and man pages into target/assets/.
assets:
    cargo xtask completions
    cargo xtask man

# Build, install both binaries into cargo's bin dir, install the root helper, check.
# sudo's PATH excludes ~/.cargo/bin, so the full path is used.
install:
    cargo install --path crates/vpnlane-cli --locked
    sudo "${CARGO_HOME:-$HOME/.cargo}/bin/vpnlane" install-helper
    "${CARGO_HOME:-$HOME/.cargo}/bin/vpnlane" doctor

# Real-Mac smoke test. Needs sudo, a token and a live network, so it is NOT in `ci`.
# `just mac-test US`. Test `vpnlane watch` by hand afterwards (switch Wi-Fi).
mac-test cc="US":
    cargo build --release
    sudo ./target/release/vpnlane install-helper
    ./target/release/vpnlane doctor
    ./target/release/vpnlane up {{cc}}
    ./target/release/vpnlane status   # also pings a tailnet peer
    ./target/release/vpnlane down

# Re-record docs/media/{cli,tui}.gif with VHS (not part of ci). Needs `brew install vhs` and node; see
# demos/README.md. Hits api.nordvpn.com; no token or sudo.
demo:
    cargo build -p vpnlane-cli
    demos/record.sh
