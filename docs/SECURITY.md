# Security model

The application listener binds only to `127.0.0.1`, including behind the default
guided HTTPS setup. The guided installer exposes a trusted HTTPS proxy on the
server’s public IPv4 address; the Rust listener remains private. Manual local-only
installations use loopback or SSH forwarding. Loopback
Host authorities are strictly validated; write requests require the matching
Origin and a session-bound CSRF token. CORS is not enabled. The local HTTP cookie
is HttpOnly and SameSite=Strict; it does not use `Secure` without TLS.

The guided provisioner enables HTTPS browser access by setting
`GUARD_PUBLIC_ORIGIN` to one exact HTTPS origin and `GUARD_PROXY_TOKEN_FILE` to a
private proxy-token file. The guided installer creates a password-based administrator account. MFA is
optional and can be enabled in Security & system. Manual installs retain the one-time key flow;
knowing the public address alone cannot create an administrator. The reverse proxy terminates trusted TLS, supplies the
configured Host, overwrites `X-Forwarded-Proto` with `https`, and overwrites
`X-Guard-Proxy-Token` with the matching private token. Merely forwarding the local
HTTP endpoint or spoofing forwarded headers does not enable public access. The
application checks the exact origin/authority and the authenticated proxy headers.
The proxy must rate-limit authentication requests and keep its token out of client
responses, request logs and published configuration.

HTTPS sessions use the separate `__Host-guard_session` cookie with Secure,
HttpOnly, SameSite=Strict and Path=/, and no Domain attribute. Public and loopback
cookies are not interchangeable. Authentication, session validation and CSRF checks
remain required; a second factor is checked when MFA is enabled. Use the browser panel at `/admin` or `/admin/`; application
assets are namespaced under `/admin/assets/` and authenticated API calls use
`/api/v1/`. Unknown paths return 404. The `/admin` path is a usability choice, not
an authentication boundary. HTTPS dashboard access does not enable HTTP MCP.

The base service installer does not edit virtual hosts or certificates. The guided
HTTPS provisioner detects a public IPv4 address, selects an available panel port
from 8443 / 9443 / 10443, and requests a trusted certificate for that IP. No domain
is required. It owns a separate Nginx configuration, private certificate-client
environment and renewal timer. Existing websites and certificate clients are not
silently replaced. Metric Owl uses port 80 for ACME validation and renewal; other
websites can still serve their existing HTTP traffic on that port. Panel traffic
uses the chosen HTTPS port. The internal application port 4877 stays on loopback.

When UFW is active, the provisioner records and adds only its required rules.
It does not disable the firewall or assume control of cloud firewall settings.
Port availability is checked separately from external reachability. Failed
configuration is reported instead of enabling plaintext login or asking users to
bypass certificate warnings. Default uninstall removes only installer-owned configuration,
renewal units and rules, while retaining account/history and certificate files.
Explicit root-only `--purge` removal is separate and uses ownership/path checks;
missing or unrecognized manifests fail closed. `--dry-run` previews that plan.
Shared OS packages, other websites, unrelated firewall rules and system-wide
journals are outside its scope. See [Operations](OPERATIONS.md#remove-the-application).
Linux installation needs root, but the agent runs as the restricted `server-guard`
account. OS/architecture detection happens locally without uploading an inventory
to GitHub. The marketing website is separate and exposes no customer dashboard.

The guided installer passes administrator credentials through bounded stdin,
stores an Argon2id password hash, and never puts the password in a command argument
or logs. New guided installations can sign in immediately using that username
and password; no setup key or QR enrollment is required. Existing administrators
and their MFA settings are never replaced by rerunning the installer. Enabling,
replacing or disabling MFA requires a signed-in session and revokes all
sessions. When MFA is enabled, login requires a current code
or a single-use recovery code. Optional QR generation is local; no external QR
service receives a secret. The UI keeps newly issued recovery codes visible until
the user confirms saving them, with live requests stopped after session revocation.

An authorized root operator has a separate offline `recover-admin` command for an
existing administrator. It can show the username, or reset a privately entered
password while the agent is stopped. Recovery revokes sessions and disables/clears
MFA and recovery codes, so the owner must enable MFA again after signing in. It
does not delete monitoring history or integration settings. This is an explicit
OS-root recovery path, not a browser/MCP reset or first-time-setup bypass. Passwords
are never supplied through argv/environment. [Recovery runbook](OPERATIONS.md#recover-administrator-access)

Manual initial setup uses a 30-minute key in a mode-0600 file; only its hash is held in the
database. The installer displays the key in its invoking terminal after readiness;
it is never printed by the service or placed in a URL. Administrator creation
removes the file. Existing accounts survive restarts/upgrades and prevent setup
key renewal. Only someone with the local key can complete first-time setup.
TOTP uses six digits, 30-second steps, and a ±1-step tolerance; a consumed
step cannot be reused. Recovery codes are hashed and single-use. Passwords use
Argon2id (19 MiB, 2 passes, 1 lane). Login attempts are persistently rate-limited.
Sessions are hashed in the database. Login issues a browser-session cookie with no
Expires or Max-Age attribute. Reloading the panel preserves the session, including
its CSRF token, and settings changes do not request another password or MFA code.
There is no idle timeout, fixed session lifetime or five-minute verification gate.
On the first upgrade, an atomic migration removes only sessions already expired
under the previous 30-minute idle / 12-hour policy, so old expired tokens do not
become valid again. Still-valid cookies are converted to browser-session cookies
by auth/status; later restarts preserve the new browser-session lifetime.
Sign out revokes the current session and clears its cookie; password or MFA changes
and End all sessions revoke every session. History retention does not revoke logins.

The browser controls when a browser session ends: closing one tab does not end it,
and a browser configured to restore its previous session may restore session
cookies too. Use Sign out to end access reliably. The server cannot observe browser
closure; hashed records from closed browsers remain listed until explicitly
revoked. Possession of an unrevoked session cookie remains sufficient for access,
so session-cookie theft is bounded by explicit revocation rather than a time limit.

API keys use AES-256-GCM encryption. `master.key` has mode 0600 and the data directory
has mode 0700. An attacker controlling the OS account or root can also read the local
master key; encryption is not an independent vault against that threat. Network
clients cannot retrieve secrets; the API reports only whether a key is configured.
Authentication secrets and provider keys are not logged. Optional email is disabled
by default. Users can connect directly to a public SMTP provider over validated
TLS 465 or mandatory STARTTLS 587; a notification relay is an optional alternative.
The encrypted SMTP password is never returned by settings APIs. Server/username
changes require password re-entry, and transport/identity/secret changes invalidate
recipient verification. Email requires a verified recipient; raw logs are not
forwarded. A successful SMTP connection test sends no email, and provider
acceptance is not proof of inbox delivery.

MCP uses the official rmcp SDK and an explicit ten-tool read allowlist. The Unix socket has
mode 0600. Each call also checks the client token hash, scopes, and revocation state.
Limits are 8 connections, 16 KiB requests, 500 KiB responses, short query timeouts,
and a daily per-client query budget. The log tool returns summary counts only.
There are no shell, arbitrary file path, SQL, arbitrary URL, settings mutation, or
service-control tools.

Existing MCP keys keep their granted tools when a release adds tools. Permission
changes require an authenticated, CSRF-protected action, and each call checks the
current grant/revocation record. Incident details include additional historical
process context only with `process_history`; current-process data is already part
of the `current_metrics` grant. A missing log summary returns unknown/null counts,
not fabricated zero errors. Metric coverage, process snapshot gaps and monitor
freshness remain explicit. MCP reads only this installed host through local/SSH
stdio; they never invoke the server's BYOK provider automatically. The calling
client's selected model interprets data with its own account/billing.

Server AI chat is a separate session/Origin/CSRF-protected manual API. It reuses
the existing verified provider among six named presets and has a 20-attempt UTC
daily budget, one in-flight AI operation, bounded context/output and timeouts, and
no automatic retry. Incident explanations use a separate ten-request analysis
budget; automatic explanations are off by default. Basic monitoring needs no AI.
Provider API charges apply independently of the free application.

Chat can read only a fixed bounded summary of this server. It has no tools for
commands, SQL, arbitrary URLs/files, external browsing or server changes. Local
admission checks supplement this code boundary; prompts/keyword filters are not
the authorization boundary. Evidence and prior answers are untrusted data.
Provider errors are sanitized, known key echoes/credential patterns are redacted,
and answers render as plain escaped text. Relevant missing or omitted history
produces an independent notice. Conversations are held in the signed-in page
state, not a new database or local-storage transcript. Selected bounded evidence
is sent to the chosen provider on explicit use; redaction is not a guarantee that
all operational data is non-sensitive. [Chat contract](SERVER_AI_CHAT.md)

Webhooks require HTTPS with no user information or fragment. DNS resolves before
connecting, every address is checked against private/link-local/multicast/loopback
ranges, and the connection is pinned to a validated IP. Environment proxies and
redirects are disabled. Signatures use:
`HMAC-SHA256(secret, unix_timestamp + '.' + raw_json_body)`, with headers
`X-Guard-Timestamp`, `X-Guard-Event`, and `X-Guard-Signature: v1=<hex>`.
Recipients should validate timestamp freshness and deduplicate event IDs. A webhook
response never triggers a server action. Failed deliveries are attempted at most
six times.

The web process never receives `docker.sock` access or Docker group membership.
A separate `server-guard docker-helper --socket /approved/path/helper.sock` process
runs only fixed `docker ps` and `docker stats` commands. Output excludes command,
environment, and connection secrets. Operators configure directory/group access
to the helper socket. The agent selects that socket as its Docker source. Actual
Linux acceptance tests for systemd/Docker remain outstanding.

Limitations: an agent cannot send an alert after its entire host shuts down. External
heartbeat monitoring belongs to a later multi-server service. The agent does not
scan unselected files. It is neither a vulnerability scanner nor an automatic repair tool.
