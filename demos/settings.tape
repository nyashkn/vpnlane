# Shared look for both recordings. Terminal window only: no title card, captions or slides.
Set Shell "bash"
Set Width 800
Set Height 440
Set FontSize 14
Set Padding 12
Set Theme "nord"
Set TypingSpeed 40ms
Set Framerate 15
# Timing convention: after every shell command, `Wait+Line@20s /^\$$/` blocks until the prompt
# is back (output finished, network included), so no fixed sleeps race the API.
