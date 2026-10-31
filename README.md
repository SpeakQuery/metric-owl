![Metric Owl — Less watching. More knowing.](website/public/assets/readme-banner.svg)

<p align="center"><img src="web/public/assets/metric-owl.svg" width="76" alt="Metric Owl"></p>
<h1 align="center">Metric Owl</h1>
<p align="center"><strong>Disk filling up? CPU overloaded? Memory running low?</strong><br>Metric Owl watches your server while you build your project.<br>Clear alerts, useful history, and optional AI help — without checking graphs all day.</p>
<p align="center"><code>Rust</code> · <code>SQLite</code> · <code>Self-hosted</code> · <code>Read-only MCP</code> · <code>Optional AI</code></p>
<p align="center"><a href="https://metricowl.cloud/">Website</a> · <a href="docs/INSTALLATION.md">Installation</a> · <a href="docs/SECURITY.md">Security</a> · <a href="docs/NOTIFICATIONS.md">Notifications</a></p>

**Public preview — v0.1.0-preview.3.** Free application binaries for Ubuntu 24.04 x86-64. Source is available for inspection under the project license; this is not an OSI open-source license.

![Metric Owl control center with example data](website/public/assets/dashboard.jpg)

*Actual application interface with illustrative, non-sensitive example data.*

Metric Owl is a self-hosted server monitor built in Rust. Its web control center is bundled into the application. It observes the sources you permit, stores history in its own SQLite database, and records incidents when your rules are met. AI is optional: ordinary monitoring and alert decisions run without a model or an API key.

## Know when your server needs you

You launched the project. Now traffic is growing, logs are filling the disk, and a
quiet server does not always mean a healthy one. Metric Owl brings the useful
signals together and tells you when a sustained condition deserves attention. It
runs continuously while the server and monitoring service are online.

| Your question | What Metric Owl gives you |
| --- | --- |
| “Is my server keeping up?” | Live CPU, memory, network and process measurements, with peaks and history. |
| “Will I run out of disk space?” | Space/inode alerts and a growth estimate when enough consistent history exists. |
| “Do I need to keep checking the panel?” | Ready-made alert packs, grouped incidents and optional verified email reports. |
| “Can I see it from another computer?” | Optional protected HTTPS access: open your panel URL, sign in and view your server. |
| “What changed before this problem?” | Incident evidence, surrounding measurements and optional AI explanations. |
| “Can my own AI assistant investigate?” | Scoped MCP tools for collected metrics, history and incident details. |

Routine monitoring runs locally without an AI key. Add your own provider only
when selected evidence needs a deeper explanation. Your AI client can read the
allowed monitoring data; it cannot run commands or change your server.

## What it does

- **Live performance:** CPU, individual cores, memory, network, disk space and top processes, with explicit unavailable or warming-up states.
- **Useful history:** minute, five-minute, hourly and daily summaries, preserving peaks, weighted averages and data coverage. Short incidents retain surrounding detail.
- **Ready-made alerts:** balanced, sensitive and sustained-load packs; editable thresholds, time windows, occupancy and recovery conditions. Repeated alerts are grouped into incidents.
- **A clear control center:** English throughout, dark and light themes, live pause, zoom, event evidence, setup shortcuts and connection-test feedback.
- **Secure administration:** installer-chosen credentials, optional two-factor authentication with local QR setup and recovery codes, expiring sessions, CSRF/Origin/Host checks and reauthentication for sensitive changes.
- **Optional AI:** bring a Gemini or OpenAI-compatible provider key. Bounded, redacted evidence, a daily budget and cached results. Critical notifications do not wait for a model.
- **Read-only MCP:** server information, current measurements, collected history, incidents and redacted log summaries through the official Rust MCP SDK. Scoped client keys can be revoked.
- **Notifications:** verified recipients, selected report times and a durable queue. Email uses a separately configured notification relay; no mail server is needed on the monitored host.

Additional monitoring includes certificate expiry, HTTP content and latency, scheduled-job heartbeat endpoints, local restart detection, and Linux pressure, swap, inode and disk I/O diagnostics. Unavailable readings are identified rather than treated as healthy. The preview remains a validation release; planned platforms and integrations are not represented as production-ready.

## A clear view, day or night

<table>
<tr><td><strong>Ready-made alert packs</strong><br><img src="website/public/assets/rules.jpg" alt="Metric Owl alert rule presets"></td><td><strong>Connect your AI client</strong><br><img src="website/public/assets/mcp.jpg" alt="Metric Owl MCP connection wizard"></td></tr>
<tr><td colspan="2"><strong>A light theme with the same readable signals</strong><br><img src="website/public/assets/dashboard-light.jpg" alt="Metric Owl light dashboard"></td></tr>
</table>

These are actual application screens using clearly labelled illustrative data.
They do not expose an owner's live server or credentials.

## Platform status

| Platform | Current status |
| --- | --- |
| Ubuntu 24.04 LTS, x86-64 | First packaged preview target; actual-server validation required before each release |
| Apple Silicon macOS | Development and local application validation; public installer package not yet provided |
| Debian 12, Linux ARM64 | Planned validation; not supported by the first downloadable binary |
| Windows Server, Kubernetes | Later work |

The Ubuntu binary is built on Ubuntu 24.04. Its release metadata records the exact source commit and required GLIBC symbol version. It is not advertised as a universal Linux binary.

## One command. Your own server.

Connect to your **Ubuntu 24.04 x86-64 server**, then run:

```bash
curl -fsSL https://metricowl.cloud/install.sh | sudo bash
```

Prefer to inspect the script first? [Read the versioned installer on GitHub](https://github.com/SpeakQuery/metric-owl/blob/v0.1.0-preview.3/scripts/bootstrap-linux.sh). You can also fetch the same pinned version directly:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.3/scripts/bootstrap-linux.sh | sudo bash
```

1. **Answer the installer.** Enter your dedicated panel hostname, choose an
   administrator username and password, and follow the HTTPS prompts.
2. **Open the printed address.** Use any browser. Sign in with the username and
   password you just chose. There is no setup key to copy or QR code required
   in this guided installation.
3. **See your server.** Live monitoring and ready-made alerts are already running.
   Nothing needs to be installed on the computer or phone viewing the dashboard.
   You can enable optional two-factor authentication later in **Security & system**.

The installer verifies release checksums and starts a restricted system service.
The application listener stays on loopback; the HTTPS proxy provides browser
access. No separate database, Node.js service or AI account is required.

For a local-only installation, use `GUARD_CONFIGURE_HTTPS=0` when running the
installer. The printed SSH tunnel forwards the panel without installing a local
server on the viewing computer. See the [installation guide](docs/INSTALLATION.md)
for manual bundles, HTTPS configuration, updates and removal.

## Connect your AI agent

After sign-in, open **Setup guide → Set up MCP connection**. Choose your client, create a scoped key and follow the generated connection instructions. Remote clients use SSH key authentication; an SSH password is never embedded in an MCP configuration.

Example questions:

- “Show a table of CPU usage over the last 24 hours.”
- “Compare when CPU and memory rose or fell during that period.”
- “Explain the evidence around the most recent critical incident.”

Only history collected by this installation is available. MCP tools cannot run shell commands, select arbitrary files or SQL, change settings, or restart services. The public website and the `/admin` panel are not public HTTP MCP endpoints.

## Where your data goes

Measurements and incident history are stored locally. The application writes its own settings, database and queues; read-only monitoring does not mean it never writes its own files. Optional email, AI and webhooks send the selected data needed for those integrations. Provider credentials are not distributed with the application or committed to this repository.

For compatibility, the executable and systemd service remain named `server-guard`; Linux data stays in `/var/lib/server-guard`. Existing macOS installations retain `~/Library/Application Support/SunucuBekcisi` and the `com.sunucubekcisi.agent` launchd identifier. The product UI is Metric Owl.

A process on an offline host cannot send an outage alert from that host. Independent whole-server outage monitoring is deferred to Pro. Local restart detection is a separate feature and does not claim to know the exact outage duration or cause.

## What this preview does not promise

- Full-host outage alerts require an independent monitor; local restart detection
  reports after the host returns.
- Database engine discovery is available. Database contents, query analysis and
  per-database connection metrics require separate connectors.
- Website discovery depends on readable server configuration. Renamed processes
  may not appear in recognized service groups.
- A short passing test is not a long-term performance guarantee. Other Linux
  distributions, ARM64 server packages, Windows and Kubernetes need validation.
- HTTPS access is opt-in. MCP still uses local or SSH stdio and requires the
  appropriate client access; the panel URL is not an HTTP MCP endpoint.

## Development

Pinned tools: Rust **1.99.1**, Node.js **22.23.4**, pnpm **10.32.1**. End users of the binary do not need Rust, Node.js or a separate SQLite installation.

```sh
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo test --workspace --locked
pnpm --dir web test
pnpm --dir web build
cargo build --workspace --locked
python3 scripts/integration-test.py
pnpm --dir web e2e
```

The first frontend build supplies the embedded assets for a clean checkout. Rust tests regenerate the shared TypeScript contracts; the final frontend build uses those exact types. Do not edit `web/src/generated` by hand. `scripts/build.sh` creates a local release and collects dependency licenses.

- [Architecture](docs/ARCHITECTURE.md)
- [Security](docs/SECURITY.md)
- [Notification relay](docs/NOTIFICATIONS.md)
- [Dependencies and licenses](docs/DEPENDENCIES.md)
- [Release process and visibility gate](docs/PUBLIC_RELEASE.md)

## License

Official, unmodified free-edition preview binaries may be downloaded and run for personal or commercial server monitoring without an application fee under [BINARY-LICENSE.txt](BINARY-LICENSE.txt). Optional provider/service charges are separate.

The source remains proprietary and available for inspection under [LICENSE](LICENSE). Public source availability does not make this project open-source software. Third-party components retain their own licenses, included with every binary release.
