# Architecture

Read this before changing code. It says what lives where and which way
dependencies point; the code says how. (Shape borrowed from rust-analyzer's
`architecture.md`: a bird's-eye view, then a map of the code, then the
invariants that are easy to break by accident.)

## Bird's-eye view

`wglane up ZA` must: find a token, make sure `~/.config/wglane/za.conf` exists
and is under 7 days old (regenerating it from the Nord API if not), read the
physical gateway, bring the WireGuard tunnel up through a root-owned helper,
then add host routes via that gateway for every Tailscale control and DERP IP
so the tailnet survives the full tunnel.

Hexagonal-lite: the core defines **ports** (traits) and pure logic; adapter
crates implement the ports; the binary wires adapters into commands. No DDD
layers beyond that.

## Crate graph

```
            wglane-cli (bin `wglane`, lib for xtask)     xtask
              │        │         │                         │
              ▼        ▼         ▼                         ▼
      wglane-nord  wglane-macos (+ bin `wglane-helper`)  wglane-cli
              │        │
              ▼        ▼
            wglane-core  (no I/O, no async, no network, no process spawning)
```

Dependency rules (the compiler enforces direction; review enforces the rest):

1. `wglane-core` depends on nothing in the workspace and does no I/O. Types
   from `std::io` may appear in errors; calls to it may not.
2. `wglane-nord` and `wglane-macos` depend only on `wglane-core`, never on each
   other. A Linux adapter would be a sibling `wglane-linux`.
3. Only `wglane-cli` knows concrete adapter types. Library code is generic over
   ports (`fn f<C: ServerCatalog>(c: &C)`), not `dyn`.
4. `thiserror` (`wglane_core::Error`) in libraries; `color-eyre` only in
   `wglane-cli` and `xtask`.
5. Blocking only. HTTP is `ureq`; no tokio anywhere.

## Ports and adapters

| Port (`wglane_core::ports`) | Production adapter | Fake (`wglane_core::testing`) |
|---|---|---|
| `ServerCatalog` | `wglane_nord::NordApi` | `FakeCatalog` |
| `TokenSource` | `wglane_nord::{EnvToken, FileToken, TokenChain}` | `FakeToken` |
| `ExitIpProbe` | `wglane_nord::NordApi` | `FakeExitIp` |
| `Tunnel` | `wglane_macos::WgQuick` (root, in helper), `HelperClient` (CLI) | `FakeTunnel` |
| `RouteBypass` | `wglane_macos::RouteTable` (root, in helper), `HelperClient` (CLI) | `FakeRoutes` |
| `BypassProvider` | `wglane_macos::{TailscaleBypass, StaticBypass}` | `FakeBypassProvider` |
| `NetworkWatcher` | `wglane_macos::RouteMonitor` | `FakeWatcher` |
| `TailnetProbe` | `wglane_macos::TailscalePing` | `FakeTailnet` |

Every external program in `wglane-macos` runs through `CommandRunner`
(`SystemRunner` in production, `wglane_macos::testing::FakeRunner` in tests).

### Privilege split

The CLI runs unprivileged. `HelperClient` implements `Tunnel` + `RouteBypass`
by running `sudo -n /usr/local/libexec/wglane-helper <verb> <args>`. The helper
(`crates/wglane-macos/src/bin/wglane-helper.rs`) runs as root and uses
`WgQuick` + `RouteTable` with `SystemRunner`. Verbs are fixed: `up <cc>`,
`down`, `bypass <gw> <cidr>...`. The helper takes no paths from the caller and
validates every argument with the core parsers. `install-helper` copies it to a
root-owned path; a sudoers rule on a user-writable binary would be a root
exploit. This verb set is the future daemon socket protocol.

## File ownership map

One unit of work per file, so parallel builders do not collide. `TODO(bead):
<id>` markers name the work items; `grep -rn "TODO(bead)" crates xtask`
lists them all.

| Crate | File | Owns | TODOs |
|---|---|---|---|
| core | `src/country.rs`, `secret.rs`, `server.rs`, `bypass.rs`, `status.rs`, `tunnel_config.rs` | domain types | 0 |
| core | `src/render.rs` (+ `snapshots/`) | `render_wg_conf` | 0 (reference) |
| core | `src/stale.rs`, `src/rank.rs` | `is_stale`, `rank_by_load` | 0 |
| core | `src/generate.rs` | `generate_config` use case | 1 |
| core | `src/ports/<port>.rs` + `src/testing/<port>.rs` | one port and its fake | 0 |
| core | `src/error.rs` | error enum (append variants; shared, keep edits small) | 0 |
| nord | `src/token.rs` | token sources | 0 (reference) |
| nord | `src/wire.rs` + `fixtures/` | JSON shapes, parsing | 2 |
| nord | `src/api.rs` | HTTP calls | 4 |
| macos | `src/command.rs`, `src/testing.rs` | `CommandRunner`, `FakeRunner` | 0 (reference) |
| macos | `src/conf_store.rs` | config files on disk | 2 |
| macos | `src/tunnel.rs` | `WgQuick` | 3 |
| macos | `src/routes.rs` | `RouteTable` | 3 |
| macos | `src/bypass.rs` | Tailscale + static bypass | 1 |
| macos | `src/watcher.rs` | `RouteMonitor` | 1 |
| macos | `src/probe.rs` | `TailscalePing` | 1 |
| macos | `src/helper.rs` | `HelperClient`, `HelperVerb` | 6 |
| macos | `src/privilege.rs` | sudoers + install | 2 |
| macos | `src/bin/wglane-helper.rs` | root-side verb dispatch | 1 |
| cli | `src/cmd/<name>.rs` | one subcommand: args, help, run | 1 each (8) |
| cli | `src/cmd/completions.rs` | completions | 0 (reference) |
| cli | `src/tui/mod.rs`, `src/tui/app.rs` | dashboard loop, state + drawing | 1, 2 |
| cli | `src/cli.rs`, `src/lib.rs`, `src/context.rs` | wiring; scaffold-owned | 0 |
| xtask | `src/main.rs` | completions, man, release | 1 |

Shared files (`error.rs`, `cli.rs`, `lib.rs`, `Cargo.toml`s) change only to
add a line; land such changes as their own small commit to avoid conflicts.
`cli.rs`'s help snapshot changes when a subcommand's `about` changes: only the
owner of that subcommand should touch `about`.

## Where does X go?

| X | Place |
|---|---|
| New domain type or pure function | `wglane-core/src/<item>.rs`, re-export in `lib.rs` |
| New port | `wglane-core/src/ports/<port>.rs` + fake in `src/testing/<port>.rs` |
| Anything that talks to Nord | `wglane-nord` (`api.rs` transport, `wire.rs` parsing) |
| Anything that runs a macOS program or touches the filesystem | `wglane-macos`, via `CommandRunner` |
| Something that needs root | the helper's verb set (`helper.rs` + `bin/wglane-helper.rs`), nowhere else |
| Subcommand behaviour, flags, help text | `wglane-cli/src/cmd/<name>.rs` |
| Output formatting (text / `--json`) | the subcommand's own file |
| Build/release automation | `xtask` |
| A Linux port | new crate `wglane-linux` implementing the same ports |

## Testing strategy

- Hand-written fakes, not mock frameworks. Fakes live next to their port.
- `insta` snapshots for rendered output: wg configs, help text, TUI frames,
  sudoers rule. Accept new snapshots with `INSTA_UPDATE=always just test` and
  review the `.snap` diff.
- Nord responses come from `crates/wglane-nord/fixtures/` (synthetic).
- No network, no root, no real `wg-quick`/`route` in tests. Only
  `SystemRunner`'s own tests spawn a process (`/bin/sh`).
- `cargo test` (nextest is not assumed).

## What was borrowed

- levish0/AxumKit: `justfile` with a `default: just --list` entry and the
  `.cargo/config.toml` `xtask` alias. Not borrowed: its compose/infra recipes.
- rust-analyzer: the structure of this document.
- Google Rust style guide: see `docs/STYLE.md`.

## Open questions (owner decides)

- Helper argument policy: sudoers rule with no argument list (helper validates)
  vs. one rule per verb. "No wildcards" here means the former.
- How generated configs reach the root-owned dir: helper `up <cc>` copies from
  the user's dir after validation, or `gen` writes via the helper.
- Is `HelperClient::clear` a separate verb or folded into `down`?
- Token file owner check needs `getuid` (libc, `unsafe`): accept a tiny
  `unsafe` exception or keep the mode-only check.
