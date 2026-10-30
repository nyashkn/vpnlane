# Dashboard (`wglane tui`)

`wglane tui` shows a one-line header on top, the server list on the left and tunnel status on the right. The status refreshes every 2 s. It needs a terminal of at least 40x8; smaller terminals get a notice until you resize. Build it with the default `tui` feature (`--no-default-features` drops it).

## Keys

Press `?` in the dashboard to see this list.

| Key | Does |
|---|---|
| `enter` | on a city: connect to that city; on a country: connect to its least-loaded server; on a group: open/close it |
| `space` | open/close a group or a country's cities (nothing on a city or a one-city country) |
| `h` / `l` | `l` opens a group or country; `h` closes an open group or country; on a closed country `h` closes its group, on a city it closes the country |
| left / right | left closes an open group or country, otherwise jumps up one level (city to country, country to group header); right opens a closed group or country, or steps into an open one |
| `j` `k`, up/down | move |
| `g` `G`, home/end | first / last row |
| `/` | search by code, country name or city name; `enter` keeps the filter, `esc` clears it |
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

The tree has three levels: continent, country, city. A country with more than one city shows `▸` (closed) or `▾` (open) after its name and lists its cities, with server counts, below it. A country with one city (or none listed) has no city level: its row connects directly and shows no chevron. `enter` on a country always connects its least-loaded server, open or not; `enter` on a city connects the least-loaded server in that city (and regenerates the config unless the cached one was made for that same city).

All groups and countries start closed, except the connected one: its group opens, and the cursor starts on the connected city when `up` recorded it, else on the country. A closed group with the active tunnel in it shows `●`.

While a filter is active, the query matches the country code, the country name and city names (case-insensitive substring, accents ignored; code-prefix hits come first). Only groups with matches are shown, all open, and the group with the best match comes first. A country with matching cities opens and lists only those cities. A country shown only because of its cities is not selectable, nor is a group header, so `enter` always connects something that matched; the toggle keys do nothing while a filter is active.

The connected city comes from a `<cc>.city` note next to the config, written when `up` generates the config. It is missing for configs made by `gen` or by an older wglane, and then only the country is shown.

## States

The `tunnel` line has a symbol, so it reads the same without color:

| Shows | Means |
|---|---|
| `· checking...` | no probe has finished yet |
| `○ down` | no wglane tunnel is up |
| `◐ connecting to XX...` / `◐ disconnecting...` | a change is running (keys that start a change are ignored) |
| `● up  XX  Name, City` | tunnel up; the country (and its city, when known) is also marked `●` in the list |
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
