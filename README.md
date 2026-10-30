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
brew install wireguard-tools wireguard-go   # provides wg and wireguard-go
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
wglane doctor       # checks wg, tailscale, helper, sudoers, token, config dir
```

`wglane --help` and `wglane <command> --help` have examples for everything.
`status` and `servers` take `--json`.

## Privilege model

Bringing a tunnel up and editing routes needs root. wglane does not run as
root. `sudo wglane install-helper` copies `wglane-helper` to the root-owned
`/usr/local/libexec/wglane-helper` and adds a sudoers rule that lets you run
only that binary without a password. The helper accepts four verbs (`up <CC>`,
`down`, `active`, `bypass <gateway> <cidr>...`), takes no paths from the caller,
renders the WireGuard config itself from three validated fields, refuses bypass
routes unless the gateway is the real physical gateway, and keeps its state
(including which `utunN` is wglane's) in a root-only directory
(`/var/db/wglane`). Anything that can run commands as your user can use those
verbs, so treat the helper as a capability of your account, not as isolation
from it. Root runs no shell and no script: the helper drives the tunnel itself
with just `wireguard-go` and `wg` plus `/sbin/ifconfig` and `/sbin/route`.
`install-helper` copies `wg` and `wireguard-go` into root-owned
`/usr/local/libexec/wglane/bin` and refuses to install unless `otool -L` shows
each copied binary (and the helper) loads only `/usr/lib` and `/System/Library`
libraries. Before every run the helper refuses either tool unless the file and
every parent directory is root-owned, not group/other-writable and has no ACL.
After upgrading `wireguard-tools` or `wireguard-go`, re-run
`sudo wglane install-helper`. Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Security model and known limits

wglane is pre-alpha and has not been audited. What it does not protect against:

- The sudo rule is a capability of your account. Any process running as you can
  use the helper to send host traffic (`0.0.0.0/1` + `128.0.0.0/1`) to any
  WireGuard peer on the NordLynx port, with keys that process chooses.
  Endpoints are not limited to Nord servers.
- `install-helper` trusts the `/opt/homebrew/bin` symlinks and the running
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
