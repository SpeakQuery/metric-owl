# Preview scope

## Free edition and license

Starting with **v0.1.0-preview.8**, the free edition is open source under
[Apache License 2.0](../LICENSE). Each installation monitors its own host; independent
copies on multiple servers are allowed without a license server-count limit.
Optional AI provider fees are paid by the user; email provider charges may also apply.
Pro fleet management and team workflows are planned for a separate repository and
are not available yet. The versioned checks below remain evidence for the versions
tested, not a claim that every subsequent release repeated a full installation.

## Platform status

The same GitHub installer detects the OS and architecture locally. It accepts
Ubuntu 22.04 / 24.04 / 26.04 LTS and Debian 12 / 13 on x86-64. Detection does not
send your machine inventory to GitHub. ARM64 and Windows binaries are not published.

| Platform | Evidence and current limit |
| --- | --- |
| Ubuntu 24.04 LTS, x86-64 | Fresh v0.1.0-preview.5 installation passed on a real server using the exact anonymous GitHub command. The service started, a trusted public-IP certificate was issued on port 8443, and the installer created an account from a username and twice-entered password without a domain, setup key or required MFA. External HTTPS password sign-in, live data and chart rendering passed. |
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
- The guided installer configures public-IP HTTPS by default. Local-only access
  remains available. MCP uses local or SSH stdio with separate client access;
  the panel URL is not an HTTP MCP endpoint.


The v0.1.0-preview.5 fresh Ubuntu 24.04 installation and trusted-IP certificate
issuance are validated. External API checks also confirmed secure session cookies,
unauthenticated-metric rejection and foreign-origin rejection, with email off and
no provider keys or relay configured. A certificate-renewal dry run against
Let’s Encrypt staging succeeded, and the server test preserved existing hosted-site
configuration, certificates and checked routes. This does not establish full installation support on the
other distributions. Direct SMTP inbox delivery has not been tested; connection
checks and provider acceptance must not be presented as delivery proof.

The distributed uninstaller passed on Ubuntu 24.04 while preserving the test account
and history. A second fresh GitHub installation with port 8443 deliberately occupied
selected 9443; trusted HTTPS, password sign-in, live CPU and SSE passed, and the
existing 8443 listener remained untouched.

Fresh SSH MCP checks passed for server information, current readings and aligned
CPU/RAM history; restricted scopes and revoked access were rejected, with no AI
requests made.

## Installer patch: preview.6

Fixes detection of the CloudPanel `sites-enabled/*.conf` layout. Administrator
creation is now first; a uniquely detected public IP is used automatically.
The application binary is unchanged from preview.5. Installer regression coverage
is separate from the preview.5 full-host acceptance above; a complete customer
CloudPanel installation remains to be verified.

## Password policy update: preview.7

The application and bundled dashboard were rebuilt with an eight-character password
minimum and no character-type requirements. The 256-byte maximum remains. Short or
mismatched installer passwords are requested again in the same session. Authentication
and browser regressions checked the seven/eight-character boundary, password changes,
session revocation and optional MFA. This is separate from the preview.5 full-host
installation evidence above.
