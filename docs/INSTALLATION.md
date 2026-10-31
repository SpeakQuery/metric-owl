# Install Metric Owl on your server

This guide uses the versioned **v0.1.0-preview.10** Linux release. The application,
account, dashboard and SQLite history run on the server you install it on. Each
installation monitors that one host; the public website has no shared customer
login. A viewing computer or phone needs only a browser for HTTPS access.

## Before you start

- Use a Linux **x86-64** server with **systemd**, SSH access and root/sudo rights.
  The installer accepts Ubuntu **22.04 / 24.04 / 26.04 LTS** and Debian **12 / 13**.
  Accepted versions and full installation evidence are different; check the
  [platform validation matrix](PREVIEW.md).
- For the guided HTTPS option, use a public **IPv4** address reachable from the
  Internet. No domain or Metric Owl cloud account is required. Port **80/TCP**
  and the selected HTTPS panel port must be reachable through both the host
  firewall and any hosting-provider firewall.
- If public-IP HTTPS is unsuitable, choose private SSH access instead. macOS is
  a development/local-validation path; there is no supported public macOS,
  Linux ARM64 or Windows production installer package.

The one-line command needs `curl` and `sudo` already available. Once downloaded,
the script can install missing Linux download prerequisites and HTTPS support.
It does not install Metric Owl on your viewing device.

## 1. Run the pinned installer

In your **server's SSH terminal**, run:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.10/scripts/bootstrap-linux.sh | sudo bash
```

If you are already root on a minimal server without `sudo`, use:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.10/scripts/bootstrap-linux.sh | bash
```

If `curl` is not installed yet, run `apt-get update` followed by
`apt-get install -y curl ca-certificates` as root, then repeat the pinned command.

[Read the script first](https://github.com/SpeakQuery/metric-owl/blob/v0.1.0-preview.10/scripts/bootstrap-linux.sh).
It fetches the matching [release assets](https://github.com/SpeakQuery/metric-owl/releases/tag/v0.1.0-preview.10),
verifies each asset against `SHA256SUMS`, and checks `release-metadata.json` for the
version, source commit, static Linux architecture and accepted platform. OS
detection is local; it does not upload a machine inventory to GitHub.

If the version is unpublished or the checksums do not match, stop. Do not bypass
verification or switch to an unversioned binary. Checksums establish integrity
relative to the trusted release channel, not a separate publisher signature.

## 2. Create your administrator and choose access

Choose your administrator username and enter your password twice. Usernames use
3–64 letters, digits, `.`, `_`, `@` or `-`. Passwords need at least 8 characters;
uppercase letters, numbers and symbols are optional. Short or mismatched passwords
are requested again in the same terminal. Passwords are entered privately, not
passed in a command argument.

Next choose:

| Choice | What happens | Browser address |
| --- | --- | --- |
| **HTTPS — open from any device** (default) | Public-IP certificate and protected HTTPS proxy; application remains on loopback | Printed `https://YOUR_SERVER_IP:PORT/admin` |
| **Private — connect through SSH** | Skips new HTTPS/certificate setup; use SSH forwarding from your viewing computer | Printed `http://127.0.0.1:LOCAL_PORT/admin` after the tunnel is open |

For HTTPS, the script selects an unambiguous public IPv4 automatically or asks
which address to use. Review the detected address and the linked Let's Encrypt
terms, then enter `yes` if you accept. It selects a free port from **8443**, **9443**
and **10443**, in that order, and prints the actual URL. It does not stop a service
to take its port.

Existing accounts, MFA settings, history and HTTPS configuration are retained on
upgrade. Choosing or forcing private setup does not remove an existing HTTPS
configuration. Reinstalling is not a password reset.

## 3. Sign in and check the result

For HTTPS, open the exact printed `/admin` address and sign in with your chosen
username/password. No setup key, authenticator app or SSH tunnel is required for
this guided sign-in. Optional two-factor authentication can be enabled later in
**Security & system**.

For private access, run the printed SSH command on the **computer you view from**,
keep that terminal open, then open the printed local URL. For a default-port
installation, a typical tunnel is:

```bash
ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:14877:127.0.0.1:4877 -p 22 YOUR_SSH_USER@YOUR_SERVER_IP
```

Then open `http://127.0.0.1:14877/admin`. Substitute your real SSH account, host and
SSH port. The local HTTP hop is inside this SSH tunnel; this is not public HTTP
login. Do not open server port 4877 to the Internet.

Check that you can sign in, receive fresh readings and sign out. A successful
installer exit, downloaded artifact or build alone does not prove that your
hosting-provider firewall permits the panel or that your application is healthy.

## HTTPS dependencies and constraints

The application service always listens on `127.0.0.1:4877`. Guided HTTPS uses a
separate Nginx proxy and an IP certificate from Let's Encrypt's short-lived
profile. Inbound **TCP 80 must stay reachable for certificate issuance and
renewal**; it is not the Metric Owl dashboard port. Panel traffic uses the
selected HTTPS port. DNS is not required for the IP option.

The script reuses usable dependencies. It may install `nginx`, `python3-venv` and
supporting OS dependencies, and maintains a private pinned Certbot environment
under `/opt/metric-owl/certbot`. Dependency updates are real system changes.
Concise progress is shown; detailed dependency logs are kept privately under
`/var/log/metric-owl-installer`. The bootstrap can install `python3`, `curl`,
`ca-certificates`, `iproute2` and `util-linux` when its download prerequisites are
missing. No Rust, Node.js or separate database server is needed to run the binary.

The installer preserves existing websites and certificates. Existing Nginx may
be reused. A non-Nginx listener on TCP 80, an explicit conflicting IP virtual host,
occupied candidate ports or an unsafe prior configuration stops setup for operator
review. It does not replace hosted sites or enable plaintext remote login. UFW
rules are added only when UFW is active; cloud firewalls remain the operator's
responsibility. A free local port is not proof of external reachability.

If setup says **SETUP INCOMPLETE**, read the reported issue and keep your data.
The account/service may already exist while HTTPS is unfinished. Correct the
network/configuration problem and rerun the pinned command; do not delete the
account, disable certificate validation or bypass browser certificate warnings.
[Troubleshooting and recovery](OPERATIONS.md)

## Operator controls

The guided command is enough for normal installation. These environment flags
are for deliberate operator use, on the privileged installer process:

| Variable | Source behavior |
| --- | --- |
| `GUARD_CONFIGURE_HTTPS=0` | Skip new HTTPS configuration; keep local/SSH access and any existing HTTPS files |
| `GUARD_CONFIGURE_HTTPS=1` | Choose HTTPS without the access-mode menu; certificate consent is still required |
| `GUARD_PUBLIC_IP` | Explicit public IPv4 address, without scheme, port or path |
| `GUARD_PANEL_PORT` | Explicit free HTTPS port 1024–65535, excluding 4877 and 4878 |
| `GUARD_HTTPS_AGREE_TOS=1` | Record the operator's explicit acceptance of the linked certificate terms |
| `GUARD_HTTPS_EMAIL` | Optional certificate contact email |
| `GUARD_PREPARE_ADMIN=0` | Skip guided account preparation; manual keyed setup remains separate |
| `GUARD_LOCAL_PORT` | Printed browser-tunnel port; default 14877 |
| `GUARD_SSH_HOST`, `GUARD_SSH_USER`, `GUARD_SSH_PORT` | Nonsecret SSH connection hints; not credentials or authorization |

For example, to choose private mode on a new installation:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.10/scripts/bootstrap-linux.sh | sudo env GUARD_CONFIGURE_HTTPS=0 bash
```

Never put passwords or private keys into these flags. New guided administrator
password entry still needs a terminal. There is no flag for public plain-HTTP
login. Operator-managed domain HTTPS is described in [Operations](OPERATIONS.md).

## After installation

- **Monitoring:** inspect Overview, History and Alert rules. Select the sources
  you want monitored; discovery does not enable every check automatically.
- **Server AI:** connect one of six BYOK providers, then ask text questions in
  Turkish or English. Optional, paid provider usage; 20 chat attempts per UTC day.
  [AI setup](AI_CONNECTIONS.md) · [Chat guide](SERVER_AI_CHAT.md)
- **External AI client:** create a separate scoped local/SSH stdio connection.
  The panel URL is not an HTTP MCP endpoint. [MCP guide](MCP_CONNECTIONS.md)
- **Email:** optional SMTP, disabled by default. Verify the recipient and test
  delivery; provider acceptance is not inbox-delivery proof. [Email](NOTIFICATIONS.md)

## Forgotten credentials, upgrades and removal

Forgotten username/password or unavailable MFA requires authorized root access on
this server; use [administrator recovery](OPERATIONS.md#recover-administrator-access).
Do not delete the database or rerun first-time setup to bypass an existing account.
Keep a private backup before an upgrade or destructive removal.

Default removal preserves account/history and encrypted settings:

```bash
sudo bash /usr/share/doc/server-guard/uninstall-linux.sh
```

Permanent data removal is a separate explicit root-only operation with ownership
checks. Inspect its plan first:

```bash
sudo bash /usr/share/doc/server-guard/uninstall-linux.sh --purge --dry-run
```

`--purge` without `--dry-run` permanently removes validated Metric Owl-owned data
and secrets, including the account/history; back up anything you need first.
Missing or unrecognized ownership records and unsafe/shared assets stop removal
for review. Read [preserve/purge behavior](OPERATIONS.md#remove-the-application)
before executing it. Shared OS packages, other sites and unrelated firewall
rules are outside the removal scope.

[AI operator guide](AI_OPERATOR_GUIDE.md) · [Operations](OPERATIONS.md) · [Security](SECURITY.md) · [Release evidence](PUBLIC_RELEASE.md) · [Preview scope](PREVIEW.md)
