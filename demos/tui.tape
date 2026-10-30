# TUI demo -> docs/media/tui.gif (run via `just demo`). Just a keystroke list: edit when the UI changes.
# Walkthrough: home -> palette, search a city -> preview a preset with `p` -> help -> quit.
# Timing: Sleep after any key that redraws or hits the network (2 s+ after launch, 0.5-1 s between keys).
# Never press Enter on a place or a preset, nor a number key: that connects. Keep it under 20 s.
Source demos/settings.tape
Output docs/media/tui.gif
Hide
Type "clear" Enter
Show
Type "vpnlane tui" Enter
Sleep 6s
Space
Sleep 600ms
Type@250ms "san"
Sleep 1500ms
Down@400ms 2
Sleep 600ms
Escape
Sleep 400ms
Type "p"
Sleep 700ms
Down@500ms 1
Sleep 1400ms
Escape
Sleep 400ms
Type "?"
Sleep 1700ms
Escape
Sleep 400ms
Type "q"
Sleep 500ms
