# Dashboard (`wglane tui`)

`wglane tui` is palette-first. Home is a status HUD: tunnel state, safety, exit,
tailnet, bypass, a latency graph, your presets and the fastest places right now.
To go somewhere, press `space` (or `:`) and type a city, a country, a preset or
an action. The continent tree is still there, one key away (`b`), for when you
do not know the name. Build it with the default `tui` feature
(`--no-default-features` drops it).

## Keys

Press `?` for this list in the dashboard (any key closes it). The palette, the
help overlay and the home keys read one action table (`tui/actions.rs`), so
they cannot drift apart.

**Home**

| Key | Does |
|---|---|
| `space`, `:` | open the jump palette |
| `1`-`9` | apply that preset now: reconnects at once, no preview |
| `0` | repeat the last request (`wglane up -`) |
| `p` | preview presets: the palette in preset mode, showing what each would change |
| `f` | connect to the fastest place measured right now |
| `b` | browse the continent tree |
| `/` | browse with the tree filter open |
| `d` | disconnect |
| `r` | recheck: status, leak check, tailnet, and a new latency round |
| `enter` | after an error: retry the change that failed |
| `?` | help |
| `q`, `esc` | quit |

**Palette** (letters type; nothing quits)

| Key | Does |
|---|---|
| type | filter places, presets and actions |
| `@` / `>` first | only presets / only actions |
| `tab` | cycle the kind: all, places, presets, actions |
| up/down, `ctrl-j`/`ctrl-k` | move |
| home/end | first / last row |
| `enter` | go: connect the place, apply the preset (reconnects), run the action |
| `esc` | close |

**Browse** (`b`)

| Key | Does |
|---|---|
| `enter` | on a country: connect its least-loaded server; on a city: that city; on a group: open/close |
| `space`, `h` / `l`, left/right | open/close a group or a country's cities, or step in/out a level |
| `j` `k`, up/down, `g` `G`, home/end | move, first / last row |
| `/` | filter by code, country or city (`enter` keeps it, `esc` clears it) |
| `:` | jump palette |
| `r` | recheck (and reload the server list after a failed load) |
| `d` | disconnect |
| `esc` | clear the filter, then back home |
| `q` | quit |

Mouse input is not used.

## Home

```
  ● up      Germany · Frankfurt                                        preset work
            DE/Frankfurt · least-load · up 1h 12m
  presets  0 last DE/Frankfurt   1 work ● active   2 stream   3 travel     p preview

  exit      203.0.113.7 · DE · protected        tailnet   ok · nas 41 ms
  bypass    14 routes                           checks    leak ok · udp 51820 sent

  latency   38 ms  min 35  max 61   to tailnet peer, last 92
  ▆▂▆▂▇▃█▄█▄█▅█▆▂▆▂▇▃█▄█▄█▅█▆▂▆█▇▃█▄█▄█▅█▆▂

  fastest right now  of 5 measured
  NL  Netherlands       27 ms ███▍   18% █▌··  fastest
  GB  United Kingdom    31 ms ███▉   45% ███▌
  ┌──────────────────────────────────────────────────────────────────────┐
  │ › jump to a city, country, preset or action             space  or  : │
  └──────────────────────────────────────────────────────────────────────┘
space jump  0-9 preset  p preview  f fastest  b browse  d disconnect  ? keys  q quit
```

- **State line.** A symbol and a word, so it reads the same without colour.
  The second line is the request behind the tunnel (`up` recorded it in
  `state.json`) and how long ago it was applied.
- **Presets strip.** Slot `0` is the last request, `1`-`9` the presets of
  `config.toml` in key order. The active preset (the one the live connection
  came from) is bold and marked `● active`. A number key reconnects at once;
  `p` shows the preview first. The preview of a preset is core
  `describe_change` on the resolved overlay (environment included), so it
  names the country a preset without one would move you to, and the routes
  it would remove.
- **Safety.** A wrong exit country (`exit IP .. is in NL, not DE`) or Nord's
  `protected = false` turns into a three-line reverse-red `LEAK RISK` block
  with `d` and `r`, and the palette pins "disconnect" as its first row (typing
  moves the cursor to the best match, so `enter` after a search never
  disconnects by accident). With `exit_ip_check = false` a reverse-yellow
  `leak check disabled` line is always shown, since nothing can tell.
- **Fastest right now.** The best places measured so far: at start, the
  countries in the connected (or last) country's continent plus every preset's
  and the last request's country; then whatever the palette and the tree
  measured. See [Latency and load](#latency-and-load).
- **Graph.** The tailnet peer round trip, scaled to the visible window
  (min..max), so a steady link draws a low flat line and the shape of a wobble
  shows; it is not an absolute scale. Samples above `tui.warn_ratio` times the
  median are yellow. The browse screen keeps the one-line version of it.

## States

| Shows | Means |
|---|---|
| `· checking` | no status yet |
| `○ down  not connected` | no wglane tunnel; the second line names the last request |
| `◐ connecting  Germany · Frankfurt` | a change is running; keys that change the tunnel wait, `space`, `b`, `?` still work |
| `◐ disconnecting` | same |
| `● up  Germany · Frankfurt` | tunnel up; `preset work` on the right when it came from a preset |
| `? unknown  the helper did not answer` | the helper could not say which tunnel is up; `r` retries, `wglane doctor` in a shell says why |

**Errors.** A failed change shows three rows in the HUD: `error` (the
message), `doctor` (the UDP 51820 check, run at once after a failure: `looks
blocked on this network` in reverse yellow, or `leaves this machine; delivery
cannot be tested`), and `next` (switch network when UDP is blocked; `enter`
retries). The footer starts with the error, so a narrow terminal cuts the
hints first.

**Sizes.** 80 columns and up: two-column info rows. 60 to 79: one column.
Below 60 columns or 14 rows: the tiny view (state, safety or exit, preset and
tailnet, the jump hint). Below 30x6: `terminal too small`. Short terminals
drop spacing first, then the graph, then the fastest list.

**Colour.** 16 ANSI colours only, used for meaning: green ok, yellow busy or
warning (load of 60% and up too), red fault or leak. `NO_COLOR=1 wglane tui`
turns colour off; symbols, bold and reverse video still carry every state.

**Logs.** While the dashboard holds the screen, log lines (`-v`, `RUST_LOG`)
and warnings go to `<config dir>/tui.log` (mode 0600), never to the terminal.

## Latency and load

Every place row (palette, home, browse) shows a relative LATENCY bar and a
LOAD bar, each with its number, scaled to the rows on screen. Bars are a fixed
width of `█` (filled) and `░` (empty) cells, so they read the same under
`NO_COLOR`. Latency is in ms and `-` means not measured; load is the percent of
server capacity as Nord reports it (60% and up in yellow). Browse labels the
columns with a `LATENCY` / `LOAD` header row (`LAT` under 50 columns; under 40
it is dropped and the footer carries a one-line legend), and the `?` overlay
has a "columns" section with the same legend; the lowest
latency gets a `fastest` tag. A place's numbers come from one server: the one
Nord recommends there (`recommended(cc, city, 1)`), its load as Nord reports
it, its latency pinged now. Nothing is estimated: a place not measured shows
`-`, one being measured `probing`.

The palette ranks by match first (exact, prefix, word prefix, substring,
letters in order from a word start), then, within the same match quality, by
`probe::rank_with_probes`: measured latency, then failed, then not probed,
load breaking ties. So the list re-sorts as results land; the cursor stays on
the same row (`probe::cursor_after`).

**While a tunnel is up**, pings are bound to the physical interface so they
never measure the tunnel's own exit, and they often all fail. Then home says
`latency unavailable while connected` (browse shows it in its footer too),
the lists sort by load, and `f` refuses. Disconnect to measure. (A follow-up adds temporary helper host routes
for the probes; it replaces only `tui::probe` in `mod.rs`.)

Probe rounds are numbered (`probe_epoch`). A finished connect or disconnect
starts a new round over every sampled server, because the path changed; a
result tagged with an older round is dropped, and a server lookup that lands
late is probed again in the current round.

## Architecture

| File | Holds |
|---|---|
| `tui/mod.rs` | terminal, event loop, the two workers, effect execution, the prober |
| `tui/app.rs` | `State`, `Event`, `Effect`, the pure `update`, the browse tree |
| `tui/hud.rs` | home drawing, safety verdict, footer, tiny view |
| `tui/palette.rs` | palette state, matcher, items, keys, drawing |
| `tui/presets.rs` | preset slots, preview, apply, the strip |
| `tui/places.rs` | place samples, ranking, bars |
| `tui/actions.rs` | action registry (palette, help and home keys) |
| `tui/probe.rs` | probe results, `rank_with_probes`, `cursor_after` |
| `tui/worker.rs` | the effect thread |

The reducer is pure: `update(state, event) -> (state, effects)`. The loop
never does I/O. Effects run on two worker threads: tunnel changes and status
on one (in order, so a connect is never interleaved with a refresh), server
lookups and probes on the other (so a slow probe round never delays a
connect). Each result comes back as an `Event`.

- `Effect::Connect(cc, city)`: a place, through `up::plan_from_last` (the last
  request's bypass set and preset stay; the preset is marked overridden).
- `Effect::Apply(Some(name))` / `Apply(None)`: `up --preset NAME` / `up -`,
  the same code path as the CLI (`up::apply_real`), so a preset's `pick` holds.
- `Effect::Sample { epoch, places }`: Nord lookups, 6 at a time, then a probe
  of the servers found. `Effect::Probe` re-probes known servers.
- `Effect::CheckUdp`: the doctor's UDP 51820 check (at start and after a
  failed change, or from the palette).
- `Effect::Refresh`: `status::gather` plus `state.json` (last request, active
  preset).

**Quitting during a connect or disconnect.** `q` always quits. The terminal is
restored at once, then wglane prints `finishing the connect before exit...`
and waits for that one change (a tunnel is never abandoned half-built). Lookups
and probes in flight are dropped.

## Tests

The reducer is pure, so every key and mode is tested without a terminal:
home, palette (search, kinds, no match, pinned disconnect), presets (number
keys, `0`, preview, apply), browse, probe re-sort with a steady cursor, stale
probe results dropped, the UDP hint and retry, and a deterministic random key
walk across all modes (selection always valid, no panic, every size draws).
Drawing is snapshot-tested with ratatui's `TestBackend` and
[insta](https://insta.rs) for each state above: home connected, connecting,
down, palette with city results and bars, palette while probing, empty
palette, no match, preset preview, leak warning, leak check disabled, error
with the UDP hint, unknown tunnel, help, 60x20 home and palette, 34x7, too
small, and the browse tree. A test also renders every state under `NO_COLOR`
and checks no cell carries a colour.

```sh
cargo test -p wglane-cli tui            # run them
cargo insta review                      # accept or reject changed snapshots
INSTA_UPDATE=always cargo test -p wglane-cli tui   # or accept them all
```

Snapshots live in `crates/wglane-cli/src/tui/snapshots/`. All data in them is
made up (203.0.113.x, 198.51.100.x, invented hosts). Review the diff before you
commit them.

To look at it by hand without touching your tunnel, run it the way
`demos/record.sh` does: a temp `HOME` and `WGLANE_CONFIG_DIR`, under
`sandbox-exec` hiding `/private/var/run/wireguard` (the tunnel then reads
`? unknown`), in a `tmux` window you resize to 100x30, 60x20 and 34x7.
