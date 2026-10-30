# Configuration

Saved settings live in `config.toml` in the config directory: the directory
that holds the token, `~/.config/wglane` unless `--config-dir` or
`WGLANE_CONFIG_DIR` says otherwise.

```
wglane config path     # where the file is (it may not exist yet)
wglane config init     # write a commented sample; never overwrites
wglane config show     # effective values and the source of each (--json too)
```

A missing file means the built-in defaults. An unknown key, or a wrong type or
value, is an error that names the key and the line. The Nord token is **not** a config key: it stays in the mode-600
`token` file or `WGLANE_TOKEN`.

## Keys

All keys are optional.

| Key | Type | Default | Used by |
|---|---|---|---|
| `default_country` | two-letter code, e.g. `"US"` | unset | `wglane up` with no argument |
| `default_city` | string, 1 to 64 letters, digits, space, `-`, `'`, `.` | unset | `wglane up` with no `--city`, when the country also came from the default (no argument): the city must exist in that country, else `up` stops and lists the valid ones |
| `pick` | `"least-load"` or `"fastest"` | `"least-load"` | reserved: validated, not used yet (servers are always picked by least load) |
| `bypass_extra` | list of CIDRs | `[]` | `up` and `watch`: routes that skip the tunnel, added to the automatic Tailscale ones |
| `exit_ip_check` | bool | `true` | `status` and the TUI; `false` skips the exit IP lookup (so `protected` and the exit-country warning read as unknown) |
| `tui.start_expanded` | bool | `false` | TUI opens every continent group |
| `tui.graph_samples` | integer, 10 to 600 | `60` | TUI latency samples kept |
| `tui.warn_ratio` | float, 1.0 to 10.0 | `1.5` | TUI draws a sample above this multiple of the window median as a spike |

| `preset_order` | list of preset names | `[]` | number-key order of presets; each name must exist |
| `[presets.<name>]` | table, see below | none | `wglane preset ...`, `wglane up --preset` |

`bypass_extra` entries must be IPv4 `/8` or longer (IPv6 `/16` or longer), the
same floor the root helper enforces. The automatic and extra routes together
may not exceed 1024; `up` checks this before touching the tunnel and `watch`
before changing routes, and both fail with a message naming `bypass_extra`.

## Presets

A preset is a named bundle applied with one command: `wglane preset apply NAME`
or `wglane up --preset NAME` (the same code path as `up`). Every key is
optional; a key a preset leaves out comes from the top-level settings.

| Key in `[presets.<name>]` | Type | Meaning |
|---|---|---|
| `country` | two-letter code | country to connect to; default: `default_country` |
| `city` | string, same rule as `default_city` | city in `country`; **requires `country`** |
| `pick` | `"least-load"` or `"fastest"` | reserved, like the top-level `pick` |
| `bypass_extra` | list of CIDRs, same rules as the top-level key | **added** to the top-level `bypass_extra` |
| `label` | 1 to 24 characters | short display text for `preset list` and the TUI |

- The name is 1 to 32 characters of `a-z`, `0-9`, `_`, `-`. An unknown key in a
  preset, a bad value, or a name outside that set is an error naming the key
  and line, like everywhere else in the file.
- A preset that names a `country` owns the city: no `city` means the whole
  country, and `default_city` is not inherited. A preset with no `country`
  inherits `default_country` and `default_city`.
- The shape of a city is checked when the file is read; that it exists in the
  country is checked at apply time (it needs Nord's list). A bad one stops
  `apply` before the tunnel is touched and lists the valid cities.
- Order: names in `preset_order` first, then the rest alphabetically. The
  first 9 get number keys 1 to 9 (in `preset list` and the TUI); further
  presets still work by name. `preset_order` entries must exist and be unique.
- The active preset is the last one applied. It is stored in
  `<config dir>/active-preset` (user-level state next to the `<cc>.city` notes)
  and cleared by `down`, or by an `up` that no longer matches it (a different
  country, city or set of bypass routes). `wglane status` shows it, and
  `status --json` has a `preset` key while one is active.
- `wglane preset show NAME` prints the effective request and a preview of what
  applying it would change against the current connection, e.g.
  `country  US -> DE`, `city  - -> Berlin`, `bypass_extra  +2 routes`, or
  `already active`.

## Precedence

For each value, the first layer that sets it wins.

| Setting | 1. CLI flag | 2. Environment | 3. `config.toml` | 4. Default |
|---|---|---|---|---|
| `default_country` | `wglane up CC` | `WGLANE_COUNTRY` | `default_country` | unset (`up` errors) |
| country and city of `up --preset NAME` | `wglane up CC`, `--city` | none: the preset beats `WGLANE_COUNTRY` | the preset, then `default_country` | unset (`up` errors) |
| `exit_ip_check` | `status --exit-ip-check BOOL` | `WGLANE_EXIT_IP_CHECK` | `exit_ip_check` | `true` |
| everything else | none | none | the key | the default above |

`config show` has no flags, so it reports `env`, `file` or `default`; `flag`
appears only for values a command takes on its own command line.

## How selection rules resolve

`wglane up` picks the country as: the argument if given, else
`WGLANE_COUNTRY`, else `default_country`, else it stops and says so. Within the
country the server is the least loaded one. `--city` picks the least loaded
server in that city; with no `--city` and no country argument, `default_city`
does the same. A country given on the command line ignores `default_city`
(so `wglane up DE` never trips over a US city). A cached config is reused only
for the same request: switching between a city and the whole country, or to
another city, regenerates it. `pick` is accepted so a file written today keeps
working when selection grows; it has no effect yet. The TUI does not apply
`default_city`.

## Worked example

```toml
default_country = "ZA"
exit_ip_check = false
bypass_extra = ["198.51.100.0/24"]

[tui]
start_expanded = true
warn_ratio = 2.0
```

```
$ WGLANE_COUNTRY=DE wglane config show
default_country        DE               env
default_city           (unset)          default
pick (reserved)        least-load       default
bypass_extra           198.51.100.0/24  file
exit_ip_check          false            file
tui.start_expanded     true             file
tui.graph_samples      60               default
tui.warn_ratio         2.0              file
```

`wglane up` connects to `DE` (env beats file); `wglane up US` connects to `US`
(flag beats env); `wglane status --exit-ip-check true` looks the exit IP up
despite the file.

## Worked example: presets

```toml
default_country = "ZA"
bypass_extra = ["198.51.100.0/24"]
preset_order = ["work", "home"]

[presets.work]
label = "Office exit"
country = "DE"
city = "Berlin"
bypass_extra = ["203.0.113.0/24", "203.0.113.9/32"]

[presets.home]
country = "ZA"

[presets.fast]
pick = "fastest"
```

```
$ wglane preset list
  KEY  NAME  LABEL        SUMMARY
  1    work  Office exit  DE, Berlin, +2 routes
  2    home               ZA
  3    fast               fastest

$ wglane preset apply work       # = wglane up --preset work
$ wglane up --preset work FR     # the preset, but exit in France (its city Berlin is dropped)
$ wglane preset show home
preset        home
key           2
country       ZA
city          -
pick          least-load
bypass_extra  198.51.100.0/24

would change:
  country  DE -> ZA
  city  Berlin -> -
```

`work` also routes `203.0.113.0/24` and `203.0.113.9/32` outside the tunnel on
top of `198.51.100.0/24`. `fast` has no `country`, so it connects to
`default_country` (ZA).
