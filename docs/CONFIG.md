# Configuration

Saved settings live in `config.toml` in the config directory: the directory
that holds the token, `~/.config/wglane` unless `--config-dir` or
`WGLANE_CONFIG_DIR` says otherwise.

```
wglane config path     # where the file is (it may not exist yet)
wglane config init     # write a commented sample; never overwrites
wglane config show     # effective values, the source of each, deprecated spellings (--json too)
```

Three rules:

- **`config.toml` is yours.** wglane reads it and never writes it.
- **Runtime state is one file**, `state.json` beside it (see [State file](#state-file)).
- **One value says where**: `location`, spelled the same in the file, in a
  preset, in `WGLANE_LOCATION` and on the command line.

A missing file means the built-in defaults. An unknown key, or a wrong type or
value, is an error that names the key and the line. The Nord token is **not**
a config key: it stays in the mode-600 `token` file or `WGLANE_TOKEN`.

## Location

```
CC            DE           the least-loaded server in the country
CC/City       DE/Berlin    the least-loaded server in that city
```

`CC` is a two-letter ISO code (`GB`, not `UK`). The city is a name or Nord
identifier; case and accents do not matter (`ch/zurich` finds `Zürich`). Its
shape is checked when the value is read (1 to 64 letters, digits, space, `-`,
`_`, `'`, `.`; no `/`); that it exists in the country is checked against Nord's
list when the connection is applied, before the tunnel is touched, and the
error lists the valid cities.

`CC/City/hostname` and a bare hostname (`de1234`) are reserved for a later
release. Today they are rejected with a "not supported yet" error, so a file
written for a future release fails loudly.

One parser serves every place a location appears: the positional argument
(`wglane up DE/Berlin`), `--city` (sugar: `up DE --city Berlin` is
`up DE/Berlin`; alone it takes the country from the layers below), `WGLANE_LOCATION`,
top-level `location`, and a preset's `location`. A location always replaces
the one below it whole: naming `DE` over a preset's `DE/Berlin` gives the
whole country, and a city in the file never follows a different country.

## Keys

All keys are optional. The top level is the **unnamed base preset**.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `location` | `CC` or `CC/City` | unset (`up` with no argument errors) | where `wglane up` connects |
| `pick` | `"least-load"` or `"fastest"` | `"least-load"` | how the server is chosen. `fastest` pings the 10 least-loaded candidates and takes the lowest round trip |
| `bypass_extra` | list of CIDRs | `[]` | routes that skip the tunnel, added to the automatic Tailscale ones |
| `exit_ip_check` | bool | `true` | the leak check: look up the exit IP in `status` and the TUI. `false` is allowed but loud, see [below](#exit_ip_check-false-is-loud). Top level only |
| `tui.start_expanded` | bool | `false` | TUI opens every continent group. Top level only |
| `tui.graph_samples` | integer, 10 to 600 | `60` | TUI latency samples kept. Top level only |
| `tui.warn_ratio` | float, 1.0 to 10.0 | `1.5` | TUI draws a sample above this multiple of the window median as a spike. Top level only |
| `preset_order` | list of preset names | `[]` | number-key order of presets; each name must exist |
| `[presets.<name>]` | table, see below | none | `wglane preset ...`, `wglane up --preset` |

`bypass_extra` entries must be IPv4 `/8` or longer (IPv6 `/16` or longer), the
same floor the root helper enforces. The automatic and extra routes together
may not exceed 1024; `up` checks this before touching the tunnel and `watch`
before changing routes, and both fail with a message naming `bypass_extra`.

## Presets are overlays

A preset is a named overlay of the top level. Apply one with
`wglane preset apply NAME`, `wglane up --preset NAME` or `WGLANE_PRESET=NAME`
(all the same code path).

Any **connection key** valid at the top level (`location`, `pick`,
`bypass_extra`) is valid in a preset with the same meaning:

- scalars (`location`, `pick`) **override** the base;
- list keys (`bypass_extra`) **add** to the base list (a preset cannot remove
  a top-level route);
- `label` (1 to 24 characters, for `preset list` and the TUI) is preset-only;
- settings that are not connection state (`tui.*`, `exit_ip_check`) are top
  level only. Putting one in a preset is an error:
  `presets.work.exit_ip_check: not a connection setting, so it cannot differ per preset; set it at the top level of config.toml`.

The name is 1 to 32 characters of `a-z`, `0-9`, `_`, `-`. Order: names in
`preset_order` first, then the rest alphabetically; the first 9 get number keys
1 to 9 (in `preset list` and the TUI), later ones still work by name.

```toml
location = "ZA"                      # the base: plain `wglane up` goes to South Africa
pick = "least-load"
bypass_extra = ["198.51.100.0/24"]
preset_order = ["work", "home"]

[presets.work]
label = "Office exit"
location = "DE/Berlin"               # replaces ZA
bypass_extra = ["203.0.113.0/24", "203.0.113.9/32"]   # added: 3 routes in all

[presets.home]
location = "ZA"                      # same as the base; routes: just the base's 1
pick = "fastest"                     # replaces least-load

[presets.fast]                       # no location: the base's ZA
pick = "fastest"
```

```
$ wglane preset list
  KEY  NAME  LABEL        SUMMARY
  1    work  Office exit  DE/Berlin, +2 routes
  2    home               ZA, fastest
  3    fast               fastest

$ wglane preset apply work       # = wglane up --preset work
$ wglane up --preset work FR     # the preset, but exit in France; status: work (location overridden)
$ wglane preset show home
preset        home
key           2
location      ZA
pick          fastest
bypass_extra  198.51.100.0/24

would change:
  location  DE/Berlin -> ZA
  pick  least-load -> fastest
  bypass_extra  -2 routes
```

`preset show` previews the **resolved** request (base, preset and environment
combined, as `up --preset` would run it) against the current connection, so
`fast` above shows the move to `ZA` it implies even though it names no
location. Lines: `location  A -> B`, `pick  A -> B`, `bypass_extra  +N routes`,
`bypass_extra  -N routes`, or `already active`.

## Precedence

For each connection key the first layer that sets it wins. The preset belongs
to the file layer, below the environment.

| Key | 1. flag | 2. environment | 3. preset (if chosen) | 4. `config.toml` top level | 5. default |
|---|---|---|---|---|---|
| `location` | `up LOCATION`, `up CC --city C` | `WGLANE_LOCATION` | preset `location` | `location` | unset |
| `pick` | `up --pick RULE` (`--fastest`) | `WGLANE_PICK` | preset `pick` | `pick` | `least-load` |
| `bypass_extra` | none | none | **added** | the list | `[]` |
| which preset | `up --preset NAME` | `WGLANE_PRESET` | n/a | n/a | none |
| `exit_ip_check` | `status --exit-ip-check BOOL` | `WGLANE_EXIT_IP_CHECK` | not allowed | `exit_ip_check` | `true` |
| `tui.*` | none | none | not allowed | the key | see Keys |

`up -` skips all layers: it repeats the last request as recorded. `wglane up`
with no argument is always the layers above, never "last used".
`config show` has no flags, so it reports `env`, `file` or `default`; `flag`
appears only for values a command takes on its own command line.

### Environment variables

| Variable | Meaning |
|---|---|
| `WGLANE_CONFIG_DIR` | the config directory (same as `--config-dir`) |
| `WGLANE_TOKEN` | the Nord token (see [TOKEN.md](TOKEN.md)) |
| `WGLANE_LOCATION` | `location`, same grammar |
| `WGLANE_PICK` | `pick` |
| `WGLANE_PRESET` | the preset to apply, like `AWS_PROFILE`; `--preset` beats it |
| `WGLANE_EXIT_IP_CHECK` | `exit_ip_check`: `true`/`false`, `1`/`0`, `yes`/`no`, `on`/`off` |
| `WGLANE_COUNTRY` | **deprecated** alias of `WGLANE_LOCATION` (country only) |

## State file

What wglane last applied is kept in `<config dir>/state.json` (the same
directory as the token and the generated configs). It is machine-written, so it
is not in `config.toml`; delete it freely, you lose only `up -`, the active
preset and cached-config notes.

```json
{
  "version": 1,
  "connected": true,
  "last": {
    "location": "DE/Berlin",
    "pick": "least-load",
    "bypass_extra": ["198.51.100.0/24", "203.0.113.0/24"],
    "preset": { "name": "work", "overridden": [] },
    "at": 1767225600
  },
  "bypass_routes": 14,
  "confs": { "de": { "requested": "Berlin", "picked": "Berlin" } }
}
```

| Field | Meaning |
|---|---|
| `version` | schema version; a file with another version is left alone and reported |
| `connected` | `up` ran and `down` has not since |
| `last` | the resolved request of the last `up`: `location`, `pick`, `bypass_extra` (base plus preset: the set `watch` re-applies), `preset` (`name`, and `overridden`: `"location"` and/or `"pick"` if you changed them after choosing the preset), `at` (Unix seconds) |
| `bypass_routes` | routes the helper last installed, for `status` (0 after `down`) |
| `confs` | per tunnel: the city the cached `.conf` was requested for and the city its server is in |

- `wglane up -` reconnects to `last` exactly (place, pick, bypass set, preset),
  like `cd -`. It works after `down` too: `down` clears `connected` and the
  active preset but keeps `last`.
- The **active preset** is the preset of `last` while `connected`. It is set by
  `preset apply` and `up --preset`; `status` shows `work`, or
  `work (location overridden)` when you named another place; `down` clears it.
  A plain `up` is not a preset, even if it happens to match one.
- `watch` and TUI connects take the bypass set from `last`. A TUI connect to
  another country keeps the preset's routes and marks the preset overridden.
  (If you edit `bypass_extra` in `config.toml` while connected, the new list
  applies at the next `up`.)

Writers (`up`, `watch`, `down`) take an exclusive lock on `state.json.lock`
around each read-change-write and replace the file by rename, so overlapping
commands do not lose updates. If the state cannot be written after a connect,
wglane prints a warning on stderr and the command still succeeds.

**Downgrading.** `state.json` replaces `active-preset`, `bypass-routes` and
`<cc>.city`, which are deleted on import. An older binary does not read
`state.json`: it sees no active preset and no city notes, and its `watch`
re-applies only the top-level `bypass_extra`. Nothing breaks; it just forgets.
Deleting `state.json` is always safe (you lose `up -`, the active preset and
the city notes; the next `up` rebuilds them).

## `exit_ip_check = false` is loud

The key stays valid. Turning the leak check off is a choice, but never a silent
one:

- `status` prints `leak check     disabled (exit_ip_check = false)` every time;
- `status --json` has `"leak_check": "ok" | "disabled" | "unavailable"` (the
  last: enabled, but the lookup failed);
- `wglane doctor` shows `WARN  leak check ...` (a warning, not a failure).

## Migration from the old keys

The old spellings still work for **one release** (0.x allows it) and map into
the new ones. Each run that sees one prints a single line to stderr naming the
replacement, and `config show` lists them:

```
warning: deprecated spelling, still works for this release: default_country, default_city -> location (see docs/CONFIG.md)
```

| Old | New | Notes |
|---|---|---|
| `default_country = "US"` | `location = "US"` | |
| `default_country = "US"` + `default_city = "Chicago"` | `location = "US/Chicago"` | `default_city` alone is an error |
| `[presets.x] country = "DE"` | `location = "DE"` | |
| `[presets.x] country = "DE"` + `city = "Berlin"` | `location = "DE/Berlin"` | `city` alone is an error |
| `WGLANE_COUNTRY=DE` | `WGLANE_LOCATION=DE` | `WGLANE_LOCATION` wins if both are set |
| `up --fastest` | `up --pick fastest` | `--fastest` stays as a short form |
| `<config dir>/active-preset`, `bypass-routes`, `<cc>.city` | `state.json` | imported once on first read, then deleted |

Using an old key together with its replacement (`location` and
`default_country`) is an error. Behavior that changed with `location`:

- a positional `DE` now replaces a preset's `DE/Berlin` whole (it used to keep
  the city when the country matched);
- a city in the file no longer survives `WGLANE_LOCATION=DE` (it used to
  clash with `WGLANE_COUNTRY`);
- a plain `up` that happens to equal a preset no longer counts as that preset;
  the active preset is the one you applied.

**Timeline.** This release: both spellings work, one stderr line per run.
Next minor release: the old keys, `WGLANE_COUNTRY` and the old record files
are removed, and using them is an error pointing here.

## `pick = "fastest"` and `--pick fastest`

The candidates are the 10 least-loaded servers Nord recommends for the country
(or city). They are pinged at once with the system `ping` (one ICMP echo each,
2 s at most), and the lowest round trip wins; equal times keep the lower load.
A server that does not answer loses to any that does. If none answers, the
least-loaded one is used and the summary line says so:

```
connected to DE, config regenerated, 12 bypass routes, server de1622.example load 19% latency 41 ms
connected to DE, config regenerated, 12 bypass routes, server de1622.example load 19% (no probe answered: picked the least loaded)
```

Choosing by latency means measuring now, so `fastest` always re-picks and
regenerates the config, even when a fresh one exists. A configured `fastest`
is switched off for one run with `--pick least-load` (or `WGLANE_PICK`).

**Probing while a tunnel is up.** Packets to any server except the connected
one would travel through the tunnel and measure its exit. So while a wglane
tunnel is up, `ping` is bound to the physical interface (the interface of the
first non-tunnel default route, as the bypass gateway is found) and a probe
that cannot leave that way fails rather than leaking into the tunnel. Where
that interface has no interface-scoped default route (common while a full
tunnel is up) every probe fails and `fastest` falls back to the least loaded
server. To measure, run `wglane down` first, or use `--pick least-load`.
`wglane servers CC --probe` shows the same measurement as a PING column next
to the load. The TUI does not probe on connect and ignores `pick`.

A cached config is reused only for the same request: switching between a city
and the whole country, or to another city, regenerates it.

## Worked example

```toml
location = "ZA"
exit_ip_check = false
bypass_extra = ["198.51.100.0/24"]

[tui]
start_expanded = true
warn_ratio = 2.0
```

```
$ WGLANE_LOCATION=DE wglane config show
location               DE               env
pick                   least-load       default
bypass_extra           198.51.100.0/24  file
preset                 (unset)          default
exit_ip_check          false            file
tui.start_expanded     true             file
tui.graph_samples      60               default
tui.warn_ratio         2.0              file
presets                (none)           default
```

`wglane up` connects to `DE` (env beats file); `wglane up US` connects to `US`
(flag beats env); `wglane status --exit-ip-check true` looks the exit IP up
despite the file.
