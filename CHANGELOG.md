# Changelog

All notable changes to wglane. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.1.0](https://github.com/nyashkn/wglane/compare/v0.0.1...v0.1.0) (2026-10-30)

First release. wglane connects a Mac to a NordLynx tunnel and keeps Tailscale working alongside it. Install it, run `wglane up <CC>` with a country code, and your tailnet stays reachable.

### What's New

#### Connect and disconnect with `wglane up`, `down` and `status`

`wglane up <CC>` connects to a Nord country over NordLynx, `wglane down` disconnects, and `wglane status` shows the country, exit IP, tailnet check and bypass route count. Host routes via the physical gateway keep Tailscale's control server and DERP relays outside the tunnel. Running `up` again generates configs as needed and switches country.

#### Pick a server with `wglane servers` and `wglane gen`

`wglane servers` lists countries, or ranked servers in one country. `wglane gen` writes tunnel configs without connecting.

#### Check your setup with `wglane doctor`

`wglane doctor` checks prerequisites and prints the fix for each failure. It also checks that the helper's tool directory is root-owned.

#### Stay connected across network changes with `wglane watch`

`wglane watch` re-points the tunnel endpoint route and the bypass routes when your network changes.

#### Dashboard with `wglane tui`

`wglane tui` opens a dashboard with a server list and a stats card, where you pick a country and connect. It is on by default through the `tui` feature.

#### Passwordless `up` and `down` with `wglane install-helper`

`wglane install-helper` installs a root helper and a sudoers rule once, so `up` and `down` need no password. `--uninstall` removes both. The helper runs only wireguard-go and wg, with no bash, wg-quick or user-writable libraries.

#### No IPv6 leaks while a tunnel is up ([#6](https://github.com/nyashkn/wglane/issues/6))

IPv6 is not tunnelled, so it used to go around the VPN. While a tunnel is up, wglane now rejects global IPv6 (`2000::/3`) and the NAT64 prefixes. This is best effort and not a kill switch.

### Fixes

* Stale tunnel records, state locking and status showing the wrong state are fixed.
* The tailnet probe pings the peer's Tailscale IP, so it no longer fails on MagicDNS names.
* The tunnel endpoint route is re-pointed after a network change, even when the bypass set is empty ([#2](https://github.com/nyashkn/wglane/issues/2)).
* Bypass removes only the routes wglane added, and refuses CIDRs that cover the gateway.
* Truncated bypass networks are validated instead of being accepted.

### Smaller changes

* `wglane completions` prints shell completions, and `cargo xtask man` builds man pages.
* Global `--json`, `-v` and `--config-dir` (or `WGLANE_CONFIG_DIR`) options.
* Releases ship `wglane` and `wglane-helper` in GitHub archives for Apple silicon and Intel. Homebrew comes later.
* README and architecture docs now cover the install flow, setup steps and troubleshooting.
* Known limit: there is no kill switch.
