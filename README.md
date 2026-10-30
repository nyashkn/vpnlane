# wglane

Use NordVPN (NordLynx/WireGuard) on macOS without breaking Tailscale or
Headscale. One command connects to a Nord country; your tailnet keeps working,
and it keeps working when you change Wi-Fi (leave `wglane watch` running in a
terminal for that). Rust CLI and TUI.

> **Status: pre-alpha.** Unit tested against fakes and smoke-tested by hand on
> one Mac across several networks (home Wi-Fi and two phone hotspots). Not
> audited, no long soak: expect rough edges and run `wglane doctor` first.
> macOS only. No release is published yet, so build from source (once
> released: `brew install nyashkn/tap/wglane-cli`). **Known gap:** IPv6 is not tunnelled; it
> is blocked while the tunnel is up (see
> [Security model and known limits](#security-model-and-known-limits)).

## Why

A full WireGuard tunnel (`AllowedIPs = 0.0.0.0/0`) also captures Tailscale's
own underlay traffic: the control server and the DERP relays. `tailscale
netcheck` then reports `UDP: false` and the tailnet dies, even though
`100.64.0.0/10` still routes. The Nord macOS app, and `wg-quick` with a
`DNS =` line, also replace the resolver and break MagicDNS.

wglane brings the tunnel up without a `DNS =` line and adds host routes via
your physical gateway for the control server and every DERP IP. The tailnet
keeps working; everything else exits through the VPN.

wglane is not affiliated with Nord Security. It uses Nord's public API to
fetch your WireGuard key and server list.

## How it works

```
 you ── wglane up ZA ──┐
                       ▼
   ┌──────────────────────────────┐   unprivileged (your user)
   │ wglane  (CLI / TUI / watch)  │   token ─▶ Nord API ─▶ za.conf (key + server)
   └───────────────┬──────────────┘
                   │ sudo, one rule, one binary
                   ▼
   ┌──────────────────────────────┐   root, no shell
   │ wglane-helper                │   wireguard-go + wg + ifconfig + route
   └───────────────┬──────────────┘
                   ▼
   routing table
     0.0.0.0/1, 128.0.0.0/1 ──────────────▶ utunN ─▶ Nord (everything else)
     Nord server /32      ─┐
     Tailscale control /32 ├─ via your real gateway (en0), NOT the tunnel
     every DERP relay /32 ─┘
     100.64.0.0/10 ──────────────────────▶ Tailscale (unchanged)

   wglane watch: Wi-Fi changes ─▶ re-point those host routes at the new gateway
```

Surviving a Wi-Fi change needs `wglane watch` running in the foreground. Without
it the routes keep pointing at the old gateway.

The tunnel's own packets and Tailscale's underlay must skip the tunnel or the
tailnet dies. That is the whole trick: host routes via the physical gateway,
no `DNS =` line, and a watcher that keeps them correct as networks change.

## Install and set up

Do these in order.

1. Clone: `git clone https://github.com/nyashkn/wglane && cd wglane`
2. Prerequisites:
   - macOS with the Xcode Command Line Tools (`xcode-select --install`).
   - Rust, via [rustup](https://rustup.rs).
   - Homebrew packages: `brew install wireguard-tools wireguard-go`.
   - The `tailscale` CLI on your `PATH`. wglane needs it to find the control
     server and relays it must protect; without it `up` installs 0 bypass
     routes and the tailnet is not protected. With the Tailscale app: Settings
     > Install CLI.
   - A NordVPN subscription. Quit the Nord app first; two VPNs on one Mac conflict.
3. Build and install: `just install`. It runs the three commands below, with the
   full path to `wglane` because `sudo` does not search `~/.cargo/bin`:

   ```sh
   cargo install --path crates/wglane-cli --locked   # wglane and wglane-helper
   sudo "$(command -v wglane)" install-helper        # one time, asks for your password
   wglane doctor                                     # what is still missing, and the fix
   ```

   After this, `up` and `down` need no password.
4. Create an access token and save it in the same step. Click path and
   details: [docs/TOKEN.md](docs/TOKEN.md) (Nord Account > NordVPN > Advanced
   settings > Get access token). Copy it, then:

   ```sh
   mkdir -p ~/.config/wglane && (umask 077; pbpaste > ~/.config/wglane/token)
   ```

   Or `export WGLANE_TOKEN=...` (the env var wins over the file). Never pass
   the token as an argument.
5. `wglane doctor` should print `ok` on every line.
6. `wglane up US`, then `wglane status`: the exit IP is Nord's and `tailnet`
   says `ok`. Run `wglane down` when done. (`up` leaves the rendered key in a
   root-only state directory until `down` removes it.)

## Quick use

```sh
# Token saved as in "Install and set up" above.
wglane up ZA        # connect (the config is generated or refreshed as needed)
wglane status       # country, exit IP, tailnet check, bypass routes
wglane down
wglane tui          # dashboard: pick a country, connect, live latency
wglane watch        # re-apply bypass routes when you change networks
wglane doctor       # checks wg, tailscale, helper, sudoers, token, config dir
wglane servers [CC] # list countries, or ranked servers in one country
wglane gen ZA       # write the WireGuard config without connecting
wglane completions zsh   # shell completions
sudo "$(command -v wglane)" install-helper --uninstall   # remove the helper
```

`wglane --help` and `wglane <command> --help` have examples for everything.
`--json` works on every command, `-v`/`-vv`/`-vvv` add log output, and
`--config-dir` (or `WGLANE_CONFIG_DIR`) moves the config directory.

## Troubleshooting

- `doctor` names the missing piece and the fix. Start there.
- Internet works but the tailnet is down: run `wglane status`. If bypass routes
  read `0`, check that `tailscale status` works and the `tailscale` CLI is on
  your `PATH`, then run `wglane up <CC>`.
- After switching Wi-Fi with no `wglane watch` running, routes point at the old
  gateway. Run `wglane watch` (leave it open) or `wglane up <CC>` again.
- After upgrading `wireguard-tools`/`wireguard-go`: `sudo wglane install-helper`.

## Privilege model

Bringing a tunnel up and editing routes needs root; wglane itself does not run
as root. `sudo wglane install-helper` copies a small helper to a root-owned path
and adds one sudoers rule that lets you run only that binary without a password.
The helper has four fixed verbs, takes no paths from the caller, runs no shell,
and refuses bypass routes unless the gateway is the real physical gateway. It
needs `wg` and `wireguard-go` from Homebrew: `install-helper` copies them into a
root-owned directory, so after upgrading either, re-run it. Anything that can run
commands as your user can use the helper, so treat it as a capability of your
account, not isolation from it. Full design:
[ARCHITECTURE.md, "Privilege split"](docs/ARCHITECTURE.md#privilege-split) and
"Trusted install" in the same section.

## Security model and known limits

wglane is pre-alpha and has not been audited. What it does not protect against:

- IPv6. The Nord config is IPv4-only, so while a tunnel is up wglane rejects
  global IPv6 (`2000::/3`) and apps fall back to IPv4 through Nord. Loopback,
  link-local and Tailscale's `fd7a::/48` are untouched. Tailscale's IPv6
  underlay path is cut too and falls back to IPv4/DERP, which have bypass
  routes. This is not a kill switch: if the tunnel drops, IPv4 is not blocked.
- The sudo rule is a capability of your account. Any process running as you can
  use the helper to send host traffic (`0.0.0.0/1` + `128.0.0.0/1`) to any
  WireGuard peer on the NordLynx port, with keys that process chooses.
  Endpoints are not limited to Nord servers.
- `install-helper` trusts the `/opt/homebrew/bin` and `/usr/local/bin` symlinks and the running
  executable (`current_exe`) at install time. If either was already replaced,
  root copies the replacement. `otool -L` does not see `dlopen`,
  `LC_DYLD_ENVIRONMENT` or `LC_LOAD_DYLINKER`, so the dependency check is a
  filter, not a proof.
- Tunnel records survive reboot but `utunN` numbers are reused. A record is
  trusted only while its control socket is the same file (device, inode) from
  the same boot; otherwise it is dropped and nothing is torn down. A tunnel
  whose `wireguard-go` died on its own therefore leaves its endpoint host route
  behind until you remove it (`sudo route delete <endpoint>`).
- `/var/run` is `root:daemon 0775` on macOS; the helper accepts that and trusts
  the `daemon` group. A user added to `daemon` defeats the run-dir check.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The gate is `just ci`.

## License

MIT. See [LICENSE](LICENSE).
