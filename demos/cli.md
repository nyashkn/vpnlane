# Demo: wglane CLI

**Terminal:** 100x28

## Steps

1. **Help**
   `run wglane --help | head -16`
   _Prove:_ commands are listed — `screen ~ "doctor"`

2. **Check prerequisites**
   `run wglane doctor`

3. **Countries**
   `run wglane servers | head -6`
   _Prove:_ list shown — `screen ~ "Afghanistan"`

4. **Least-loaded servers in South Africa**
   `run wglane servers ZA --limit 5 | awk '{print $1, $2}'`
   _Prove:_ servers shown — `screen ~ "nordvpn.com"`
