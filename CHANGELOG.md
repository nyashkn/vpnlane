# Changelog

All notable changes to wglane. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.4.0](https://github.com/nyashkn/wglane/compare/v0.3.0...v0.4.0) (2026-10-30)


### What's New

* **tui:** palette-first dashboard (variant C, Jump) ([#32](https://github.com/nyashkn/wglane/issues/32)) ([9b3e49d](https://github.com/nyashkn/wglane/commit/9b3e49da7929ee41647f32be628090b4e6949ae3))


### Fixes

* **tui:** label browse columns and make load bars readable ([#36](https://github.com/nyashkn/wglane/issues/36)) ([d1a9606](https://github.com/nyashkn/wglane/commit/d1a9606390a5dab72968bf01c6a8baa5fbeeb8ca))

## [0.3.0](https://github.com/nyashkn/wglane/compare/v0.2.0...v0.3.0) (2026-10-30)

This release adds saved settings and presets, city-level server choice, a leak check in `wglane status`, and a faster way to pick a server.

### What's New

#### Saved settings in `config.toml` ([#24](https://github.com/nyashkn/wglane/issues/24))

Settings can now be saved in `config.toml` instead of being typed each time. In [#31](https://github.com/nyashkn/wglane/issues/31) the config moved to a v2 format with one location value, and wglane now keeps a state file.

#### Presets ([#27](https://github.com/nyashkn/wglane/issues/27))

A preset is a named bundle of settings. You can preview a preset before using it, and wglane tracks which preset is active. Presets can also be applied as overlays on your config ([#31](https://github.com/nyashkn/wglane/issues/31)).

#### Pick a city ([#26](https://github.com/nyashkn/wglane/issues/26))

`wglane servers`, `wglane ls`, `wglane up` and the TUI now work at the city level, not only by country.

#### Faster server choice ([#28](https://github.com/nyashkn/wglane/issues/28))

`wglane up --fastest` connects to the fastest server. `wglane servers --probe` probes servers. The dashboard now does its work in a non-blocking worker.

#### Leak check and a stable `status --json` ([#23](https://github.com/nyashkn/wglane/issues/23))

`wglane status` now includes a leak check, and the `status --json` output is stable. `wglane doctor` also checks UDP port 51820.

### Fixes

* Previewing a preset now resolves the preset first ([#29](https://github.com/nyashkn/wglane/issues/29)).
* Preset routes are kept when the routes refresh ([#29](https://github.com/nyashkn/wglane/issues/29)).
* `default_city` is now scoped correctly ([#29](https://github.com/nyashkn/wglane/issues/29)).

### Smaller changes

* The docs now have terminal-only demo recordings made with VHS ([#30](https://github.com/nyashkn/wglane/issues/30)).

## [0.2.0](https://github.com/nyashkn/wglane/compare/v0.1.1...v0.2.0) (2026-10-30)

This release improves the `wglane tui` dashboard and fixes IPv6 block routes on macOS.

### What's New

#### Search countries in the dashboard ([#18](https://github.com/nyashkn/wglane/issues/18))

The `wglane tui` dashboard now has country search, so you can find a country without scrolling the list.

#### A clearer dashboard ([#20](https://github.com/nyashkn/wglane/issues/20))

The server list is now a continent tree. A state banner shows the current state, and a help overlay lists the keys. The dashboard also respects `NO_COLOR`.

### Fixes

* The latency graph in `wglane tui` is fixed ([#18](https://github.com/nyashkn/wglane/issues/18)).
* The latency graph now scales to the window you can see ([#21](https://github.com/nyashkn/wglane/issues/21)).
* On macOS, IPv6 block routes are now checked with `netstat`, and routes left stranded are recovered ([#16](https://github.com/nyashkn/wglane/issues/16)).

### Smaller changes

* The docs now include CLI and TUI demo recordings ([#19](https://github.com/nyashkn/wglane/issues/19)).

## [0.1.1](https://github.com/nyashkn/wglane/compare/v0.1.0...v0.1.1) (2026-10-30)

A small release that fixes how wglane is published. The command itself does not change.

### Fixes

* Release archives are now uploaded to the release that release-please creates, so the download is attached to the right version ([#14](https://github.com/nyashkn/wglane/issues/14)).
* `Cargo.lock` now stays in step with the released version ([#14](https://github.com/nyashkn/wglane/issues/14)).

### Smaller changes

* The README install instructions are corrected ([#14](https://github.com/nyashkn/wglane/issues/14)).

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
