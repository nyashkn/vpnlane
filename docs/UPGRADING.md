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
   Run the old `wglane down` (step 1 above), then retry. (The new helper cannot
   tear down a tunnel the old one started, so this is a hard stop, not a warning.)
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
| `skipped <rule> (WARNING: differs ...)` | `/etc/sudoers.d/wglane` is not byte-for-byte what the old version generated for you (you edited it, or it is for another user). It is left alone, and so are the old helper and tools it points at. Delete the file yourself if you do not need it (`sudo rm /etc/sudoers.d/wglane`), then re-run `sudo vpnlane install-helper` to finish the cleanup. |
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

If you keep running the OLD `wglane` after this, it will report that its helper is
missing; do not re-run the old `install-helper`, it recreates the old layout.

### Rollback

1. `vpnlane down`.
2. `sudo vpnlane install-helper --uninstall`.
3. `mv ~/.config/vpnlane ~/.config/wglane`.
4. From the old release archive, `sudo ./wglane install-helper`.
