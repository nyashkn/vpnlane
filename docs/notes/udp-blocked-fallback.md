# Spike: when outbound UDP 51820 is blocked

Status: spike, no code. Bead wgl-6xs. Checked 2026-10-30.

## Problem

NordLynx (WireGuard) uses UDP 51820 only ([Nord: open ports][ports]). On a
network that drops it, `wglane up` builds the tunnel but no handshake ever
completes. `wglane doctor` now reports `udp 51820`, but it cannot tell a
silent drop from a working path: a WireGuard server never answers an
unauthenticated initiation, so the check proves only that this machine sent
the packet. Treat `??` as "unknown", and a `FAIL` as "the local stack refused".

## The project rule

Per `docs/ARCHITECTURE.md`, the root helper runs `wireguard-go` and `wg`
(plus `ifconfig`, `route`, `netstat`) from root-owned paths and nothing else.
Every fallback below needs a different transport, so each is judged against it.

## Options

| Option | What Nord says | Fits the rule? |
|---|---|---|
| OpenVPN over TCP | OpenVPN TCP uses 443 and 1231-1234 ([ports][ports]); Nord's OpenVPN is documented for manual setup ([protocol page][protocols]). A forum post says Nord moved TCP from 443 to 80 ([snbforums][snb]); unverified. | No. Needs an `openvpn` binary run as root, a second install/trust path in `install-helper`, and OpenVPN service credentials, which `wglane` does not fetch (it only reads the NordLynx key). |
| NordWhisper | Launched January 2025; "web tunnel technology" that looks like web browsing; Nord recommends it for restrictive networks, and it is not compatible with Meshnet, DIP or Onion Over VPN ([protocols][protocols], [Nord press][press], [third-party summary][enc]). | No, and probably not possible at all. These sources describe no public wire format, no manual config and no third-party client. Whether any exists: unknown. |
| Obfuscated servers | Nord's current obfuscated-servers page tells Linux users to `nordvpn set protocol nordwhisper` ([page][obf]), so Nord now ties "obfuscated" to NordWhisper. What the older OpenVPN-based obfuscation needs: unknown (not checked). | No for the same reasons as the two above. |
| WireGuard over a TCP/UDP relay (e.g. udp2raw) | Nord documents no such relay endpoint. Without a Nord-run server end it cannot work. | Would still run wireguard-go, but needs a server we do not control: not possible. |

Also relevant: Nord states it does not support manual NordLynx connections,
only its apps ([NordLynx blog][lynx]). `wglane` already depends on an
undocumented API path; every fallback above would add more of that risk.

## Recommendation

1. Do not implement a fallback now. Keep the `doctor` check and its fix text
   (switch network or hotspot, ask for UDP 51820 to be allowed, or use the
   Nord app's NordWhisper).
2. If UDP-blocked networks turn out to be common for the owner, the only option
   with a documented manual-config path is OpenVPN TCP 443. Nothing here
   suggests the other options can be reached from outside Nord's own apps.
3. **Owner decision needed:** adding OpenVPN (or any second tunnel
   technology) changes the rule that the root helper runs only `wireguard-go`
   and `wg`. It adds a root-executed binary, a larger trusted-install surface
   and a new credential type. That is a security-model change, not a feature
   toggle. It should be decided by the owner and recorded in
   `docs/decisions` before any code is written.

## Unverified

- NordWhisper wire protocol and any third-party support: unknown.
- Whether Nord still serves OpenVPN TCP on 443 or only 80 now: unknown.
- Whether the Nord app and `wglane` can run side by side for a NordWhisper
  workaround: unknown, not tested.

[ports]: https://support.nordvpn.com/hc/en-us/articles/19684449567121-What-open-ports-does-NordVPN-offer
[protocols]: https://support.nordvpn.com/hc/en-us/articles/19482810153745-Which-NordVPN-protocol-should-I-choose
[press]: https://nordsecurity.com/press-area/nordvpn-introduces-nordwhisper-a-new-solution-for-restricted-networks
[enc]: https://encapsulated.network/what-is-nordlynx/
[obf]: https://support.nordvpn.com/hc/en-us/articles/19615332252561-How-to-connect-and-disconnect-from-NordVPN-s-obfuscated-servers
[lynx]: https://nordvpn.com/blog/nordlynx-protocol-wireguard/
[snb]: https://www.snbforums.com/threads/nordvpn-openvpn-udp-tcp-connection-port-changes-heads-up.98022/
