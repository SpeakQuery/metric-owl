# Preview scope

## Platform status

The same GitHub installer detects the OS and architecture locally. It accepts
Ubuntu 22.04 / 24.04 / 26.04 LTS and Debian 12 / 13 on x86-64. Detection does not
send your machine inventory to GitHub. ARM64 and Windows binaries are not published.

| Platform | Evidence and current limit |
| --- | --- |
| Ubuntu 24.04 LTS, x86-64 | Previous full server installation validated. The preview.4 fresh-customer installation, systemd and public-IP certificate acceptance are still pending. |
| Ubuntu 22.04 and 26.04 LTS, x86-64 | Static binary passed fresh-account creation, password sign-in, SQLite and real-metric reads in isolated userspaces; email defaults off. Full OS/service/certificate installation is not validated. |
| Debian 12 and 13, x86-64 | Same isolated-userspace application checks passed. Full OS/service/certificate installation is not validated. |
| Apple Silicon macOS | Development/local application validation; no public installer package. |
| Linux ARM64, Windows Server, Kubernetes | No published supported server package. |

The Linux release uses a statically linked musl binary. Compatibility checks for
Ubuntu 22.04 / 26.04 and Debian 12 / 13 shared an Ubuntu 24.04 host kernel. They are
not tests of those distributions' kernels, systemd, firewall integration or ACME
certificate issuance. A short passing test is not a long-term workload guarantee.

## What this preview does not promise

- Full-host outage alerts require an independent monitor; local restart detection
  reports after the host returns.
- Database engine discovery is available. Database contents, query analysis and
  per-database connection metrics require separate connectors.
- Website discovery depends on readable server configuration. Renamed processes
  may not appear in recognized service groups.
- A short passing test is not a long-term performance guarantee. Full installations on the additional Linux
  versions, ARM64 server packages, Windows and Kubernetes need validation.
- HTTPS access is opt-in. MCP still uses local or SSH stdio and requires the
  appropriate client access; the panel URL is not an HTTP MCP endpoint.


The first public IPv4 installer is prepared for v0.1.0-preview.4. Release acceptance must include a real Ubuntu installation and trusted HTTPS access. Local tests alone do not prove certificate issuance or email delivery.
