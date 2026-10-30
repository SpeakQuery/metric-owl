<p align="center"><img src="web/public/assets/metric-owl.svg" width="76" alt="Metric Owl"></p>
<h1 align="center">Metric Owl</h1>
<p align="center"><strong>A watchful companion for your server.</strong><br>Live metrics. Clear alerts. Local history. Optional AI insight.</p>
<p align="center"><a href="https://metricowl.cloud/">Website</a> · <a href="docs/INSTALLATION.md">Installation</a> · <a href="docs/SECURITY.md">Security</a> · <a href="docs/NOTIFICATIONS.md">Notifications</a></p>

**Private preview — v0.1.0-preview.2.** The repository and release assets remain private during validation. Public downloads are not open. Changing repository visibility is an owner decision; no installer or workflow changes it automatically.

![Metric Owl control center with example data](website/public/assets/dashboard.jpg)

*Actual application interface with illustrative, non-sensitive example data.*

Metric Owl is a self-hosted server monitor built in Rust. Its web control center is bundled into the application. It observes the sources you permit, stores history in its own SQLite database, and records incidents when your rules are met. AI is optional: ordinary monitoring and alert decisions run without a model or an API key.

## What it does

- **Live performance:** CPU, individual cores, memory, network, disk space and top processes, with explicit unavailable or warming-up states.
- **Useful history:** minute, five-minute, hourly and daily summaries, preserving peaks, weighted averages and data coverage. Short incidents retain surrounding detail.
- **Ready-made alerts:** balanced, sensitive and sustained-load packs; editable thresholds, time windows, occupancy and recovery conditions. Repeated alerts are grouped into incidents.
- **A clear control center:** English throughout, dark and light themes, live pause, zoom, event evidence, setup shortcuts and connection-test feedback.
- **Secure administration:** a one-time setup key, password confirmation, required TOTP, recovery codes, expiring sessions, CSRF/Origin/Host checks and reauthentication for sensitive changes.
- **Optional AI:** bring a Gemini or OpenAI-compatible provider key. Bounded, redacted evidence, a daily budget and cached results. Critical notifications do not wait for a model.
- **Read-only MCP:** server information, current measurements, collected history, incidents and redacted log summaries through the official Rust MCP SDK. Scoped client keys can be revoked.
- **Notifications:** verified recipients, selected report times and a durable queue. Email uses a separately configured notification relay; no mail server is needed on the monitored host.

Additional monitoring includes certificate expiry, HTTP content and latency, scheduled-job heartbeat endpoints, local restart detection, and Linux pressure, swap, inode and disk I/O diagnostics. Unavailable readings are identified rather than treated as healthy. The preview remains a validation release; planned platforms and integrations are not represented as production-ready.

## Platform status

| Platform | Current status |
| --- | --- |
| Ubuntu 24.04 LTS, x86-64 | First packaged preview target; actual-server validation required before each release |
| Apple Silicon macOS | Development and local application validation; public installer package not yet provided |
| Debian 12, Linux ARM64 | Planned validation; not supported by the first downloadable binary |
| Windows Server, Kubernetes | Later work |

The Ubuntu binary is built on Ubuntu 24.04. Its release metadata records the exact source commit and required GLIBC symbol version. It is not advertised as a universal Linux binary.

## Install the private preview

An authorized operator with repository access runs this from their own computer after the reviewed private release is uploaded:

```sh
python3 scripts/install-private-release.py --host root@YOUR_SERVER_IP
```

The helper uses the operator's existing GitHub CLI authentication locally. It passes short-lived asset download URLs over SSH, downloads the assets directly from GitHub on the server, verifies SHA-256 and source provenance, and runs the installer. It does not store a GitHub token on the server. An existing SSH control socket can be supplied with `--control`; an alternate SSH port uses `--port`.

The installer prints the setup key, detected SSH host/user/port, a copyable SSH tunnel command and a browser address. Open the tunnel **on your own computer** and then visit `http://127.0.0.1:14877/admin`. Create your admin account, enter the new password twice, connect your authenticator, and save your recovery codes. Future sign-ins use your account and authenticator; they do not ask for the setup key again.

The monitoring service listens on the server's loopback address, port 4877. The browser tunnel uses local port 14877 to avoid colliding with an installation already running on your computer. Existing websites, certificates, firewall rules and ports 80/443 are left alone.

See the [installation guide](docs/INSTALLATION.md) for manual bundles, upgrades and removal.

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

The source remains proprietary and available for inspection under [LICENSE](LICENSE). A public GitHub repository would not, by itself, make this project open-source software. Third-party components retain their own licenses, included with every binary release.
