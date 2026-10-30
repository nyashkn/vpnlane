# wglane

NordLynx (WireGuard) tunnels on macOS that leave Tailscale and Headscale working.

> **Status: pre-alpha (v0.1 in progress).** `up`, `down`, `status`, `servers`,
> `gen`, `doctor`, `watch`, `tui` and `install-helper` are implemented and unit
> tested against fakes. It has not had a real-world soak: expect rough edges and
> check `wglane doctor` first. macOS only. Not packaged yet; build from source.

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

## Install

From source (no release is published yet):

```sh
brew install wireguard-tools          # provides wg-quick
cargo install --path crates/wglane-cli
sudo wglane install-helper            # one time, see "Privilege model"
wglane doctor                         # what is still missing, and the fix
```

## Quick use

```sh
# Access token from Nord account > Access token. Never pass it as an argument.
mkdir -p ~/.config/wglane && (umask 077; pbpaste > ~/.config/wglane/token)
# or: export WGLANE_TOKEN=...   (the env var wins over the file)

wglane up ZA        # connect (the config is generated or refreshed as needed)
wglane status       # country, exit IP, tailnet check, bypass routes
wglane down
wglane tui          # dashboard: pick a country, connect, live latency
wglane watch        # re-apply bypass routes when you change networks
wglane doctor       # checks wg-quick, tailscale, helper, sudoers, token, config dir
```

`wglane --help` and `wglane <command> --help` have examples for everything.
`status` and `servers` take `--json`.

## Privilege model

Bringing a tunnel up and editing routes needs root. wglane does not run as
root. `sudo wglane install-helper` copies `wglane-helper` to the root-owned
`/usr/local/libexec/wglane-helper` and adds a sudoers rule that lets you run
only that binary without a password. The helper accepts three verbs (`up <CC>`,
`down`, `bypass <gateway> <cidr>...`), takes no paths from the caller, renders
the WireGuard config itself from three validated fields, refuses bypass routes
unless the gateway is the real physical gateway, and keeps its state in a
root-only directory (`/var/db/wglane`). Anything that can run commands as your
user can use those three verbs, so treat the helper as a capability of your
account, not as isolation from it. Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The gate is `just ci`.

## License

MIT. See [LICENSE](LICENSE).
