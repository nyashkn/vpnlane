# Upgrading from wglane (0.4.x) to vpnlane (0.5.0)

The project was renamed from `wglane` to `vpnlane`. This is a hard cut: there is
no compatibility layer. `vpnlane` does not read `~/.config/wglane` or any
`WGLANE_*` variable, and there is no `wglane` shim binary.

## Steps

1. With the OLD binary, bring the tunnel down first: `wglane down`.
2. Install vpnlane (release archive or `cargo install --path crates/vpnlane-cli --locked`).
3. Remove the old binaries: `cargo uninstall wglane-cli` (or `rm ~/.cargo/bin/wglane ~/.cargo/bin/wglane-helper`).
4. Move your config, token and presets:

   ```sh
   mv ~/.config/wglane ~/.config/vpnlane
   ```

   If you set `WGLANE_CONFIG_DIR`, move that directory wherever you like and set `VPNLANE_CONFIG_DIR` instead.
5. Rename environment variables in your shell profile, scripts and CI:

   | Old | New |
   |---|---|
   | `WGLANE_CONFIG_DIR` | `VPNLANE_CONFIG_DIR` |
   | `WGLANE_TOKEN` | `VPNLANE_TOKEN` |
   | `WGLANE_LOCATION` | `VPNLANE_LOCATION` |
   | `WGLANE_PICK` | `VPNLANE_PICK` |
   | `WGLANE_PRESET` | `VPNLANE_PRESET` |
   | `WGLANE_EXIT_IP_CHECK` | `VPNLANE_EXIT_IP_CHECK` |
   | `WGLANE_COUNTRY` (deprecated) | `VPNLANE_LOCATION` |

6. Re-run the helper install: `sudo vpnlane install-helper`.
7. Regenerate shell completions (`vpnlane completions zsh`, etc.); old `_wglane` files are inert and can be deleted.

## Optional `vl` alias

`vl` is a shell alias only; no second binary ships.

```sh
# ~/.zshrc
alias vl=vpnlane
compdef vl=vpnlane
# ~/.bashrc
alias vl=vpnlane
complete -F _vpnlane vl   # after sourcing `vpnlane completions bash`
```

## R3 pending: root-side names (not migrated yet)

In 0.5.0 the root-owned pieces still use the old names, so nothing on the
privileged side moves yet. The `vpnlane-helper` binary is copied by
`vpnlane install-helper` to the existing location:

- `/usr/local/libexec/wglane-helper` and `/usr/local/libexec/wglane/bin`
- `/etc/sudoers.d/wglane`
- `/var/db/wglane`

A later change renames these and migrates them (helper, sudoers rule, state
directory, with `install-helper` cleaning up the old files). Until then, do not
delete them by hand. Their migration steps will be added here.
