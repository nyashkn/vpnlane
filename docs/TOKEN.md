# Getting a NordVPN access token

vpnlane uses your NordVPN access token to ask Nord's API for your NordLynx
(WireGuard) private key and the server list. It sends the token only to
`api.nordvpn.com` (as the Basic-auth password when fetching your service
credentials) and never writes it anywhere except the file you choose below.

## Generate it

In a browser. Nord's wording changes; these are the steps in
[Nord's support article][support] at the time of writing:

1. Log in to [Nord Account](https://my.nordaccount.com/).
2. Click **NordVPN**.
3. Scroll to **Advanced settings** and click **Get access token**.
4. Nord emails you a verification code; enter it.
5. Click **Generate new token**.
6. Choose an expiry: a temporary token that expires after 30 days, or one that
   does not expire. Nord recommends turning on multi-factor authentication if
   you pick the non-expiring one.
7. Click **Generate token** and copy it. Nord shows it only once.

If you lose it, click **Revoke** on the same page, then generate a new one.

## Save it

```sh
mkdir -p ~/.config/vpnlane && (umask 077; pbpaste > ~/.config/vpnlane/token)
```

The file must be mode 600; vpnlane refuses a group- or world-readable token file.
Alternatively `export VPNLANE_TOKEN=...`; the environment variable wins over the
file.

Never pass the token as a command-line argument (it would show up in `ps` and
shell history), and do not paste it into issues, chats or logs.

## When it expires or is revoked

`vpnlane up` fails with `token rejected`. Generate a new token and overwrite
`~/.config/vpnlane/token` with the command above. Revoking a token in Nord
Account has the same effect immediately.

## Source

[Nord's support article][support] describes the same Nord Account access
token for its Linux app. The steps were not checked against the live account pages, so if
Nord's labels differ from the list above, follow Nord's.

[support]: https://support.nordvpn.com/hc/en-us/articles/20286980309265-How-to-log-in-to-NordVPN-without-a-GUI-using-a-token
