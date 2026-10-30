# TUI demo -> docs/media/tui.gif (run via `just demo`). Just a keystroke list: edit when the UI changes.
# Timing: Sleep after any key that redraws or hits the network (1.5 s+ after launch, 0.5-1 s between keys).
# Never press Enter on a country/city: that connects. Keep the whole thing under 15 s.
Source demos/settings.tape
Output docs/media/tui.gif
# Hidden start: re-exec the shell where $NOTUN (set by record.sh) hides the real wireguard run dir.
Hide
Type 'exec sandbox-exec -p "$NOTUN" bash' Enter
Sleep 500ms
Type "clear" Enter
Show
Type "wglane tui" Enter
Sleep 2500ms
Down@500ms 2
Right
Sleep 800ms
Down@400ms 3
Sleep 600ms
Type "/"
Type@200ms "za"
Sleep 1500ms
Enter
Sleep 800ms
Type "?"
Sleep 2s
Escape
Sleep 500ms
Type "q"
Sleep 500ms
