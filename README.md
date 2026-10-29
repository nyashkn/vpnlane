# wglane

NordLynx (WireGuard) tunnels on macOS that leave Tailscale and Headscale working.

> **Status: pre-alpha.** The workspace, CLI surface and tests exist; most
> commands still answer `not implemented yet`. Do not use it yet.

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

## Install (once released)

```sh
brew install wireguard-tools          # wg-quick
cargo install --path crates/wglane-cli
sudo wglane install-helper            # one time: passwordless up/down
```

## Quick use

```sh
# Access token from Nord account > Access token. Never pass it as an argument.
mkdir -p ~/.config/wglane && (umask 077; pbpaste > ~/.config/wglane/token)
# or: export WGLANE_TOKEN=...

wglane up ZA        # connect (the config is generated or refreshed as needed)
wglane status       # country, exit IP, tailnet check, bypass routes
wglane down
wglane doctor       # what is missing, and the command that fixes it
```

`wglane --help` and `wglane <command> --help` have examples for everything.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). The gate is `just ci`.

## License

MIT. See [LICENSE](LICENSE).
