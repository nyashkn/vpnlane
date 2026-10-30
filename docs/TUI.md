# Dashboard (`wglane tui`)

`wglane tui` shows a one-line header on top, the server list on the left and tunnel status on the right. The status refreshes every 2 s. It needs a terminal of at least 40x8; smaller terminals get a notice until you resize. Build it with the default `tui` feature (`--no-default-features` drops it).

## Keys

Press `?` in the dashboard to see this list.

| Key | Does |
|---|---|
| `enter` | connect to the selected country, or open/close a group |
| `space` | open/close a group |
| `h` / `l` | close / open a group (`h` on a country closes its group) |
| left / right | left closes a group, or jumps from a country to its group header; right opens a closed group, or steps into an open one |
| `j` `k`, up/down | move |
| `g` `G`, home/end | first / last row |
| `/` | search by code or name; `enter` keeps the filter, `esc` clears it |
| `d` | disconnect |
| `r` | reload the server list (after a failed load) |
| `esc` | clear the filter; with no filter, quit |
| `q` | quit |
| `?` | help; any key closes it |

While you are searching, every key is part of the query, so `q`, `d`, `j` and the rest are typed as letters.

## Header and latency

The top line summarises the tunnel: `● up  US  283 ms ▁▂▃▂▁  exit 203.0.113.7`, `○ down` or `◐ connecting to US...`. On narrow terminals it drops the mini graph first, then the exit ip, then the latency. `NO_COLOR` removes its color too.

In the stats card, the `latency` line is the ping to the tailnet peer through the tunnel (the same value as the `tailnet ok (..)` line), with min and max over the samples shown. The 2-row graph below it is scaled to the min..max of those samples, so a steady link draws a low flat line and the shape of any wobble is visible; it is not an absolute scale. Samples above 1.5x the median are drawn in yellow.

## Server tree

Countries are grouped by continent: Africa, Asia, Europe, Middle East, North America, Oceania, South America. The groups come from a fixed table in `crates/wglane-cli/src/tui/continent.rs`, because Nord's API has no region field. A code the table does not know goes under **Other**, so a new Nord country still shows up. When that happens, add the code to the table and to the `NORD` list in its test.

All groups start closed, except the one with the connected country: that group opens and the cursor starts on the country. A closed group with the active tunnel in it shows `●`. While a filter is active, only groups with matches are shown, all open, and the group with the best match comes first. The cursor skips headers, so `enter` always connects a match.

## States

The `tunnel` line has a symbol, so it reads the same without color:

| Shows | Means |
|---|---|
| `· checking...` | no probe has finished yet |
| `○ down` | no wglane tunnel is up |
| `◐ connecting to XX...` / `◐ disconnecting...` | a change is running (keys that start a change are ignored) |
| `● up  XX  Name` | tunnel up; the country is also marked `●` in the list |
| `? unknown (run wglane doctor)` | the helper could not say which tunnel is up |

Errors appear in the bottom line, ahead of the key hints, starting with `error:`, so a narrow terminal cuts the hints first. Starting a new connect or disconnect clears the old message.

Color is limited to the 16 ANSI colors, so your terminal theme decides how they look. `NO_COLOR=1 wglane tui` turns color off; bold and reverse video still show state and the selected row (marked `>`).

## Tests

`app.rs` keeps the state, the reducer (`update`: state + event to state + effects) and drawing; `mod.rs` runs the terminal and the effects. The reducer is pure, so key handling is tested without a terminal. Drawing is snapshot-tested with ratatui's `TestBackend` and [insta](https://insta.rs): each state above, the help overlay, search in the tree, and a terminal that is too small.

```sh
cargo test -p wglane-cli tui            # run them
cargo insta review                      # accept or reject changed snapshots
INSTA_UPDATE=always cargo test -p wglane-cli tui   # or accept them all
```

Snapshots live in `crates/wglane-cli/src/tui/snapshots/`. Review the diff before you commit them.
