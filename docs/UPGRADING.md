# Upgrading from wglane (0.4.x) to vpnlane (0.5.0)

The project was renamed from `wglane` to `vpnlane`. This is a hard cut: there is
no compatibility layer. `vpnlane` does not read `~/.config/wglane` or any
`WGLANE_*` variable, and there is no `wglane` shim binary.

## Steps

1. Bring the old tunnel down first. With the OLD binary: `wglane down`. If you no longer have it:
   `sudo /usr/local/libexec/wglane-helper down` (the old sudoers rule is still in place at this point).
2. Install vpnlane (release archive or `cargo install --path crates/vpnlane-cli --locked`).
3. Run `sudo vpnlane install-helper` (step 6 below describes what it does). Only after it
   succeeds, remove the old binaries: `cargo uninstall wglane-cli` (or `rm ~/.cargo/bin/wglane ~/.cargo/bin/wglane-helper`).
   Do not delete the old `wglane` binary earlier: it is the easiest way to bring a tunnel down.
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

6. If you have not yet: `sudo vpnlane install-helper` (see below).
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

## What `sudo vpnlane install-helper` does to the root side

The root-owned pieces moved to the new name:

| Piece | Old | New |
|---|---|---|
| helper | `/usr/local/libexec/wglane-helper` | `/usr/local/libexec/vpnlane-helper` |
| tools (`wg`, `wireguard-go`) | `/usr/local/libexec/wglane/bin` | `/usr/local/libexec/vpnlane/bin` |
| sudoers rule | `/etc/sudoers.d/wglane` | `/etc/sudoers.d/vpnlane` |
| state | `/var/db/wglane` | `/var/db/vpnlane` |

`install-helper` migrates in this order, and prints one line per step:

1. **Refuses while an old tunnel may be up.** If `/var/db/wglane/tunnels` lists a
   tunnel or `/var/run/wireguard/wglane.name` exists, it exits non-zero and
   changes nothing:
   `an old wglane tunnel may still be up (...). Run the OLD binary's `wglane down` first, then re-run `sudo vpnlane install-helper`. Nothing was changed.`
   Run the old `wglane down` or `sudo /usr/local/libexec/wglane-helper down` (step 1
   above), then retry. The new helper cannot tear down a tunnel the old one started,
   so this is a hard stop. The same check guards `--uninstall` and runs again under
   the old helper's lock before anything old is deleted.
   A tunnel record can be left behind by a reboot (no tunnel is actually up). Running
   `wglane status` or `sudo /usr/local/libexec/wglane-helper active` prunes a dead
   record; then retry. Leftover `bypass.json` or `<cc>.conf` files in `/var/db/wglane`
   also count as "down has not run": `down` removes them. The `.conf` files hold
   private keys and are root-only; `install-helper` never deletes them, and you may
   remove them by hand after `down`.
2. **Installs the new helper, tools and sudoers rule** (`installed ...` lines).
   The rule is checked with `visudo -cf` before it is activated, and the result is
   re-read and verified. If anything here fails, the old install is untouched and
   still works.
3. **Only then removes the old files**, each only if it is positively identified.
   Nothing is followed through a symlink and nothing is deleted recursively.

What each line means:

| Line | Meaning |
|---|---|
| `installed <path>` | New artifact written and verified. |
| `removed <path>` | Old artifact identified and deleted. |
| `skipped <path> (not present)` | Nothing there; normal on a fresh machine or a re-run. |
| `skipped <rule> (WARNING: differs ...)` | `/etc/sudoers.d/wglane` is not byte-for-byte what the old version generated for you (you edited it, or it is for another user). It is left alone, and so are the old helper, tools and state directory it relates to. Delete the file yourself if you do not need it (`sudo rm /etc/sudoers.d/wglane`), then re-run `sudo vpnlane install-helper` to finish the cleanup. |
| `skipped <dir> (contains X, left in place)` | The old directory holds a file we did not create; it is kept. |
| `skipped <state dir> (an old helper is running)` | Re-run in a moment. |
| `REFUSED <path> (...)` | The old file is a symlink, not root-owned, or group/world-writable. It is left alone and the command exits non-zero. Inspect it by hand; the new install is already in place and working. |
| `FAILED <path> (...)` | An OS error while deleting. The new install is in place. |

Re-running is safe: the second run reinstalls the new files (this is also how
you refresh the tools after upgrading `wireguard-tools`) and reports
`skipped ... (not present)` for everything old.

`vpnlane doctor` shows a `WARN legacy files` row while the old rule or helper
exists; `sudo vpnlane install-helper` clears it. `sudo vpnlane install-helper --uninstall`
removes the new install and, by the same rules, any old one.

If post-install verification ever fails, the new files are in place next to the old
ones (two sudoers rules, each naming its own helper). That works and is safe; fix
the cause and re-run.

If you keep running the OLD `wglane` after this, it will report that its helper is
missing; do not re-run the old `install-helper`, it recreates the old layout.

### Rollback

Both layouts can coexist, so which way back depends on whether the old install is still intact.

- **Old install still there** (the migration stopped early, or the old rule was kept): remove only the NEW files, privilege first:
  1. `vpnlane down` and `wglane down` (take down both tunnels).
  2. `sudo rm /etc/sudoers.d/vpnlane`, then `sudo rm /usr/local/libexec/vpnlane-helper`, then `sudo rm -r /usr/local/libexec/vpnlane`, then `sudo rm -r /var/db/vpnlane` (only if it exists).
  3. `mv ~/.config/vpnlane ~/.config/wglane` if you moved it.
- **Old install already removed**: restore it from the v0.4.0 archive.
  1. `vpnlane down`.
  2. `sudo vpnlane install-helper --uninstall` (this removes everything, new and old; skip it if you only want the old one back).
  3. `mv ~/.config/vpnlane ~/.config/wglane`.
  4. From the v0.4.0 archive, `sudo ./wglane install-helper`.
