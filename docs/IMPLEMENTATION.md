# Implementation status — 1 November 2026

This page describes the **preview.10 source implementation**. Metric Owl remains
a public preview, not a production-availability guarantee. The native Rust agent
embeds an English React control center and stores its state/history in local
SQLite. The Apache-2.0 free edition monitors one host per installation; independent
copies on multiple servers are permitted. Fleet/team Pro workflows are not available.

For a versioned artifact and its validation, use [Release evidence](PUBLIC_RELEASE.md)
and [Preview scope](PREVIEW.md). A source change, passing test, published checksum
and reachable customer installation establish different things.

## Implemented components

- Real CPU/per-core CPU, available-memory-based Linux RAM, network deltas, disks
  and resource-leading processes. Process CPU is per core and may exceed 100%.
  Linux user threads are excluded from process/service-RSS totals.
- Linux swap, memory PSI, I/O wait, rolling OOM counts, block I/O latency/utilization
  and inode diagnostics. Unsupported/reset/stale readings remain unavailable.
- Minute history, coarser SQLite aggregates, coverage/integrity metadata, incident
  evidence, seven-day prospective process snapshots and quota/retention reporting.
- Editable CPU, memory, disk and Linux diagnostic rules, with severity/recovery and
  time-window conditions. Deleted diagnostic presets are not recreated on restart.
- Selected redacted logs, rotation handling, bounded folders, Linux service/journal
  checks and an optional permission-limited Docker helper. Discovery of declared
  web hosts and running service/database engines does not enable monitoring itself.
- HTTP status/content/JSON/latency and verified TLS checks. Blocked destinations
  stay unavailable; DNS/content failures are failures; redirects are not followed.
- Declared scheduled-job state/deadline/runtime reports. The reporting command does
  not execute a job or validate a backup restore. Host restart detection reports
  after return; it is not independent whole-host outage detection.
- Guided Linux administrator creation, eight-character minimum password, optional
  TOTP/recovery codes, Argon2id, revocable browser sessions and Host/Origin/CSRF
  validation. No repeated timed reauthentication during an active session.
- Public-IP HTTPS proxy with private application listener, or private SSH-forwarded
  access. Existing account/MFA/history are preserved by normal upgrade/reinstall.
- **Six BYOK providers**: Grok (default), OpenAI, Claude, Gemini, DeepSeek and Mistral;
  synthetic generation verification before atomic encrypted save, automatic
  endpoint/model settings and provider-specific text protocols.
- **Server AI text chat**: single-host TR/EN concise replies, inferred bounded
  periods, readable UTC peak intervals, fixed read-only evidence, visible 20-attempt
  UTC daily budget, manual retry and independent relevant-history notices. No
  uploads/audio, general-purpose agent tools or persistent chat database.
- Optional cached incident explanations with a separate 10-request daily budget.
  Automatic explanations are off by default; routine metric rules remain local.
- **Ten read-only MCP tools**, local/SSH stdio wizard, private key files, generated
  Codex/JSON configuration, editable per-client grants, scopes and revocation.
  The calling client's selected model interprets MCP evidence; MCP does not call
  the server's Grok key. There is no HTTP MCP endpoint or multi-host selector.
- Optional direct SMTP over verified TLS/STARTTLS, recipient verification and a
  durable queue; no local mail server is required. Optional relay and signed HTTPS
  webhooks are separate. Webhook responses cannot trigger server actions.
- Public product website and illustrative demo media, separate from each private
  installation's dashboard, authentication and monitoring data.

## Evidence and remaining limits

Automated tests exercise real isolated HTTP routing/auth/CSRF, bounded analytic
fixtures, scopes/revocation, process identity, quota/history behavior, installer
contracts and browser journeys. Provider/UI fixtures use mocks where documented.
A successful real provider call verifies that selected account at that moment,
not every provider, future availability or general root-cause accuracy. Never
publish test keys, real telemetry or private operator evidence with release assets.

Linux x86-64 packaging targets the Ubuntu/Debian versions accepted by the installer.
The platform matrix records how deeply each was checked. macOS is for development
and local validation, not evidence of Linux service/TLS installation. Linux ARM64,
Windows Server and Kubernetes have no published supported package.

History begins with collection and may be reduced by quota pressure. Metric
windows can cover up to 366 days; Server AI limits its summary window to 60 days;
process-history queries cover up to seven days. These limits do not promise that
those durations of data exist. Snapshots do not establish continuous per-process
causation. Aggregates cannot recover exact spike seconds or arbitrary threshold
durations; statistical correlation/outliers are not proof of root cause.

Website/job monitor states are latest configured observations rather than complete
historical uptime. Log summary counts are latest-check batches, not a full-text
archival search. Database names/sizes/connections/query performance require separate
read-only database integrations; only engine/process discovery is included now.
Backup reports do not prove restorability. Whole-host outage alerts require an
independent monitor. Long-running pathological workloads and additional full-OS
installations need separate validation.

SMTP authentication, provider acceptance, queue acceptance and inbox delivery are
different states. Checksums verify integrity in the trusted release channel; a
separate publisher-signature system and macOS notarization are not implied.
Recovery and destructive removal are explicit root-only operator actions, distinct
from first-time setup and normal preserve-data uninstall. See [Operations](OPERATIONS.md).

[Architecture](ARCHITECTURE.md) · [Security](SECURITY.md) · [AI](AI_CONNECTIONS.md) · [MCP](MCP_CONNECTIONS.md)
