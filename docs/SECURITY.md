# Security model

The dashboard listens only on `127.0.0.1`. Host and, when present, Origin must match
exactly. Write requests require Origin and a session-bound CSRF token. CORS is not
enabled. The local HTTP cookie is HttpOnly and SameSite=Strict; it does not use the
`Secure` flag without TLS. Do not expose this endpoint directly through a public
reverse proxy. Remote access in v1 uses an SSH tunnel with local port 4877 to satisfy
Host validation. The UI is served only at `/admin` and `/admin/`; `/`,
`/index.html`, and unknown paths return 404. This path is a usability choice, not
an authentication boundary. The API keeps its existing authentication checks.

Installers do not edit Nginx, Apache, virtual hosts, certificate stores, firewall
rules, or ports 80/443. Linux installation needs root to install the service, but
the service runs as the restricted `server-guard` account. No public listener is opened.

Initial setup uses a 30-minute key in a mode-0600 file; only its hash is held in the
database. The installer displays the key in its invoking terminal after readiness;
it is never printed by the service or placed in a URL. Administrator creation
removes the file. Existing accounts survive restarts/upgrades and prevent setup
key renewal. Only someone with the local key can complete first-time setup.
TOTP uses six digits, 30-second steps, and a ±1-step tolerance; a consumed
step cannot be reused. Recovery codes are hashed and single-use. Passwords use
Argon2id (19 MiB, 2 passes, 1 lane). Login attempts are persistently rate-limited.
Sessions are hashed in the database, expire after 30 minutes of inactivity, and have
a maximum lifetime of 12 hours. Sensitive settings require password+MFA confirmation
within the last 5 minutes. Password or MFA rotation revokes all sessions.

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
