# Changelog

All notable changes to wglane. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [0.1.0](https://github.com/nyashkn/wglane/compare/v0.0.1...v0.1.0) (2026-10-30)


### What's New

* **cli:** generate_config, gen and servers commands ([edc4f00](https://github.com/nyashkn/wglane/commit/edc4f0031e90a21a9645f74b50fe8ad8960374f0))
* **cli:** status and doctor commands ([b61c3a0](https://github.com/nyashkn/wglane/commit/b61c3a06c1c3c6259ed2546ec4eb5c1f685be21e))
* **cli:** up with auto-gen and switch, down ([4cc713e](https://github.com/nyashkn/wglane/commit/4cc713ebb39cad0fb7a527e06798b0d61e808385))
* **cli:** wire real adapters into up/down/status/tui, add e2e test, sync docs ([00f83c6](https://github.com/nyashkn/wglane/commit/00f83c6a2fe61b61b97311f0c542556982f6e0af))
* **doctor:** check root-owned tool dir, require bash&gt;=4 at install, exit quietly, watch records route count ([529a981](https://github.com/nyashkn/wglane/commit/529a981f5d012b7147faf2b86e02cf117527b0e0))
* **macos,cli:** route monitor watcher and wglane watch ([cb98d6a](https://github.com/nyashkn/wglane/commit/cb98d6af0b5a326627a4b1648cf171fee4c10977))
* **macos:** conf store, Tailscale bypass provider, tailnet probe ([1293268](https://github.com/nyashkn/wglane/commit/129326843a3a742c1d3975056aaa7b318c8e4ad5))
* **macos:** privileged helper binary and sudo client ([5b29603](https://github.com/nyashkn/wglane/commit/5b2960315612b25823e19b7b5f869a6903eb204c))
* **macos:** reject global IPv6 while a tunnel is up ([#6](https://github.com/nyashkn/wglane/issues/6)) ([e372530](https://github.com/nyashkn/wglane/commit/e372530bf414c7949ec5705581a28c49416bada0))
* **macos:** sudoers rule + install-helper ([b641fb7](https://github.com/nyashkn/wglane/commit/b641fb7a015e541e4459ff90e3c19a019a3cb909))
* **macos:** wg-quick tunnel and route table adapters ([510a9cb](https://github.com/nyashkn/wglane/commit/510a9cb69d297eb2228fd69fcf010323453e4e6b))
* **nord:** API client and wire parsing ([9fdcf64](https://github.com/nyashkn/wglane/commit/9fdcf644b7e38fc8cf092f9c627e7e521e5eacf6))
* **release:** release-please flow with version-agreement check ([#3](https://github.com/nyashkn/wglane/issues/3)) ([23699b1](https://github.com/nyashkn/wglane/commit/23699b1a3841237e44e1d20d1886a3a793fd3b2e))
* scaffold wglane workspace ([83a2de0](https://github.com/nyashkn/wglane/commit/83a2de00c192b1cef5e2e47f2729b118f007ffdc))
* **tui:** reducer-based dashboard with server list and stats card ([12f9290](https://github.com/nyashkn/wglane/commit/12f9290807c226f67b54c6001975251ce392df14))
* **xtask:** release builds wglane binaries; add cargo-dist config ([b65f9bf](https://github.com/nyashkn/wglane/commit/b65f9bfe528355c4ea6db351c9b42d029566489f))


### Fixes

* **helper:** root drives wireguard-go and wg only; no bash, wg-quick or user-writable dylibs ([e3ca3e6](https://github.com/nyashkn/wglane/commit/e3ca3e6fcb15c83aa09e72fccdd912fef93b45bd))
* **helper:** run only root-owned wg-quick/bash/wg/wireguard-go ([be0676b](https://github.com/nyashkn/wglane/commit/be0676b6c5f557aaee3d51c1dd9cf8da9e9612a0))
* **macos:** stale tunnel records, state lock, run-dir trust, otool parsing, status unknown ([e69615f](https://github.com/nyashkn/wglane/commit/e69615fba941672f874198b164f981a578f523ba))
* **macos:** validate truncated bypass network; shared min prefix in RouteTable ([594431f](https://github.com/nyashkn/wglane/commit/594431f41dcfbcfe7f9f2ecf8ad0952bda251f83))
* **probe:** ping the peer's Tailscale IP, not its MagicDNS name ([956fd62](https://github.com/nyashkn/wglane/commit/956fd62f539373949e10704fc9e669b0336d1903))
* **release:** allow the github-actions bot to run release-notes ([#12](https://github.com/nyashkn/wglane/issues/12)) ([a36f94e](https://github.com/nyashkn/wglane/commit/a36f94e5b52200b313d1330f3c8f5cfb8bc1c118))
* **release:** credential scan flags values, not the word 'token' ([#10](https://github.com/nyashkn/wglane/issues/10)) ([b282855](https://github.com/nyashkn/wglane/commit/b2828559f34566b033e0fa04c3cbca802d6e46a5))
* **release:** give claude-code-action a github_token so it skips OIDC ([#11](https://github.com/nyashkn/wglane/issues/11)) ([8d64f1e](https://github.com/nyashkn/wglane/commit/8d64f1e89737ef6ccc6c74bb5a349315b1c4fe73))
* **release:** run the notes step the way bd-board does ([#13](https://github.com/nyashkn/wglane/issues/13)) ([31fdd9f](https://github.com/nyashkn/wglane/commit/31fdd9f74e14537dea9991763d69e91ce5f5d97f))
* **release:** version-independent help snapshot; dispatch notes with GITHUB_TOKEN ([#9](https://github.com/nyashkn/wglane/issues/9)) ([1c0f380](https://github.com/nyashkn/wglane/commit/1c0f3809d3fbcaf84387addda2cc852c7b711c1b))
* **routes:** bypass deletes only recorded routes, refuses CIDRs covering the gateway ([79812de](https://github.com/nyashkn/wglane/commit/79812de51e4bbfc0356ad0f451c90359e5379b0a))
* **test:** point the no-bash scan at the helper bin's new home in wglane-cli ([e571225](https://github.com/nyashkn/wglane/commit/e57122570ed534df1986896d76e1ecde77c875e6))
* **watch:** re-point the tunnel endpoint host route on network change ([d128006](https://github.com/nyashkn/wglane/commit/d1280069a952b5bf2ba346e6a104620efd28807a))
* **watch:** refresh the endpoint route even when the bypass set is empty ([#2](https://github.com/nyashkn/wglane/issues/2)) ([71310f4](https://github.com/nyashkn/wglane/commit/71310f493bb7a000b092f7453623357ed4d5ddf2))

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
- While a tunnel is up, global IPv6 (`2000::/3`) and the NAT64 prefixes are rejected, best effort (not a kill switch), so IPv6 does not leak outside the VPN.

### Known issues

- IPv6 is not tunnelled (rejected best effort while up). There is no kill switch.
