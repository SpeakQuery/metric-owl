# Guide for installation assistants

Read this page together with [Installation](INSTALLATION.md),
[Operations](OPERATIONS.md), [Security](SECURITY.md) and the versioned
[validation matrix](PREVIEW.md). It describes the standard GitHub Linux installer.
Do not infer capabilities from a screenshot, a saved API key or a successful build.

## Identify the requested server

Run the installation on the customer's chosen server through their authorized SSH
session. The public marketing site is not a hosted monitoring account. Supported
installer inputs are Linux x86-64 on Ubuntu 22.04/24.04/26.04 or Debian 12/13;
platform-specific validation limits are in PREVIEW.md. Do not substitute a Windows,
ARM64 or macOS package that has not been released.

## Install the versioned package

```sh
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.10/scripts/bootstrap-linux.sh | sudo bash
```

The script fetches the matching GitHub release, verifies its checksums and
source/platform metadata, installs the local monitoring service, and asks for an
administrator username and a twice-entered password in the terminal. Do not put a
password into a command argument, a chat transcript, source control or a public
log. Let the customer enter their own password when working interactively.

Choose the requested access mode:

- **HTTPS:** public IPv4, a free panel port (8443, then 9443 or 10443), and inbound
  TCP 80 for certificate validation are required. Follow the installer's actual
  certificate and firewall diagnostics. The hosting provider's separate firewall
  may need an operator change. Do not bypass failed certificate validation.
- **Private:** no public HTTPS proxy or certificate is installed. Use the printed
  SSH tunnel and local HTTP address. The application listens on loopback at 4877;
  this does not enable public, unencrypted HTTP access.

Report the **exact URL printed by the installer**. Do not guess a port, invent a
domain, expose the loopback listener, overwrite another website's Nginx file or
disable unrelated firewall rules. Installation is incomplete if the script reports
that account or access configuration failed. Rerunning the same installer preserves
an existing account and monitoring history.

## Verify the requested outcome

Confirm the service is active, the printed URL is reachable by the chosen access
mode, login succeeds, and live readings become available. A release download or
HTTP 200 alone does not prove authenticated monitoring. Keep accounts, API keys,
host details and monitoring evidence out of public screenshots and repository files.

## Choose the AI path

**Server AI in the panel:** the customer selects one of the six provider presets
and supplies their own API key. Endpoint/model settings are selected by the
application. A successful connection test establishes provider access; a saved key
alone does not. Manual text chat sends bounded monitoring summaries to that
provider and uses its API billing. Responses are instructed to be short and in the
question's language. Ordinary local alert detection works without AI.

**External AI over MCP:** the customer's Codex or other MCP client reads this
server using local or SSH stdio and its granted read permissions. The external
client's selected model interprets the returned evidence. MCP reads do not use the
server's Grok key or automatically invoke its AI provider. The panel HTTPS URL is
not an HTTP MCP endpoint. Do not create broad shell access as a substitute for a
scoped MCP connection. Each Free installation exposes only its own host.

Historical answers must disclose time range, resolution, gaps and unavailable
metrics. A minute aggregate is not second-by-second history. Process snapshots
cannot reconstruct activity before they were recorded. An unconfigured website,
service, backup or log monitor is unknown, not healthy.

## Account recovery

For a forgotten username, run **on the installed server as root**:

```sh
sudo bash /usr/share/doc/server-guard/recover-admin.sh --show-username
```

For a forgotten password:

```sh
sudo bash /usr/share/doc/server-guard/recover-admin.sh
```

The local helper prompts privately for a new password; it does not reveal the old
one. Read the recovery section in OPERATIONS.md for session/MFA effects and service
restoration. There is no unauthenticated web or MCP password-reset endpoint. This
is server-root recovery, not an email password-reset flow.

## Removal

Use the installed, version-matched uninstaller and the exact inspect/purge commands
in [Operations](OPERATIONS.md#remove-the-application). Default removal retains data;
the explicit purge mode removes owned application data, including SQLite history,
the account and encrypted integration keys. Never replace the ownership checks
with a broad recursive deletion or remove shared Nginx, Certbot, Python, firewall
or database packages. Custom/package-manager installations and ownership conflicts
must be handled through the documented route rather than guessed paths.
