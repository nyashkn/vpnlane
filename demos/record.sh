#!/usr/bin/env bash
# Records demos/{cli,tui}.md to docs/media/{cli,tui}.gif. Needs: node >= 22.18, ffmpeg,
# a debug build of wglane (cargo build -p wglane-cli), network access to api.nordvpn.com,
# sprintshow is not on npm: clone+build it once (outside this repo, no node deps added here):
#   git clone https://github.com/zookanalytics/sprintshow ~/.cache/sprintshow-src
#   (cd ~/.cache/sprintshow-src && npm ci && npm run build && npx playwright install chromium-headless-shell)
# Needs no token or sudo.
set -euo pipefail
SPRINTSHOW=${SPRINTSHOW:-$HOME/.cache/sprintshow-src}
cd "$(dirname "$0")/.."
ROOT=$PWD
W=$(mktemp -d); trap 'kill ${PROXY:-} 2>/dev/null || true; rm -rf "$W"' EXIT
mkdir -p "$W/bin" "$W/home"
ln -s "$ROOT/target/debug/wglane" "$W/bin/wglane"
# Privacy: neutral prompt, empty temp HOME, a fixed generic config path, and a PATH
# with no tailscale so neither the tailnet peer name nor the user's paths can appear.
rm -rf /tmp/wglane-demo; mkdir -p /tmp/wglane-demo; chmod 700 /tmp/wglane-demo
export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-$HOME/Library/Caches/ms-playwright}"  # macOS default; set for Linux
export HOME="$W/home" WGLANE_CONFIG_DIR=/tmp/wglane-demo PS1='$ ' \
  PATH="$W/bin:/usr/bin:/bin:/opt/homebrew/bin" NO_COLOR= RUST_BACKTRACE=0
unset TAILSCALE_AUTHKEY WGLANE_TOKEN
SS="node $SPRINTSHOW/dist/cli.js"
$SS run demos/cli.md --cwd "$W/home" --no-narrate -o "$W/cli.mp4"
node demos/proxy-first-only.mjs 18089 & PROXY=$!; sleep 1
HTTPS_PROXY=http://127.0.0.1:18089 $SS run demos/tui.md --cwd "$W/home" --no-narrate -o "$W/tui.mp4"
for n in cli tui; do
  ffmpeg -y -loglevel error -i "$W/$n.mp4" -vf "fps=10,scale=800:-1:flags=lanczos,palettegen=max_colors=64" "$W/p.png"
  ffmpeg -y -loglevel error -i "$W/$n.mp4" -i "$W/p.png" -lavfi "fps=10,scale=800:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=none" "docs/media/$n.gif"
  [ -n "${KEEP_MP4:-}" ] && cp "$W/$n.mp4" "$KEEP_MP4/"
done
ls -l docs/media/*.gif
