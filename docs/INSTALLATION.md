# Install Metric Owl

The public preview supports Ubuntu 24.04 LTS on x86-64. Debian 12 and ARM64 are
not supported by this binary. Application binaries are free under BINARY-LICENSE.txt;
the source is published for inspection under LICENSE.

## One-command installation

Connect to the server with SSH, then run this **on the server**:

```bash
curl -fsSL https://metricowl.cloud/install.sh | sudo bash
```

The script downloads the pinned `v0.1.0-preview.3` release, verifies checksums,
installs the service and bundled panel, and offers HTTPS setup. Choose an
administrator username and password in the installer. Supply a dedicated hostname
you control whose DNS points to the server, then review the certificate terms
when prompted. The installer does not silently accept terms or overwrite
existing virtual hosts. When complete it prints the exact panel URL. Open it, enter the credentials you
just chose, and start monitoring. This guided flow does not require a setup key
or an authenticator. Optional two-factor authentication can be enabled later in
**Security & system**.

The same versioned script can be inspected or downloaded from GitHub:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.3/scripts/bootstrap-linux.sh | sudo bash
```

For local-only installation, set `GUARD_CONFIGURE_HTTPS=0` for the installer:

```bash
curl -fsSL https://metricowl.cloud/install.sh | sudo env GUARD_CONFIGURE_HTTPS=0 bash
```

With HTTPS configured, open the printed `https://YOUR_PANEL_HOST/admin` address
from any computer or phone. No viewer-side installation or SSH tunnel is required.
An existing HTTPS configuration is preserved on upgrade; the installer prints its
saved panel URL again.

## Open the control center

If you chose local-only access, or need an SSH fallback, run the printed tunnel command on your own computer and keep that terminal open. This forwards the browser connection; it does not install another server. A typical command is:

    ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:14877:127.0.0.1:4877 -p 22 root@YOUR_SERVER_IP

Then open http://127.0.0.1:14877/admin in your browser. The local port differs from the server port so a local Mac installation can remain running.

The guided installer creates your administrator account. Sign in with the chosen username and password; enable optional two-factor authentication later if you want it. For a manual installation without prepared credentials, use the one-time setup key, create your password and enroll your authenticator. That manual key expires after 30 minutes. Once the account exists, subsequent sign-ins use your username and password, plus a second factor only when enabled. Existing installations keep their current MFA settings.

If an unused initial key expires, an authorized server administrator can run the application's reset-setup command as the service user, then use the installed show-setup helper. Reset-setup only works before an administrator exists; it is not a password-reset bypass.

The application service always binds to 127.0.0.1:4877. Optional HTTPS provisioning adds a dedicated virtual host and certificate; existing virtual hosts are not overwritten. The public product website is separate from each installation’s control center.

## Optional HTTPS browser access

Install Metric Owl once on the machine being monitored. A computer or phone used
to view its HTTPS panel does not need Metric Owl, Rust, Node.js, a database or an
SSH tunnel installed. Open the address supplied by the operator, for example
`https://monitor.example.com/admin`, and sign in with your existing administrator
account. An authenticator code is requested only if two-factor authentication is enabled.

The installer can provision a dedicated hostname automatically. For an existing reverse proxy, the operator can configure the same protected mode manually:

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

Automatic HTTPS provisioning requires a dedicated hostname with DNS already pointing to the server. An existing virtual-host conflict requires an explicit operator-managed proxy configuration. A public IP alone
is not a valid browser HTTPS setup unless it has a trusted certificate matching
that IP. The product website does not provide a shared account for monitoring
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

The executable and systemd unit retain the compatibility name server-guard. Official preview binaries may be used without an application license fee under BINARY-LICENSE.txt. Optional external service charges are separate.

## Remove the application

The supplied uninstall-linux.sh stops and removes the Metric Owl service and executable. Review the script before running it as root. It preserves the data directory by default. Removing private history is a separate, explicit operator action.
