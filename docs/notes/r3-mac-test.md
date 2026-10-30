# R3 real-Mac test (owner, with sudo)

Run on the Mac that has `wglane` 0.4 installed and working. No step below is
automated: the code was only tested against a temp-dir root. Stop at the first
mismatch and do not continue; the rollback is at the end. Each step lists the
expected result.

Use the build from the PR branch (`cargo build --release`), referred to as
`./target/release/vpnlane` (and `vpnlane-helper` next to it).

## 0. Snapshot the old state

```sh
OLD=$(command -v wglane); echo "$OLD"
wglane up DE && wglane status
```
Expect: DE up, a `utunN` shown.

```sh
ls -l /usr/local/libexec/ /usr/local/libexec/wglane/bin /etc/sudoers.d/
sudo cat /etc/sudoers.d/wglane
sudo ls -ld /var/db/wglane; sudo ls -l /var/db/wglane
sudo cat /var/db/wglane/tunnels
```
Expect: `wglane-helper` (root:wheel, 0755), `wglane/bin/{wg,wireguard-go}`,
`sudoers.d/wglane` (-r--r-----); the rule reads exactly
`<you> ALL=(root) NOPASSWD: /usr/local/libexec/wglane-helper`; `/var/db/wglane`
is `drwx------ root`; `tunnels` has one `de utunN ...` line.
Save all of this output to a file.

## 1. Old tunnel up: install and uninstall must refuse

```sh
sudo ./target/release/vpnlane install-helper; echo "exit=$?"
```
Expect: `Error: an old wglane tunnel may still be up (... lists a tunnel). Run the OLD binary's `wglane down` first, then re-run `sudo vpnlane install-helper`. Nothing was changed.`, `exit=1`.

Also `sudo ./target/release/vpnlane install-helper --uninstall` must refuse the same way (exit 1, nothing removed).

Verify nothing changed:
```sh
ls -l /usr/local/libexec/ /etc/sudoers.d/; wglane status
```
Expect: no `vpnlane*` entries anywhere; DE still up.

## 2. Down with the OLD binary, then install

```sh
wglane down
sudo cat /var/db/wglane/tunnels | wc -c
```
Expect: `0` (empty file; or "No such file").

```sh
sudo ./target/release/vpnlane install-helper; echo "exit=$?"
```
Expect, all `exit=0`:
```
installed /usr/local/libexec/vpnlane-helper
installed /usr/local/libexec/vpnlane/bin
installed /etc/sudoers.d/vpnlane
removed   /etc/sudoers.d/wglane (matched the old generated rule exactly)
removed   /usr/local/libexec/wglane-helper
removed   /usr/local/libexec/wglane/bin
removed   /usr/local/libexec/wglane
removed   /var/db/wglane
```
If you see `skipped ... WARNING: differs`, `REFUSED` or `FAILED`: stop, save the
output, and see the table in `docs/UPGRADING.md`. The new install is still fine.

## 3. Verify the result

```sh
sudo visudo -c
ls -l /usr/local/libexec/ /etc/sudoers.d/
sudo cat /etc/sudoers.d/vpnlane
ls -ld /var/db/ | head -1; ls /var/db | grep -i -e wglane -e vpnlane
```
Expect: `visudo -c` says `parsed OK` for `/etc/sudoers.d/vpnlane` and the main file;
only `vpnlane-helper` and `vpnlane/` in libexec (no `wglane*`); only
`vpnlane` in sudoers.d (plus whatever else you had; no `wglane`); the rule reads
`<you> ALL=(root) NOPASSWD: /usr/local/libexec/vpnlane-helper`. `/var/db/vpnlane`
may not exist yet (created on first `up`); `/var/db/wglane` must not exist.

Idempotence:
```sh
sudo ./target/release/vpnlane install-helper
```
Expect: three `installed` lines, then `skipped ... (not present)` for the old paths; `exit=0`.

## 4. Use it without a password

```sh
sudo -k
./target/release/vpnlane doctor
```
Expect: every row `ok` (token and config dir as before), no `legacy files` row, no WARN about legacy.

```sh
./target/release/vpnlane up US
./target/release/vpnlane status
```
Expect: no password prompt; US up with a `utunN`; `sudo ls -l /var/db/vpnlane` now shows `tunnels` and `lock`.

```sh
./target/release/vpnlane down
ifconfig | grep -c '^utun'; netstat -rn -f inet | grep -c UGHS
tailscale ping <a-peer>
```
Expect: utun count back to what it was before step 0; no leftover split routes;
the peer answers. `status` shows nothing up.

## 5. Old artifacts gone

```sh
ls /usr/local/libexec/wglane* /etc/sudoers.d/wglane /var/db/wglane /var/run/wireguard/wglane.name 2>&1
```
Expect: every path `No such file or directory`.

## 6. Doctor warns about a leftover (optional negative test)

Never write into `/etc/sudoers.d` directly. Write to a temp file, validate, then move:
```sh
T=$(mktemp /tmp/wglane-rule.XXXXXX)
echo "$USER ALL=(root) NOPASSWD: /usr/local/libexec/wglane-helper" > "$T"
sudo visudo -cf "$T"            # expect: parsed OK
sudo chown root:wheel "$T"; sudo chmod 0440 "$T"
sudo mv "$T" /etc/sudoers.d/wglane
sudo visudo -c
./target/release/vpnlane doctor | grep legacy
```
Expect: `WARN  legacy files  old wglane install still present (/etc/sudoers.d/wglane): run `sudo vpnlane install-helper``. Then
```sh
sudo ./target/release/vpnlane install-helper
```
Expect `removed   /etc/sudoers.d/wglane` (the file equals the old rule, so it is ours); doctor is clean again.

### 6b. Edited old rule is kept

Same as above, but add a comment line to the temp file first (`echo '# mine' >> "$T"`), validate and move it into place, then run `sudo ./target/release/vpnlane install-helper`.
Expect `skipped   /etc/sudoers.d/wglane (WARNING: differs ...)`, exit 0, and the old `wglane-helper` still present. Then `sudo rm /etc/sudoers.d/wglane`, re-run install-helper, expect the old helper to be removed.

### 6c. Stale record after reboot

Start `wglane up DE`, reboot (do not run `down`), then `sudo ./target/release/vpnlane install-helper`.
Expect the refusal naming `/var/db/wglane/tunnels`. Run `wglane status` (or `sudo /usr/local/libexec/wglane-helper active`), which prunes the dead record; re-run install-helper and expect success.

## 7. Uninstall both, reinstall

```sh
sudo ./target/release/vpnlane install-helper --uninstall
ls /usr/local/libexec/ /etc/sudoers.d/ | grep -i -e wglane -e vpnlane
sudo ./target/release/vpnlane install-helper
```
Expect: `removed` lines for the vpnlane files; the grep prints nothing; the
reinstall prints three `installed` lines.

## Rollback (if anything above went wrong)

The old and new layouts can coexist, so a half-migrated Mac still works.
1. Take down BOTH tunnels: `vpnlane down` and `wglane down` (or `sudo /usr/local/libexec/wglane-helper down`).
2. If the old install is still intact (old `wglane-helper`, `/etc/sudoers.d/wglane` present): remove only the new files, privilege first:
   `sudo rm /etc/sudoers.d/vpnlane`, then `sudo rm /usr/local/libexec/vpnlane-helper`, then `sudo rm -r /usr/local/libexec/vpnlane`, then `sudo rm -r /var/db/vpnlane` if present. Check with `wglane doctor`.
3. If the old install is already gone: `sudo ./target/release/vpnlane install-helper --uninstall` (removes everything), then from the v0.4.0 archive `sudo ./wglane install-helper`, then `wglane doctor`.
4. If you moved config: `mv ~/.config/vpnlane ~/.config/wglane`.
