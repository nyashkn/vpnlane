# Architecture

Read this before changing code. It says what lives where and which way
dependencies point; the code says how. It is
shaped as a bird's-eye view, a map of the code, then the invariants that are
easy to break by accident.

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
   xtask ──▶ wglane-cli (bin `wglane`, bin `wglane-helper`, lib for xtask)
                  │        │
                  ▼        ▼
            wglane-nord  wglane-macos
                  │        │
                  ▼        ▼
            wglane-core  (no I/O, no async, no network, no process spawning)

   `wglane-helper` is a second binary of the `wglane-cli` package; its logic
   lives in `wglane-macos` (`helper_main`).
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
| `Tunnel` | `wglane_macos::WgGo` (root, in helper), `HelperClient` (CLI) | `FakeTunnel` |
| `RouteBypass` | `wglane_macos::RouteTable` (root, in helper), `HelperClient` (CLI) | `FakeRoutes` |
| `BypassProvider` | `wglane_macos::{TailscaleBypass, StaticBypass}` | `FakeBypassProvider` |
| `NetworkWatcher` | `wglane_macos::RouteMonitor` | `FakeWatcher` |
| `TailnetProbe` | `wglane_macos::TailscalePing` | `FakeTailnet` |

External programs in `wglane-macos` run through `CommandRunner`
(`SystemRunner` in production, `wglane_macos::testing::FakeRunner` in tests),
with these exceptions, which spawn directly: the streaming `route -n monitor`
in `watcher.rs`, the `ls -lde` ACL check in `privilege.rs`, the `sysctl -n
kern.boottime` read in `tunnel.rs`, and the helper's own `RootRunner`
(`helper.rs`).

### Command layer

Each `cmd/<name>.rs` has a `run` that builds real adapters and prints, and a
generic function over ports that does the work (`up::connect`,
`down::disconnect`, `status::gather`). `up::connect_real` and
`down::disconnect_real` return structured outcomes and never print, because
the TUI owns the screen; `up::run`/`down::run` print the outcome. The
`cmd::e2e` test drives up, status and down through these functions with fakes.

`status` shows the bypass-route count `up` recorded in `<config_dir>/bypass-routes`
(the helper's own `bypass.json` is in a root-only directory, so the unprivileged
CLI cannot read it). The count is 0 when no tunnel is active.

### Privilege split

The CLI runs unprivileged. `HelperClient` implements `Tunnel` + `RouteBypass`
by running `sudo -n /usr/local/libexec/wglane-helper <verb> <args>`. The helper
(`wglane_macos::helper_main`, started by the thin
`crates/wglane-cli/src/bin/wglane-helper.rs`) runs as root and uses `WgGo` +
`RouteTable` with a `RootRunner`.

**Verbs** (fixed; this set is the future daemon socket protocol): `up <cc>`,
`down`, `active`, `bypass <gw> [<cidr>...]` (0 to 1024 CIDRs). The helper takes
no paths from the caller and validates every argument with the core parsers.
Every verb runs under one `flock` on the state directory, so calls never
interleave. The sudoers rule
names only the helper and has no argument list.

**What root runs.** Never a shell or script, and never a library outside
`/usr/lib` and `/System/Library`. `WgGo` does, in order:

1. `wireguard-go utun` (reports the interface via `WG_TUN_NAME_FILE`).
2. `wg setconf utunN /dev/stdin` with the rendered config minus `Address`.
3. `ifconfig` (address, `up`).
4. `route`: endpoint host route via the physical gateway, then `0.0.0.0/1`
   and `128.0.0.0/1` via the interface.

`down` deletes those routes and the interface's control socket, which ends
`wireguard-go`.

**Which tunnel is ours.** Recorded in the root-only `/var/db/wglane/tunnels`
(`cc utunN endpoint dev:ino:boot`). `active` (a helper verb, because that dir
is private) lists only those, so the user's other tunnels are never counted or
touched. A record is trusted only while its control socket is the same file
from the same boot.

**Trusted install.** `install-helper` copies the helper, `wg` and
`wireguard-go` into root:wheel paths (`/usr/local/libexec/wglane-helper`,
`/usr/local/libexec/wglane/bin`) because a sudoers rule on a user-writable
binary would be a root exploit. It refuses to install unless `otool -L` on
each copy lists only `/usr/lib/` and `/System/Library/` paths (no `@rpath`, no
`..`), or if any ancestor is not root-owned, g/o-unwritable and ACL-free.
`RootRunner` resolves `wg` and `wireguard-go` only there and re-verifies the
canonical file and every ancestor on every exec; `ifconfig`, `route` and
`netstat` are fixed `/sbin` and `/usr/sbin` paths; `PATH` is system dirs only.
Upgrading wireguard-tools or wireguard-go requires re-running `install-helper`.

**Limits.** IPv4 endpoints and the IPv4 tunnel only (IPv6 is not tunnelled, see
the README).

### Network change

`wglane watch` listens to `route -n monitor` through `NetworkWatcher`. On each
change it reads the new physical gateway and calls the helper's `bypass` verb.
The client always makes this call, even when the bypass set is empty (for
example, no `tailscale` CLI): an empty `bypass <gw>` clears the routes recorded
earlier and only re-points the endpoint. The helper first re-points every tunnel endpoint host route at the new gateway
(`Tunnel::refresh_endpoint`), then replaces the Tailscale bypass routes it
recorded earlier (never routes it did not add). Skipping the endpoint step
leaves the tunnel's own packets going to the old gateway: IPv4 stalls while
Tailscale over IPv6 still looks healthy.

## Code map

One concern per file, so changes stay local.

| Crate | File | Owns |
|---|---|---|
| core | `src/country.rs`, `secret.rs`, `server.rs`, `bypass.rs`, `status.rs`, `tunnel_config.rs` | domain types |
| core | `src/render.rs` (+ `snapshots/`) | `render_wg_conf` |
| core | `src/stale.rs`, `src/rank.rs` | `is_stale`, `rank_by_load` |
| core | `src/generate.rs` | `generate_config` use case |
| core | `src/ports/<port>.rs` + `src/testing/<port>.rs` | one port and its fake |
| core | `src/error.rs` | error enum (append variants; shared, keep edits small) |
| nord | `src/token.rs` | token sources |
| nord | `src/wire.rs` + `fixtures/` | JSON shapes, parsing |
| nord | `src/api.rs` | HTTP calls |
| macos | `src/command.rs`, `src/testing.rs` | `CommandRunner`, `FakeRunner` |
| macos | `src/conf_store.rs` | config files on disk |
| macos | `src/tunnel.rs` | `WgGo` (runs as root) |
| macos | `src/routes.rs` | `RouteTable` (runs as root) |
| macos | `src/bypass.rs` | Tailscale + static bypass |
| macos | `src/watcher.rs` | `RouteMonitor` |
| macos | `src/probe.rs` | `TailscalePing` |
| macos | `src/helper.rs` | `HelperClient`, `HelperVerb`, root-side `Helper` + `RootRunner` + `helper_main` |
| macos | `src/privilege.rs` | sudoers, install, trusted-path checks (runs as root) |
| cli | `src/bin/wglane-helper.rs` | entry shim: calls `helper_main` |
| cli | `src/cmd/<name>.rs` | one subcommand: args, help, run |
| cli | `src/cmd/completions.rs` | completions |
| cli | `src/tui/mod.rs`, `src/tui/app.rs` | dashboard loop, state + drawing |
| cli | `src/cli.rs`, `src/lib.rs`, `src/context.rs` | wiring |
| xtask | `src/main.rs` | completions, man, release |

Shared files (`error.rs`, `cli.rs`, `lib.rs`, `Cargo.toml`s) change only to
add a line; keep such changes in their own small commit.
`cli.rs`'s help snapshot changes when a subcommand's `about` changes; review
that `.snap` diff.

## Where does X go?

| X | Place |
|---|---|
| New domain type or pure function | `wglane-core/src/<item>.rs`, re-export in `lib.rs` |
| New port | `wglane-core/src/ports/<port>.rs` + fake in `src/testing/<port>.rs` |
| Anything that talks to Nord | `wglane-nord` (`api.rs` transport, `wire.rs` parsing) |
| Anything that runs a macOS program or touches the filesystem | `wglane-macos`, via `CommandRunner` |
| Something that needs root | a helper verb in `helper.rs`, implemented by `tunnel.rs`, `routes.rs` or `privilege.rs`; nowhere else |
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
- No network, no root, no real `wireguard-go`/`route` in tests. A few tests do
  spawn harmless processes: `SystemRunner`'s own (`/bin/sh`), the privilege
  tests (`/bin/chmod`), and `doctor_exit` (runs the built `wglane` binary).
- `cargo test` (nextest is not assumed).

## Decisions taken

- Sudoers rule has no argument list; the helper validates every argument.
- `up <cc>` takes the three config fields on stdin and renders the config
  inside the root-owned dir; the helper never reads a user-supplied path.
- Bypass cleanup is folded into the `down` verb.
- Token file mode is checked; the owner check is not yet done. It would not need
  `unsafe` (`MetadataExt::uid` and `rustix::process::geteuid` are safe), but
  `wglane-nord` does not depend on `rustix` today.

## Credits

- The `justfile` default entry and the `xtask` alias follow common Rust project
  layouts; the structure of this document follows rust-analyzer's
  `architecture.md`; style rules follow the Google Rust style guide
  (see `docs/STYLE.md`).
