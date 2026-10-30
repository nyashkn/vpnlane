# CLI demo -> docs/media/cli.gif (run via `just demo`). Edit commands freely.
Source demos/settings.tape
Output docs/media/cli.gif
# Hidden start: re-exec the shell where $NOTUN (set by record.sh) hides the real wireguard run dir.
Hide
Type 'exec sandbox-exec -p "$NOTUN" bash' Enter
Sleep 500ms
Type "clear" Enter
Show
Type "vpnlane --help | head -16" Enter
Wait+Line@20s /^\$$/
Sleep 1500ms
Type "clear" Enter
Type "vpnlane doctor" Enter
Wait+Line@20s /^\$$/
Sleep 2s
Type "clear" Enter
Type "vpnlane servers | head -6" Enter
Wait+Line@20s /^\$$/
Sleep 1500ms
Type "vpnlane servers ZA --limit 5 | awk '{print $1, $2}'" Enter
Wait+Line@20s /^\$$/
Sleep 2s
