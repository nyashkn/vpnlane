# Building with multiple agents

vpnlane is small enough for one `target/`, but parallel agents each need their
own build directory. `target/debug` is ~1.5 GB, so N agents = N x 1.5 GB plus
link contention.

## Layout

```
remote host
  ~/.cargo/registry                 shared, ~2 GB
  ~/.cache/sccache                  cap 20 GB (optional, shared by agents)
  /var/build/<agent>/target         one per agent, outside the worktree
  <worktree>/                       source only
```

`CARGO_TARGET_DIR` must live outside the worktree: `git clean` cannot remove it
and worktrees stay small.

## Environment per agent

```bash
export CARGO_TARGET_DIR=/var/build/$AGENT/target
export CARGO_BUILD_JOBS=$(( $(nproc) / N_AGENTS ))
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

Example: 8 agents, 2 GB each, 20 GB sccache cap, 2 GB registry = 38 GB.
Without `[profile.agent]` the same setup is ~60 GB.

## Shared compiler cache (optional)

sccache cuts the dependency rebuild that every fresh agent target pays:

```bash
cargo install sccache
export RUSTC_WRAPPER=sccache
export SCCACHE_CACHE_SIZE=20G
export SCCACHE_BUCKET=vpnlane-rust-cache
export SCCACHE_ENDPOINT=https://<account>.r2.cloudflarestorage.com
```

R2 speaks the S3 API and has no egress fee. Skip sccache while one agent builds
at a time; add it when agents start sharing a host or CI runners.

## Hygiene

- One-shot agents: `cargo clean` or `cargo sweep --time 3` after the task.
- Long-lived sessions: keep incremental (the default `dev` profile).
- Watch `sccache --show-stats`; below ~60% hit rate it is not paying for itself.
- Never point two agents at the same `target/`: Cargo's lock serializes them
  ("Blocking waiting for file lock on build directory").
