# Demo recordings

`cli.md` and `tui.md` are [sprintshow](https://github.com/zookanalytics/sprintshow)
terminal scripts; `docs/media/{cli,tui}.gif` are made from them.

Regenerate with `just demo`. One-time setup (sprintshow is not on npm; nothing is added to this repo):

```sh
git clone https://github.com/zookanalytics/sprintshow ~/.cache/sprintshow-src
cd ~/.cache/sprintshow-src && npm ci && npm run build && npx playwright install chromium-headless-shell
```

Also needs node >= 22.18, ffmpeg and network access to api.nordvpn.com. No token, sudo or VPN.

Privacy: `record.sh` runs the shell with an empty temp `HOME`, `WGLANE_CONFIG_DIR=/tmp/wglane-demo`,
prompt `$ `, and a PATH without `tailscale`. The recording trims sprintshow's 3 s title card. For the TUI, `proxy-first-only.mjs` lets only the
country-list request through, so the exit-IP lookup fails and the panel reads `unknown`.
The `servers ZA` output is piped through `awk` to drop the server IP column.
`doctor` reflects the recording machine's helper/wireguard-tools state. Review the frames before committing.
