# Building with multiple agents

vpnlane is small enough for one `target/`, but parallel agents each need their
own build directory. `target/debug` is ~1.3 GB, so N agents = N x 1.3 GB plus
link contention.

## Layout

```
remote host
  ~/.cargo/registry                 shared, ~2 GB
  ~/.cache/sccache                  local cache, cap 20 GB (optional)
  /var/build/<agent>/target         one per agent, outside the worktree
  <worktree>/                       source only
```

`CARGO_TARGET_DIR` must live outside the worktree: `git clean` cannot remove it
and worktrees stay small.

## Environment per agent

```bash
export CARGO_TARGET_DIR=/var/build/$AGENT/target
export CARGO_BUILD_JOBS=$(( $(sysctl -n hw.ncpu 2>/dev/null || nproc) / N_AGENTS ))
[ "$CARGO_BUILD_JOBS" -ge 1 ] || CARGO_BUILD_JOBS=1   # Cargo rejects 0 jobs
```

Build with the agent profile:

```bash
cargo build --profile agent
cargo test --profile agent --workspace --all-features
```

`[profile.agent]` inherits `dev` but sets `debug = 0` and
`incremental = false`. Debug info and incremental artifacts are most of a Rust
target directory and agents rarely consume either.

## Disk budget

```
disk = N_AGENTS x target_agent + sccache_cap + registry + slack
```

Measured on a clean build plus tests: `agent` ~0.55 GB, `dev` ~1.3 GB.

Example: 8 agents x 0.6 GB + 20 GB sccache cap + 2 GB registry = ~27 GB.
Without `[profile.agent]` the same setup is 8 x 1.3 + 22 = ~33 GB.

## Shared compiler cache (optional)

sccache cuts the dependency rebuild that every fresh agent target pays:

```bash
cargo install sccache
export RUSTC_WRAPPER=sccache
export SCCACHE_BUCKET=vpnlane-rust-cache
export SCCACHE_ENDPOINT=https://<account>.r2.cloudflarestorage.com
export SCCACHE_REGION=auto
export AWS_ACCESS_KEY_ID=...        # R2 API token, from your secret store
export AWS_SECRET_ACCESS_KEY=...
```

`SCCACHE_CACHE_SIZE` only caps the local disk cache (`~/.cache/sccache`); the
bucket is capped by an R2 lifecycle rule.

R2 speaks the S3 API and has no egress fee. Skip sccache while one agent builds
at a time; add it when agents start sharing a host or CI runners.

## Hygiene

- One-shot agents: `cargo clean` or `cargo sweep --time 3` (needs
  `cargo install cargo-sweep`) after the task.
- Long-lived sessions: keep incremental (the default `dev` profile).
- Watch `sccache --show-stats`; below ~60% hit rate it is not paying for itself.
- Never point two agents at the same `target/`: Cargo's lock serializes them
  ("Blocking waiting for file lock on build directory").
