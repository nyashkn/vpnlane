# Demo recordings

`docs/media/{cli,tui}.gif` show only the real terminal: no title card, captions or slides.
Regenerate with `just demo`; it runs `demos/cli.tape` and `demos/tui.tape` through [VHS](https://github.com/charmbracelet/vhs).

Setup: `brew install vhs` (pulls ttyd + ffmpeg), node (for the proxy), network access to api.nordvpn.com. No token, sudo or VPN.

## Why VHS

| Option | Verdict |
|---|---|
| sprintshow (previous) | Every run renders a title card and step caption bars; no option removes them, only `ffmpeg -ss` cropping of the card. Needs a source checkout, Playwright and a browser. Rejected. |
| **VHS** | `.tape` is a plain-text keystroke list; output is the terminal only; GIF is written directly (small, deterministic). One brew install. **Chosen.** |
| hyperframes | HTML/GSAP motion-graphics compositions. Its terminal pieces (`techniques.md` typing effect, `prompt-type-submit-generate`) animate fake text, not a real program, so they cannot show the actual TUI. Not a fit. |

## Editing the scripts

`tui.tape` is a short keystroke list: re-edit it after a UI change and run `just demo`.
`demos/record.sh tui` (after `cargo build -p vpnlane-cli`) re-records only the TUI GIF.
- `Type "x"` types literally; `Type@200ms "za"` slows typing; `Down@400ms 3` repeats a key with a gap.
- `Sleep` after any key that redraws or hits the network (1.5 s+ after launch, 0.5 to 1 s between navigation keys).
- In `cli.tape`, `Wait+Line@20s /^\$$/` waits for the prompt to return, so network latency cannot race the typing.
- Look and size live in `settings.tape` (800x440, Nord). Keep each GIF under ~1.5 MB and 20 s.

## Privacy

`record.sh` runs with an empty temp `HOME`, `VPNLANE_CONFIG_DIR=/tmp/vpnlane-demo`, prompt `$ `, a PATH without
`tailscale`, and `RUST_LOG=error` for the CLI tape (the TUI logs to `<config dir>/tui.log`, so it needs none), and
made-up presets in `config.toml`. For the TUI, `proxy-nord-only.mjs`
tunnels only HTTPS to api.nordvpn.com (the country and server lists load for real, so the palette shows places)
and refuses the plain-HTTP exit-IP lookup, so the exit reads `unknown`; latency and load may still read `-`
(the snapshots in the TUI tests show the bars).
`servers ZA` is piped through `awk` to drop the server IP column. The tunnel indicator and `doctor` reflect the
recording machine's state. Review the frames before committing.

The recording shows the real tunnel state through the real root helper (a read-only `status` query), with a dummy
mode-600 token in the temp config dir. `record.sh` refuses to run unless `vpnlane status` reports `down`, so
a live tunnel can never end up in a GIF; run `vpnlane down` first. `doctor` shows what the recording machine
lacks (for example `tailscale`, which is kept off PATH on purpose).
