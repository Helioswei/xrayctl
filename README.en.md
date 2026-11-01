# xrayctl

One VPS, one binary, a set of reproducible checkpoints. `xrayctl` takes a **VLESS + REALITY**
server from "fresh box" to "clients connected" without hand-editing any credential field.

```bash
# Linux / macOS (no sudo, installs to ~/.local/bin)
curl -fsSL https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.sh | bash

# Windows
irm https://raw.githubusercontent.com/Helioswei/xrayctl/main/install.ps1 | iex
```

Prefer zero local install? Install only on the server and drive it over ssh:
`bash install.sh --remote my-vps`, then `ssh my-vps xrayctl status`.

## Quick start

```bash
xrayctl --host my-vps deploy --clients laptop,phone   # install xray + keys + config + systemd
xrayctl --host my-vps client sync --qr                # render Clash profile + links + phone QR
xrayctl --host my-vps selftest                        # really connect with every credential
```

`client sync` writes files only. Add `--apply` if you want it to take over your Clash Verge
profile (it backs up the old one first). Credential fields should never be copied by hand —
a single typo produces the same error message as a broken server config, and you will spend
an hour finding it.

## What it covers

```
pick a box  → ssh setup → install xray (pinned version, tested) → open ports
            → per-device credentials (uuid + shortId) → Clash / vless link / QR
            → harden (fail2ban + ufw) → verify (loopback, port, egress IP, traffic ledger)
```

Buying the VPS is out of scope: read prices and options, then hand the purchase back to the human.

## Notes

- Default Xray version is pinned to a **verified** release, not `latest`; `--xray-version` overrides.
- Clients need `support-x25519mlkem768: true` on recent Xray servers — `client sync` emits it.
- Uninstall removes everything by default (`xrayctl uninstall --yes`); `--keep-xray` keeps the tunnel alive.
- Full docs (Chinese): [README.md](README.md), agent workflow: [SKILL.md](SKILL.md)
