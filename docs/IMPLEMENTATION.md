# Implementation status — October 31, 2026

Metric Owl is an English, self-hosted monitoring application. The native Rust agent
bundles the React control center and stores its history in local SQLite. The current
release is a **private preview**, not a public production release. Repository visibility
must remain private until the owner explicitly changes it.

## Working components

- Real CPU, per-core CPU, available-memory-based usage, network counters, disks and top processes. Linux user threads are excluded from process counts and service RSS totals.
- Linux diagnostic readers for swap, memory PSI, CPU I/O wait, OOM counters, block I/O latency/utilization and inode usage. Unsupported, stale and reset counters are unavailable rather than fabricated zeros.
- Minute history and progressively coarser SQLite aggregates, event evidence, retention pressure and quota reporting.
- Ready-made CPU, memory, disk and Linux diagnostic rules; editable time windows, recovery and severity. User-deleted diagnostic presets are not recreated on every restart. Small partitions retain percentage rules; the fixed 1 GiB free-space default is seeded only for volumes of at least 10 GiB.
- Selected log files with redaction and rotation handling; bounded folder scanning; optional Linux service/journal and permission-limited Docker helper sources.
- Bounded discovery of declared Nginx/Apache hostnames and running service/database engines. Discovery does not enable monitoring automatically or change web configuration.
- HTTP status, expected text/JSON and response-time checks. DNS and content-check failures produce failure measurements; blocked network destinations remain unavailable. Redirects are not followed.
- Verified TLS certificate expiry and failure checks. Verification failures do not produce a fabricated expiry date.
- Scheduled-job reports and deadline/runtime checks. The report command records a job's declared state; it does not execute the job or prove a backup can be restored.
- Host restart detection after the machine returns. Agent restarts do not masquerade as host reboots. Independent offline detection is reserved for a future external monitor.
- Password confirmation at initial setup, one-time expiring setup key, Argon2id, required TOTP, recovery codes, expiring sessions, reauthentication, Origin/Host/CSRF checks and revocation.
- Private loopback dashboard, with alternate local SSH tunnel ports supported. Installers print the actual server connection details when available; existing administrator accounts are preserved.
- MCP setup wizard for local or SSH stdio. The app creates private client key files and copyable Codex/JSON configuration. Six read tools, scoped access, query bounds and revocation are enforced.
- Notification relay with local-test, Postmark and authenticated SMTP providers. SMTP requires verified TLS; recipient verification and durable queue remain mandatory. No local mail server is required.
- Optional OpenAI-compatible/Groq/xAI and Gemini analysis adapters, bounded evidence, daily request budget and cached results. Routine monitoring works without AI.
- Signed HTTPS webhook integration with target validation; a webhook response cannot trigger server commands.
- English public product website, illustrative application screenshots and a browser-only server-address connection helper.

## Verification

The acceptance report and raw evidence are generated under `artifacts/` and are not
part of the source distribution. Tests cover real application HTTP APIs and MCP,
synthetic 24-hour aligned CPU/RAM history, actual Mac metrics, source lifecycle,
authentication, installer output, and current browser interactions. Actual Ubuntu
acceptance is reported separately from macOS; a Mac build is never Linux evidence.

A single bounded xAI generation with synthetic data succeeded. Automatic AI analysis
remains disabled in the test installation. Credentials are stored outside source code
and encrypted in the agent database. SMTP authentication and one provider-accepted test message succeeded against the configured
provider; inbox delivery has not been confirmed.

## Deliberate preview limits

- Public downloads, Pro, Windows Server, Kubernetes and a multi-server dashboard are not available.
- The initial Linux artifact targets Ubuntu 24.04 x86-64. Debian 12, Linux ARM64 and other distributions require their own installation tests; an Ubuntu binary is not implicitly compatible with older glibc.
- Monitoring only has history collected since installation. The 24-hour MCP acceptance table is a clearly labelled synthetic test, not a claim of 24 hours of real uptime.
- Rule preview uses the last five minutes of detailed samples. Aggregated averages cannot recreate exact long-window threshold durations.
- Selected log files provide recent redacted lines, not a full-text archival log search. MCP's log summary is numeric, not an unrestricted file reader.
- Database names, sizes, connection counts and query performance need separate read-only database integrations. Only engine/process discovery is implemented now.
- Per-process historical charts, target-specific long-term latency charts, full backup validation and automatic anomaly root-cause certainty are not implemented.
- SSH MCP requires pre-existing noninteractive SSH access and permission to read the selected installation's private data. The wizard does not grant root access or install SSH keys.
- SMTP acceptance, queue acceptance and inbox delivery are different states. Bounce/delivery callbacks, public hosted enrollment and notification service terms remain release work.
- A short CPU/RSS observation is not a 24-hour soak test. Heavy log scans, pathological filesystems and sustained multi-client workloads need separate validation.
- Checksums verify downloaded artifact integrity within the authenticated release channel. Signed public manifests and macOS codesign/notarization remain release work.

The original macOS data directory and launchd identifier are retained for upgrade
compatibility. Internal binary/service names are stable identifiers, not the displayed
product name. All application-owned UI, MCP descriptions, mail templates and generated
instructions are English; custom user text is preserved.
