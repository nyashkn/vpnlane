# Changelog

All notable changes to wglane. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added

- `wglane up <CC>` / `down` / `status`: connect to a Nord country over
  NordLynx, disconnect, and show country, exit IP, tailnet check and bypass
  route count. Host routes via the physical gateway keep Tailscale's control
  server and DERP relays outside the tunnel.
- `wglane servers` (countries, or ranked servers in one country) and
  `wglane gen` (write tunnel configs without connecting).
- `wglane doctor`: checks prerequisites and prints the fix for each failure.
- `wglane watch`: re-points the tunnel endpoint route and the bypass routes when
  the network changes.
- `wglane tui`: dashboard to pick a country and connect (default `tui` feature).
- `wglane install-helper` (and `--uninstall`): one-time root helper plus sudoers
  rule for passwordless `up`/`down`.
- `wglane completions` for shell completions; man pages via `cargo xtask man`.
- Global `--json`, `-v` and `--config-dir` / `WGLANE_CONFIG_DIR`.

### Known issues

- IPv6 is not tunnelled: on a dual-stack network IPv6 traffic bypasses the VPN.
  There is no kill switch.
