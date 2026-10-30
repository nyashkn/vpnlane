#!/usr/bin/env bash
# Records demos/{cli,tui}.tape to docs/media/{cli,tui}.gif with VHS (brew install vhs; pulls ttyd+ffmpeg).
# Needs a debug build (cargo build -p wglane-cli) and network access to api.nordvpn.com. No token or sudo.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
W=$(mktemp -d); trap 'kill ${PROXY:-} 2>/dev/null || true; rm -rf "$W"' EXIT
mkdir -p "$W/bin" "$W/home"
ln -s "$ROOT/target/debug/wglane" "$W/bin/wglane"
# Privacy: neutral prompt, empty temp HOME, fixed generic config path, PATH without tailscale.
rm -rf /tmp/wglane-demo; mkdir -p /tmp/wglane-demo; chmod 700 /tmp/wglane-demo
# Dummy token (never a real one) so `doctor` reads ok and the TUI shows no token error.
echo demotoken0123456789 > /tmp/wglane-demo/token; chmod 600 /tmp/wglane-demo/token
# Deterministic tunnel state: the tapes re-exec the shell under sandbox-exec hiding the recorder's wireguard run dir (even from the root helper), so the tunnel reads DOWN.
export NOTUN='(version 1)(allow default)(deny file-read* (subpath "/private/var/run/wireguard"))'
export HOME="$W/home" WGLANE_CONFIG_DIR=/tmp/wglane-demo PS1='$ ' \
  PATH="$W/bin:/usr/bin:/bin:/opt/homebrew/bin" RUST_BACKTRACE=0 RUST_LOG=error
unset TAILSCALE_AUTHKEY WGLANE_TOKEN
vhs demos/cli.tape
# Proxy lets only the country-list request through, so the exit-IP lookup fails ("unknown").
node demos/proxy-first-only.mjs 18089 & PROXY=$!; sleep 1
HTTPS_PROXY=http://127.0.0.1:18089 vhs demos/tui.tape
ls -l docs/media/*.gif
