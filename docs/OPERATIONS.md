# Advanced installation and operations

Start with the [short installation guide](INSTALLATION.md). This page covers manual connections, proxy operations and upgrades.

## Network access

Fresh installations ask you to choose **HTTPS — open from any device (recommended)**
or **Private — connect through SSH** after creating the administrator account.
Enter selects HTTPS. Existing HTTPS installations keep their configuration.
The HTTPS option selects port 8443, then 9443 or 10443 if occupied. It prints the exact HTTPS URL after setup. A free local port does not prove the address is reachable from the Internet.

Metric Owl uses inbound TCP 80 for certificate validation and renewal, not for its dashboard. Other websites may continue using that port for their own HTTP traffic. The selected panel port must also be reachable. When UFW is active, the installer adds only its required access rules; it does not disable the firewall. If your hosting provider has a separate cloud firewall or security group, allow these ports there. DNS and a domain are not required for IPv4 access.

HTTPS setup uses an isolated Certbot environment and reuses usable dependencies.
It may install system Python/venv support packages and update their dependencies.
The terminal shows concise progress; detailed dependency output is kept in root-only
logs on the server. Private mode skips certificate tools and new HTTPS/Nginx setup;
the dashboard stays local and is reached through SSH.

## Open the control center

If you chose local-only access, or need an SSH fallback, run the printed tunnel command on your own computer and keep that terminal open. This forwards the browser connection; it does not install another server. A typical command is:

    ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:14877:127.0.0.1:4877 -p 22 root@YOUR_SERVER_IP

Then open http://127.0.0.1:14877/admin in your browser. The local port differs from the server port so a local Mac installation can remain running.

The guided installer creates your administrator account. Sign in with the chosen username and password; enable optional two-factor authentication later if you want it. For a manual installation without prepared credentials, use the one-time setup key, create your password and enroll your authenticator. That manual key expires after 30 minutes. Once the account exists, subsequent sign-ins use your username and password, plus a second factor only when enabled. Existing installations keep their current MFA settings.

If an unused initial key expires, an authorized server administrator can run the application's reset-setup command as the service user, then use the installed show-setup helper. Reset-setup only works before an administrator exists; it is not a password-reset bypass.

The application service always binds to 127.0.0.1:4877. The HTTPS proxy exposes the selected public IPv4 address on the selected port (8443, 9443 or 10443); existing hosted domains and certificates are preserved. The public product website is separate from each installation’s control center.

## Recover administrator access

Use an SSH terminal on the monitored Linux server with `sudo` or root access.
There is no remote password-reset endpoint. Password hashes cannot reveal an old
password; recovery sets a new one. To display the current administrator username
without changing the account or stopping monitoring:

```bash
sudo bash /usr/share/doc/server-guard/recover-admin.sh --show-username
```

To recover an existing account:

```bash
sudo bash /usr/share/doc/server-guard/recover-admin.sh
```

The helper stops Metric Owl, displays the username, and asks twice for a new
password through a hidden terminal prompt. Nothing you type as a password is
printed, passed in command arguments/environment variables, or written into a
helper file. Use 8 or more characters, up to 256 UTF-8 bytes. A mismatch, invalid
password, or cancellation leaves the password unchanged. The helper restarts
the service if it was running beforehand, including after a failed or cancelled
recovery; a previously stopped service remains stopped.

A successful reset signs out **every browser session** and disables and clears
the old authenticator secret, recovery codes and pending MFA setup. This lets the
server owner recover when the authenticator was lost too. Sign in using the new
password, then enable two-factor authentication again in **Security & system**
if desired. Monitoring history, incident records, rules, MCP keys, encrypted
provider settings and the existing vault key are preserved. Root recovery is
recorded in the local audit trail without passwords.

For a custom installation, the underlying command is
`sudo /path/to/server-guard --data-dir /absolute/private/data recover-admin`;
add `--show-username` for the username only. Stop that installation first, and
restart it afterwards. Reset refuses a live collector, symbolic application
paths, a missing existing administrator/database, or an invalid/missing vault
key. It does not create a replacement account or a new vault key. On standard
installations, prefer the wrapper above because it restores the service even
when recovery is cancelled.

The exact installed preview and source commit, when supplied by the release,
are recorded in `/usr/share/doc/server-guard/release-metadata.json`.

## Optional domain or operator-managed HTTPS

Install Metric Owl once on the machine being monitored. A computer or phone used
to view its HTTPS panel does not need Metric Owl, Rust, Node.js, a database or an
SSH tunnel installed. Open the address supplied by the operator, for example
`https://monitor.example.com/admin`, and sign in with your existing administrator
account. An authenticator code is requested only if two-factor authentication is enabled.

The standard installer uses your public IPv4 address and an available port from 8443, 9443 or 10443. Operators who prefer a dedicated domain or existing reverse proxy can configure the same protected mode manually:

1. The guided installer creates a password-based administrator account. MFA is
   optional in Security & system. Manual installations retain the keyed setup
   option. Existing installations keep their account and MFA.
2. Choose an HTTPS origin you control, with a valid TLS certificate. Keep the Rust
   listener on `127.0.0.1:4877` and leave that port closed to the Internet.
3. Generate a cryptographically random proxy token and store it in a private file
   readable only by the service account and the trusted reverse proxy. Never
   publish it, include it in browser URLs or put it into source control.
4. Set `GUARD_PUBLIC_ORIGIN=https://monitor.example.com` and
   `GUARD_PROXY_TOKEN_FILE=/private/path/to/proxy-token` in the service environment.
   The origin contains only the HTTPS scheme and authority, with no `/admin` path.
5. Configure the existing TLS proxy to forward `/admin`, `/admin/`,
   `/admin/assets/` and `/api/v1/` to the loopback application. It must overwrite
   `Host` with the configured authority, `X-Forwarded-Proto` with `https`, and
   `X-Guard-Proxy-Token` with its private token. Do not pass these values through
   from a browser. Preserve the browser's Origin header for validation.
6. Apply authentication rate limits and request-size limits at the proxy. Disable
   buffering for the live SSE stream, and use the application's security headers
   rather than incompatible marketing-page CSP rules. Validate the proxy config
   before reloading it, then test login, MFA, unauthenticated API rejection,
   cross-origin rejection, logout and live updates.

A domain-based configuration requires DNS pointing to the server. An existing virtual-host conflict requires an explicit operator-managed proxy configuration. The default IPv4 mode obtains a certificate for the public IP and requires inbound port 80 for certificate validation and renewal, plus the selected panel port for access. The product website does not provide a shared account for monitoring
other customers' machines; each installation has its own administrator account.

After HTTPS is enabled, ordinary sign-ins need only the HTTPS `/admin` URL. The
one-time setup key is not requested again. SSH access is still available for local
administration and recovery. MCP remains a separate local or SSH stdio connection;
the HTTPS panel URL is not an HTTP MCP endpoint.

## Connection hints and MCP

After login, open Setup guide, then Set up MCP connection. Create a scoped client key and follow the generated instructions. Remote unattended clients require SSH key authentication. Passwords are not embedded in client configurations.

The installer records only nonsecret SSH host, username and port suggestions in connection-hints.json within the private data directory. It detects the active SSH connection. If detection is unavailable, supply GUARD_SSH_HOST, GUARD_SSH_USER and GUARD_SSH_PORT to the installer. GUARD_LOCAL_PORT changes the local browser tunnel port. All suggestions remain editable in the panel.

## Manual release bundle

An operator may download the versioned archive and SHA256SUMS from the public release page, verify the archive checksum, then unpack it on the supported server. From the unpacked bundle directory run:

    sudo bash scripts/install-linux.sh release

The installer verifies the inner binary checksum and checks that the candidate binary can start before replacing the existing executable. Checksums establish integrity relative to the trusted GitHub release; they are not a separate publisher signature.

## Upgrade and rollback

Re-run the installer with a reviewed newer release. It preserves /var/lib/server-guard, the administrator account, history, encrypted integration settings and systemd drop-ins. Do not delete the data directory when upgrading.

If activation fails, the installer attempts to restore the previous executable, main service unit and service state. It does not reverse database migrations; keep an operator-managed backup of the private data directory before major upgrades. Installation failure is reported rather than resetting credentials.

The executable and systemd unit retain the compatibility name server-guard. The free
edition is open source under [Apache License 2.0](../LICENSE), with third-party notices
included in the distribution. Independent copies may run on multiple servers; the
current dashboard monitors the host where that copy is installed. Optional AI and
email provider charges are separate.

## Remove the application

Run removal commands in a root-capable terminal on the monitored Linux server.
For the standard guided/archive installation, preview the data-preserving plan:

```bash
sudo bash /usr/share/doc/server-guard/uninstall-linux.sh --dry-run
```

Then remove the running application while preserving its data:

```bash
sudo bash /usr/share/doc/server-guard/uninstall-linux.sh
```

This stops/disables the Metric Owl service and removes its executable and main
service unit. Installer-managed panel configuration, renewal units and only the
firewall rules recorded by the HTTPS installer are detached. The administrator,
history and encrypted settings in `/var/lib/server-guard`, dedicated certificate
files, ownership manifest and maintenance helpers are retained. Certificates
are no longer renewed after removal. Reinstallation can reuse this retained data.

To permanently remove the account, database, encrypted integration/MCP keys and
verified dedicated Metric Owl assets, first inspect the purge plan:

```bash
sudo bash /usr/share/doc/server-guard/uninstall-linux.sh --purge --dry-run
```

After reviewing the paths and keeping any required backup, run the explicit purge:

```bash
sudo bash /usr/share/doc/server-guard/uninstall-linux.sh --purge
```

`--purge` deletes verified Metric Owl data, installed application/service/helper/
notice files, dedicated TLS/account/client directories and its installer/panel
logs. It can also follow a previous data-preserving uninstall. It never removes
Nginx, Python, UFW, other OS packages, the `server-guard` operating-system account,
other services, unrelated firewall rules, or the shared system journal. The OS
account is retained because an operator may have reused it outside Metric Owl.

Ownership is recorded in the root-owned
`/etc/metric-owl/install-manifest.json`. Removal refuses missing/unrecognized
ownership, modified installed files, unknown service overrides, unfamiliar data
entries, unsafe links or certificate assets referenced by another configuration.
Normal internal Certbot lineage links and standard private-environment Python
launcher links are checked and unlinked safely; links cannot expand deletion
outside an owned directory. Operator-managed/legacy HTTPS and unfinished HTTPS
cleanup require review. The tool prints an error instead of guessing or using a
generic recursive deletion against an arbitrary path.

For an older installation without a manifest, reinstall a checksum-verified
current release to record recognized ownership, then inspect the plan again.
An unexpected file or custom configuration still requires operator review.
Do not move backups into the application data directory to bypass these checks.

The same non-mutating JSON plan is available through
`sudo python3 /usr/share/doc/server-guard/manage-installation.py inspect --purge`.
Its fields are `mode`, `remove_files`, `remove_trees`, and `preserve`; it contains
paths and resource categories, not account credentials or monitoring payloads.

If installed through the `.deb` package, the standard uninstaller refuses to
delete package-owned files. Keep a checksum-verified unpacked release outside
the application directories and run `sudo apt remove server-guard` first. Then
use that verified release copy to inspect and, if intended, remove the retained
data and owned TLS assets:

```bash
sudo python3 /path/to/verified-release/scripts/manage-installation.py inspect --purge
sudo python3 /path/to/verified-release/scripts/manage-installation.py uninstall --purge
```

The second command is permanent; the first only displays the plan. The manifest
must remain in place. `apt remove` itself preserves monitoring data; package
manager removal is not proof that the Metric Owl database was purged.

For an offline backup, first confirm the monitoring service has stopped, then
archive the complete data directory, including SQLite/WAL files and `master.key`,
into a root-only backup location outside any web directory. Preserve file ownership
and permissions, inspect the archive contents, and verify a restore to a separate
private directory before considering deletion. The archive contains authentication
and integration secrets; do not attach it to a public issue or release. Keep the
certificate directory separately if you intend to reuse it. A purge is permanent;
do not run it until the backup and the displayed removal plan have been reviewed.
