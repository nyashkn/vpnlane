# Fixtures

Synthetic Nord API responses for tests. Every key is `base64("SYNTHETIC-...")`,
every IP is from the documentation ranges (192.0.2.0/24, 198.51.100.0/24,
2001:db8::/32), every hostname ends in `.example.invalid`. Never paste a real
response here without replacing keys, IPs and account fields.

| File | Endpoint |
|---|---|
| `countries.json` | `GET /v1/servers/countries` |
| `recommendations_za.json` | `GET /v1/servers/recommendations?filters[country_id]=2&...` (server 3 has no `wireguard_udp`: must be skipped) |
| `credentials.json` | `GET /v1/users/services/credentials` |
| `insights.json` | `GET /v1/helpers/ips/insights` |
