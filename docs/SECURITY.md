# Security model

The application listener binds only to `127.0.0.1`, including when optional HTTPS
access is enabled. Default access is local or through SSH forwarding. Loopback
Host authorities are strictly validated; write requests require the matching
Origin and a session-bound CSRF token. CORS is not enabled. The local HTTP cookie
is HttpOnly and SameSite=Strict; it does not use `Secure` without TLS.

An operator may explicitly enable HTTPS browser access by setting
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
cookies are not interchangeable. Authentication, MFA, session expiry and CSRF
checks remain required. Use the browser panel at `/admin` or `/admin/`; application
assets are namespaced under `/admin/assets/` and authenticated API calls use
`/api/v1/`. Unknown paths return 404. The `/admin` path is a usability choice, not
an authentication boundary. HTTPS dashboard access does not enable HTTP MCP.

The basic service installer does not edit virtual hosts or certificates. The
optional HTTPS provisioner adds a dedicated Nginx host and certificate after the
operator selects a hostname; existing virtual-host conflicts fail safely. It does
not silently replace other sites or open the application port to the network.
Linux installation needs root to install the service, but the service runs as the
restricted `server-guard` account. No public application listener is opened.

The guided installer passes administrator credentials through bounded stdin,
stores an Argon2id password hash, and never puts the password in a command argument
or logs. New guided installations can sign in immediately using that username
and password; no setup key or QR enrollment is required. Existing administrators
and their MFA settings are never replaced by rerunning the installer. Enabling,
replacing or disabling MFA requires recent reauthentication and revokes all
sessions. When MFA is enabled, login and reauthentication require a current code
or a single-use recovery code. Optional QR generation is local; no external QR
service receives a secret. The UI keeps newly issued recovery codes visible until
the user confirms saving them, with live requests stopped after session revocation.

Manual initial setup uses a 30-minute key in a mode-0600 file; only its hash is held in the
database. The installer displays the key in its invoking terminal after readiness;
it is never printed by the service or placed in a URL. Administrator creation
removes the file. Existing accounts survive restarts/upgrades and prevent setup
key renewal. Only someone with the local key can complete first-time setup.
TOTP uses six digits, 30-second steps, and a ±1-step tolerance; a consumed
step cannot be reused. Recovery codes are hashed and single-use. Passwords use
Argon2id (19 MiB, 2 passes, 1 lane). Login attempts are persistently rate-limited.
Sessions are hashed in the database, expire after 30 minutes of inactivity, and have
a maximum lifetime of 12 hours. Sensitive settings require password confirmation
within the last 5 minutes, with a second factor when MFA is enabled. Password or
MFA changes revoke all sessions.

API keys use AES-256-GCM encryption. `master.key` has mode 0600 and the data directory
has mode 0700. An attacker controlling the OS account or root can also read the local
master key; encryption is not an independent vault against that threat. Network
clients cannot retrieve secrets; the API reports only whether a key is configured.
Authentication secrets and provider keys are not logged. Raw logs are not forwarded
to the notification relay.

MCP uses the official rmcp SDK and an explicit tool allowlist. The Unix socket has
mode 0600. Each call also checks the client token hash, scopes, and revocation state.
Limits are 8 connections, 16 KiB requests, 500 KiB responses, short query timeouts,
and a daily per-client query budget. The log tool returns summary counts only.
There are no shell, arbitrary file path, SQL, arbitrary URL, settings mutation, or
service-control tools.

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
