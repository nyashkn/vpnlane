#!/usr/bin/env bash
# Records demos/{cli,tui}.tape to docs/media/{cli,tui}.gif with VHS (brew install vhs; pulls ttyd+ffmpeg).
# `demos/record.sh tui` (or `cli`) records only that one.
# Needs a debug build (cargo build -p vpnlane-cli) and network access to api.nordvpn.com. No token or sudo.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$PWD
W=$(mktemp -d); trap 'kill ${PROXY:-} 2>/dev/null || true; rm -rf "$W"' EXIT
mkdir -p "$W/bin" "$W/home"
ln -s "$ROOT/target/debug/vpnlane" "$W/bin/vpnlane"
# Privacy: neutral prompt, empty temp HOME, fixed generic config path, PATH without tailscale.
rm -rf /tmp/vpnlane-demo; mkdir -p /tmp/vpnlane-demo; chmod 700 /tmp/vpnlane-demo
# Dummy token (never a real one) so `doctor` reads ok and the TUI shows no token error.
echo demotoken0123456789 > /tmp/vpnlane-demo/token; chmod 600 /tmp/vpnlane-demo/token
# Made-up presets so the TUI strip and preview have something to show.
cat > /tmp/vpnlane-demo/config.toml <<'TOML'
location = "DE/Frankfurt"
preset_order = ["work", "stream", "travel"]

[presets.work]
location = "DE/Frankfurt"
bypass_extra = ["198.51.100.0/24"]

[presets.stream]
location = "US/New York"
pick = "fastest"

[presets.travel]
location = "ZA"
TOML
# The recording shows the real tunnel state, so refuse to record while a tunnel is up (the TUI would show it).
# `status` asks the root helper (read-only); anything but "down" aborts.
if ! "$ROOT/target/debug/vpnlane" status --json 2>/dev/null | grep -q '"state": "down"'; then
  echo "record.sh: a vpnlane tunnel is up or the helper did not answer; run 'vpnlane down' (or 'sudo vpnlane install-helper') first." >&2
  exit 1
fi
export HOME="$W/home" VPNLANE_CONFIG_DIR=/tmp/vpnlane-demo PS1='$ ' \
  PATH="$W/bin:/usr/bin:/bin:/opt/homebrew/bin" RUST_BACKTRACE=0 RUST_LOG=error
unset TAILSCALE_AUTHKEY VPNLANE_TOKEN
ONLY=${1:-all}
[ "$ONLY" = tui ] || vhs demos/cli.tape
if [ "$ONLY" != cli ]; then
  # The proxy tunnels only HTTPS to api.nordvpn.com (country and server lists load for real) and refuses the
  # plain-HTTP exit-IP lookup, so the exit reads "unknown". Latency and load may read `-` (ICMP probes).
  node demos/proxy-nord-only.mjs 18089 & PROXY=$!; sleep 1
  # No RUST_LOG here: the TUI sends logs to <config dir>/tui.log, so nothing paints over it.
  env -u RUST_LOG HTTPS_PROXY=http://127.0.0.1:18089 vhs demos/tui.tape
fi
ls -l docs/media/*.gif
