# Architecture

Read this before changing code. It says what lives where and which way
dependencies point; the code says how. It is
shaped as a bird's-eye view, a map of the code, then the invariants that are
easy to break by accident.

## Bird's-eye view

`vpnlane up ZA` must: find a token, make sure `~/.config/vpnlane/za.conf` exists
and is under 7 days old (regenerating it from the Nord API if not), read the
physical gateway, bring the WireGuard tunnel up through a root-owned helper,
then add host routes via that gateway for every Tailscale control and DERP IP
so the tailnet survives the full tunnel.

Hexagonal-lite: the core defines **ports** (traits) and pure logic; adapter
crates implement the ports; the binary wires adapters into commands. No DDD
layers beyond that.

## Crate graph

```
   xtask ──▶ vpnlane-cli (bin `vpnlane`, bin `vpnlane-helper`, lib for xtask)
                  │        │
                  ▼        ▼
            vpnlane-nord  vpnlane-macos
                  │        │
                  ▼        ▼
            vpnlane-core  (no I/O, no async, no network, no process spawning)

   `vpnlane-helper` is a second binary of the `vpnlane-cli` package; its logic
   lives in `vpnlane-macos` (`helper_main`).
```

Dependency rules (the compiler enforces direction; review enforces the rest):

1. `vpnlane-core` depends on nothing in the workspace and does no I/O. Types
   from `std::io` may appear in errors; calls to it may not.
2. `vpnlane-nord` and `vpnlane-macos` depend only on `vpnlane-core`, never on each
   other. A Linux adapter would be a sibling `vpnlane-linux`.
3. Only `vpnlane-cli` knows concrete adapter types. Library code is generic over
   ports (`fn f<C: ServerCatalog>(c: &C)`), not `dyn`.
4. `thiserror` (`vpnlane_core::Error`) in libraries; `color-eyre` only in
   `vpnlane-cli` and `xtask`.
5. Blocking only. HTTP is `ureq`; no tokio anywhere.

## Ports and adapters

| Port (`vpnlane_core::ports`) | Production adapter | Fake (`vpnlane_core::testing`) |
|---|---|---|
| `ServerCatalog` | `vpnlane_nord::NordApi` | `FakeCatalog` |
| `TokenSource` | `vpnlane_nord::{EnvToken, FileToken, TokenChain}` | `FakeToken` |
| `ExitIpProbe` | `vpnlane_nord::NordApi` | `FakeExitIp` |
| `Tunnel` | `vpnlane_macos::WgGo` (root, in helper), `HelperClient` (CLI) | `FakeTunnel` |
| `RouteBypass` | `vpnlane_macos::RouteTable` (root, in helper), `HelperClient` (CLI) | `FakeRoutes` |
| `BypassProvider` | `vpnlane_macos::{TailscaleBypass, StaticBypass}` | `FakeBypassProvider` |
| `NetworkWatcher` | `vpnlane_macos::RouteMonitor` | `FakeWatcher` |
| `TailnetProbe` | `vpnlane_macos::TailscalePing` | `FakeTailnet` |
| `UdpProbe` | `vpnlane_macos::UdpSend` | `FakeUdp` |
| `Prober` | `vpnlane_macos::PingProbe` (system `ping`, bound to the physical interface while a tunnel is up) | `FakeProber` |

External programs in `vpnlane-macos` run through `CommandRunner`
(`SystemRunner` in production, `vpnlane_macos::testing::FakeRunner` in tests),
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

`status` shows the bypass-route count `up` recorded in `<config_dir>/state.json`
(the helper's own `bypass.json` is in a root-only directory, so the unprivileged
CLI cannot read it). The count is 0 when no tunnel is active.

Runtime state lives in one file, `<config_dir>/state.json` (schema:
`vpnlane_core::State`, documented in [CONFIG.md](CONFIG.md)), never in
`config.toml`. `Context` is the only reader and writer. `up` records the
resolved request (`LastRequest`: place, pick, bypass set, preset); `down` keeps
it and clears the active preset; `watch` and TUI connects read the bypass set
from it, so a preset's routes survive a network change or a connect to another
country. Older releases kept `active-preset`, `bypass-routes` and `<cc>.city`
files; they are imported once and deleted.

### Privilege split

The CLI runs unprivileged. `HelperClient` implements `Tunnel` + `RouteBypass`
by running `sudo -n /usr/local/libexec/wglane-helper <verb> <args>`. The helper
(`vpnlane_macos::helper_main`, started by the thin
`crates/vpnlane-cli/src/bin/vpnlane-helper.rs`) runs as root and uses `WgGo` +
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
5. `route add -inet6 -net <prefix> ::1 -reject` for `2000::/3`, `64:ff9b::/96`
   and `64:ff9b:1::/48` (NAT64): IPv6 is rejected while a tunnel is up. `route`
   exits 0 even when the kernel refuses, so each add is checked with
   `netstat -rn -f inet6` (destination parsed as that exact prefix, gateway `::1`,
   flag `R`, netif `lo0`; `route get` cannot see reject routes) and a
   failed check fails `up`. The routes are shared; later tunnels find them in
   place.

`down` deletes those routes (the v6 blocks with the last tunnel, or when `prune`
drops the last dead record, and only after the same check shows they are ours) and the interface's control socket, which ends
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

**Limits.** IPv4 endpoints and the IPv4 tunnel only. IPv6 is not tunnelled: it
is rejected on a best-effort basis (interface-bound sockets can bypass unscoped
routes; not a kill switch). Tailscale's IPv6 underlay may fall back to
IPv4/DERP, which have bypass routes; this is unverified. See the README.

### Network change

`vpnlane watch` listens to `route -n monitor` through `NetworkWatcher`. On each
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
| core | `src/location.rs` | `Location` (`CC` or `CC/City`), the one parser for every layer |
| core | `src/config.rs`, `src/preset.rs` | `config.toml` schema, old-key aliases, `Effective::request` (base, preset, env, flags), `describe_change` |
| core | `src/state.rs` | `State`, `LastRequest`: the `state.json` schema (no I/O) |
| core | `src/render.rs` (+ `snapshots/`) | `render_wg_conf` |
| core | `src/stale.rs`, `src/rank.rs` | `is_stale`, `rank_by_load` |
| core | `src/fastest.rs` | `pick_fastest`, `Choice` (pure choice by measured latency) |
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
| macos | `src/udp_probe.rs` | `UdpSend` |
| macos | `src/ping_probe.rs` | `PingProbe`, `physical_interface` |
| macos | `src/helper.rs` | `HelperClient`, `HelperVerb`, root-side `Helper` + `RootRunner` + `helper_main` |
| macos | `src/privilege.rs` | sudoers, install, trusted-path checks (runs as root) |
| cli | `src/bin/vpnlane-helper.rs` | entry shim: calls `helper_main` |
| cli | `src/cmd/<name>.rs` | one subcommand: args, help, run |
| cli | `src/cmd/completions.rs` | completions |
| cli | `src/cmd/fastest.rs` | concurrent probing, probe scope (physical interface while a tunnel is up) |
| cli | `src/tui/mod.rs`, `src/tui/app.rs` | dashboard loop and effects; state, reducer, browse tree |
| cli | `src/tui/{hud,palette,presets,places,actions}.rs` | home HUD, jump palette, preset slots and preview, place samples and bars, action registry |
| cli | `src/tui/worker.rs`, `src/tui/probe.rs` | effect worker threads; probe results and their ordering |
| cli | `src/cli.rs`, `src/lib.rs`, `src/context.rs` | wiring |
| xtask | `src/main.rs` | completions, man, release |

Shared files (`error.rs`, `cli.rs`, `lib.rs`, `Cargo.toml`s) change only to
add a line; keep such changes in their own small commit.
`cli.rs`'s help snapshot changes when a subcommand's `about` changes; review
that `.snap` diff.

## Where does X go?

| X | Place |
|---|---|
| New domain type or pure function | `vpnlane-core/src/<item>.rs`, re-export in `lib.rs` |
| New port | `vpnlane-core/src/ports/<port>.rs` + fake in `src/testing/<port>.rs` |
| Anything that talks to Nord | `vpnlane-nord` (`api.rs` transport, `wire.rs` parsing) |
| Anything that runs a macOS program or touches the filesystem | `vpnlane-macos`, via `CommandRunner` |
| Something that needs root | a helper verb in `helper.rs`, implemented by `tunnel.rs`, `routes.rs` or `privilege.rs`; nowhere else |
| Subcommand behaviour, flags, help text | `vpnlane-cli/src/cmd/<name>.rs` |
| Output formatting (text / `--json`) | the subcommand's own file |
| Build/release automation | `xtask` |
| A Linux port | new crate `vpnlane-linux` implementing the same ports |

## Testing strategy

- Hand-written fakes, not mock frameworks. Fakes live next to their port.
- `insta` snapshots for rendered output: wg configs, help text, TUI frames,
  sudoers rule. Accept new snapshots with `INSTA_UPDATE=always just test` and
  review the `.snap` diff.
- Nord responses come from `crates/vpnlane-nord/fixtures/` (synthetic).
- No network, no root, no real `wireguard-go`/`route` in tests. A few tests do
  spawn harmless processes: `SystemRunner`'s own (`/bin/sh`), the privilege
  tests (`/bin/chmod`), and `doctor_exit` (runs the built `vpnlane` binary).
- `cargo test` (nextest is not assumed).

## Decisions taken

- Sudoers rule has no argument list; the helper validates every argument.
- `up <cc>` takes the three config fields on stdin and renders the config
  inside the root-owned dir; the helper never reads a user-supplied path.
- Bypass cleanup is folded into the `down` verb.
- Token file mode is checked; the owner check is not yet done. It would not need
  `unsafe` (`MetadataExt::uid` and `rustix::process::geteuid` are safe), but
  `vpnlane-nord` does not depend on `rustix` today.

## Credits

- The `justfile` default entry and the `xtask` alias follow common Rust project
  layouts; the structure of this document follows rust-analyzer's
  `architecture.md`; style rules follow the Google Rust style guide
  (see `docs/STYLE.md`).
