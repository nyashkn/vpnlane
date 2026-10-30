# CLI demo -> docs/media/cli.gif (run via `just demo`). Edit commands freely.
Source demos/settings.tape
Output docs/media/cli.gif
Hide
Type "clear" Enter
Show
Type "wglane --help | head -16" Enter
Wait+Line@20s /^\$$/
Sleep 1500ms
Type "clear" Enter
Type "wglane doctor" Enter
Wait+Line@20s /^\$$/
Sleep 2s
Type "clear" Enter
Type "wglane servers | head -6" Enter
Wait+Line@20s /^\$$/
Sleep 1500ms
Type "wglane servers ZA --limit 5 | awk '{print $1, $2}'" Enter
Wait+Line@20s /^\$$/
Sleep 2s
