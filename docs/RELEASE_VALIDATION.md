# Preview.10 release validation

Published 1 November 2026 (Europe/Istanbul). This is a public prerelease, not a
claim that every Linux distribution or mail client has completed acceptance.

- Release: [v0.1.0-preview.10](https://github.com/SpeakQuery/metric-owl/releases/tag/v0.1.0-preview.10).
- Application source: [`3ae4a274b26a56264bcd254e14867e44a44ba7a0`](https://github.com/SpeakQuery/metric-owl/commit/3ae4a274b26a56264bcd254e14867e44a44ba7a0).
- Static Linux binary SHA-256: `29fee0af6ec2fd3d705f762fc45555bce64991f3d8b8975cc27d331d9e4c7d24`.
- All 16 release assets were checked against their versioned SHA256SUMS; anonymous
  downloads of the binary, bootstrap and metadata were independently verified.

## Source, application and package

[Ubuntu source validation passed](https://github.com/SpeakQuery/metric-owl/actions/runs/41245532353).
It covers the source boundary audit, pinned frontend build, Rust tests/Clippy,
root-only recovery ownership regression, API/relay/MCP integration, public-access
contracts, installer/HTTPS/monitoring fixtures, browser acceptance and static Linux
packaging. Local checks also passed 146 Rust tests, 28 frontend unit tests, nine
browser application flows and 18 synthetic responsive email previews.

## Actual published installer

[Fresh published-package lifecycle passed](https://github.com/SpeakQuery/metric-owl/actions/runs/41246213659)
on a disposable Ubuntu 24.04 x86-64 GitHub VM. It ran the actual anonymous,
version-pinned curl command, selected private SSH/loopback access and verified:

1. Terminal administrator setup, password login and real collected metrics.
2. No AI provider key required for monitoring.
3. Root username lookup without creating root-owned SQLite sidecars, including a
   fixture with a newer live WAL value.
4. Hidden-terminal password recovery, old-session revocation and service restart.
5. Default removal and reinstallation preserving the recovered account/data.
6. Non-mutating purge preview and actual removal of the application's owned data,
   SQLite database, integration keys, binary and maintenance files.
7. An unrelated sentinel file remained unchanged.

## Limits of this evidence

The fresh lifecycle used private HTTP/SSH access; it did not issue a new public-IP
HTTPS certificate. Historical versioned certificate/deployment evidence and the
other platform limits remain in [PREVIEW.md](PREVIEW.md). Ownership, symlink,
shared-TLS and unknown-path behavior also has isolated fixtures; a passing sentinel
check is not a guarantee about arbitrary custom installations.

No real AI provider call or external email was sent by these release tests.
Browser email previews do not establish Gmail/Outlook inbox rendering or delivery.
An upgraded optional relay is needed for the shared HTML templates; older relays
retain their plain-text composition. Shared OS packages, the service account,
system journal, external backups and client SSH/MCP configuration are deliberately
retained by purge. See [Operations](OPERATIONS.md) before permanent removal.
