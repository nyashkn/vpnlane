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
value, is an error that names the key and, for unknown keys and type errors,
the line. The Nord token is **not** a config key: it stays in the mode-600
`token` file or `WGLANE_TOKEN`.

## Keys

All keys are optional.

| Key | Type | Default | Used by |
|---|---|---|---|
| `default_country` | two-letter code, e.g. `"US"` | unset | `wglane up` with no argument |
| `default_city` | string, 1 to 64 letters, digits, space, `-`, `'`, `.` | unset | reserved: validated, not used yet |
| `pick` | `"least-load"` or `"fastest"` | `"least-load"` | reserved: validated, not used yet (servers are always picked by least load) |
| `bypass_extra` | list of CIDRs | `[]` | `up` and `watch`: routes that skip the tunnel, added to the automatic Tailscale ones |
| `exit_ip_check` | bool | `true` | `status` and the TUI; `false` skips the exit IP lookup |
| `tui.start_expanded` | bool | `false` | TUI opens every continent group |
| `tui.graph_samples` | integer, 10 to 600 | `60` | TUI latency samples kept |
| `tui.warn_ratio` | float, 1.0 to 10.0 | `1.5` | TUI draws a sample above this multiple of the window median as a spike |

`bypass_extra` entries must be IPv4 `/8` or longer (IPv6 `/16` or longer), the
same floor the root helper enforces. The automatic and extra routes together
may not exceed 1024; `up` and `watch` fail with a message naming
`bypass_extra` if they do.

## Precedence

For each value, the first layer that sets it wins.

| Setting | 1. CLI flag | 2. Environment | 3. `config.toml` | 4. Default |
|---|---|---|---|---|
| `default_country` | `wglane up CC` | `WGLANE_COUNTRY` | `default_country` | unset (`up` errors) |
| `exit_ip_check` | `status --exit-ip-check BOOL` | `WGLANE_EXIT_IP_CHECK` | `exit_ip_check` | `true` |
| everything else | none | none | the key | the default above |

`config show` has no flags, so it reports `env`, `file` or `default`; `flag`
appears only for values a command takes on its own command line.

## How selection rules resolve

`wglane up` picks the country as: the argument if given, else
`WGLANE_COUNTRY`, else `default_country`, else it stops and says so. Within the
country the server is the least loaded one. `pick` and `default_city` are
accepted so a file written today keeps working when selection grows; they have
no effect yet.

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
default_city (reserved) (unset)          default
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
