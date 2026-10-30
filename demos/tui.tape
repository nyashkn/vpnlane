# TUI demo -> docs/media/tui.gif (run via `just demo`). Just a keystroke list: edit when the UI changes.
# Timing: Type = chars (40ms each); Sleep after any key that triggers a redraw or a network call.
# Use Sleep 1.5s+ after launch (country list fetch), 500-800ms between navigation keys.
Source demos/settings.tape
Output docs/media/tui.gif
Hide
Type "clear" Enter
Show
Type "wglane tui" Enter
Sleep 2.5s
Enter
Sleep 600ms
Down@400ms 3
Sleep 800ms
Type "/"
Type@200ms "za"
Sleep 1s
Enter
Sleep 1s
Type "?"
Sleep 2s
Escape
Sleep 500ms
Type "q"
Sleep 500ms
