# Metric Owl · Your AI Server Analyst

**BUILT IN RUST. ENGINEERED FOR LONG-RUNNING SERVERS.**

Turn server signals into a conversation you can understand.

<a href="https://metricowl.cloud/assets/demo/metric-owl-overview-v5.mp4"><img src="website/public/assets/demo/overview-autoplay-v5.gif" width="100%" alt="Animated 48-second Metric Owl preview: free monitoring, clear alerts and optional AI."></a>

Silent preview · click to watch with sound.

**[▶ Watch Metric Owl in 48 seconds — English narration](https://metricowl.cloud/assets/demo/metric-owl-overview-v5.mp4)**

Illustrative example data · Music: [“Tech Live” — Kevin MacLeod](https://incompetech.com/music/royalty-free/index.html?isrc=USUAN1700030), [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) · [Credits and edits](website/public/assets/demo/credits.txt).

Watch live performance, catch problems with local rules, and give your AI assistant
the evidence to explain what changed. Free, open-source server monitoring under
[Apache License 2.0](LICENSE), optional
bring-your-own-key **Server AI text chat**, incident explanations, and read-only MCP
for Codex and other clients. One installation observes one server.

[Website](https://metricowl.cloud/) · [Install](#install-on-your-server) · [Server AI](#talk-to-your-server) · [MCP](#ask-your-server-a-better-question) · [Operations](docs/OPERATIONS.md) · [Preview scope](docs/PREVIEW.md)

## BUILT IN RUST

**The monitoring backend, local API and notification relay are written in Rust.**
A monitor runs alongside your applications all day. We chose a systems language
that gives us control over resources and catches important mistakes early:

- **MEMORY SAFETY:** safe Rust checks ownership and borrowing at compile time,
  preventing classes of invalid memory access and use-after-free bugs.
- **NO GARBAGE COLLECTOR:** deterministic ownership handles resource lifetimes
  without a language-level garbage collector.
- **NATIVE PERFORMANCE:** compiled machine code, with a static musl Linux release;
  no Node.js VM or Rust build toolchain is needed to run the released agent.
- **SAFER CONCURRENCY:** ownership and type checks catch many unsafe ways of
  sharing data between concurrent workers before the program ships.
- **EXPLICIT ERROR HANDLING:** `Result` and `Option` make failures and unavailable
  values visible in the code, helping preserve the difference between missing data
  and a healthy zero.
- **STRONG DEVELOPMENT TOOLS:** Cargo tests, compiler diagnostics, rustfmt and
  strict Clippy checks support repeatable releases and clearer maintenance.

The browser interface uses React/TypeScript; installation and HTTPS helpers use
Bash/Python. Rust is our engineering foundation, with product tests and measured
acceptance completing the picture. [Why Rust and how we use it](docs/WHY_RUST.md).

## Install on your server

**Ubuntu 22.04 / 24.04 / 26.04 LTS or Debian 12 / 13, x86-64, with systemd.**
Public IPv4 is needed for the guided HTTPS option; no domain or Metric Owl cloud
account is needed. The script detects your OS locally. Acceptance depth differs
by platform; see [versions and validation](docs/PREVIEW.md). Run on your server:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.10/scripts/bootstrap-linux.sh | sudo bash
```

Already logged in as root on a minimal server without `sudo`? Use the same
command ending in `| bash`. If `curl` is missing, install it first with
`apt-get update` and `apt-get install -y curl ca-certificates` as root.

1. Choose your administrator username and enter your password twice. Select
   **HTTPS — open from any device (recommended)**, then review the detected IP
   and accept the Let’s Encrypt terms.
2. Open the printed **`https://YOUR_SERVER_IP:PORT/admin`** address and sign in.

The installer selects **8443**, or **9443 / 10443** if occupied, and prints the exact URL.
For HTTPS, port **80** (Metric Owl certificate validation) and the selected panel port must be reachable. [Network details](docs/OPERATIONS.md#network-access).

For private access, choose **Private — connect through SSH** and follow the printed
tunnel instructions. This skips certificate tools. HTTPS may install Certbot and
system Python/venv support packages, including dependency updates; usable dependencies
are reused. Two-factor authentication is optional.

[Inspect the installer](https://github.com/SpeakQuery/metric-owl/blob/v0.1.0-preview.10/scripts/bootstrap-linux.sh) · [Release assets and checksums](https://github.com/SpeakQuery/metric-owl/releases/tag/v0.1.0-preview.10) · [Installation help](docs/INSTALLATION.md) · [Verified preview.10 checks](docs/RELEASE_VALIDATION.md)

## A watchful companion for your server

- **See it live:** CPU, RAM, disks, network and processes in one dashboard.
- **Catch it early:** ready-made rules, time-window conditions and grouped incidents.
- **Understand the history:** compare measurements and inspect incident evidence.
- **Ask directly:** short Turkish or English answers grounded in this server's data.
- **Keep your choices:** use your own AI provider, SMTP account and MCP client.

![System architecture: the browser connects to Metric Owl on your server; local rules and SQLite history remain on that server.](website/public/assets/readme-system.svg)

Your application, account and monitoring history live on **your server**.
The public Metric Owl website provides product information; it is not a shared
monitoring service or customer login.

## Email alerts people can read

Warning, critical, recovery and scheduled-summary messages use a consistent
**HTML layout with a plain-text alternative**. They show the server alias,
recorded time, observed condition and suggested next checks. Direct SMTP can link
to the installation's configured HTTPS dashboard; relay messages omit an unpinned
dashboard link. Verification and connection-test messages use the same design.

Bring your own SMTP account or use the optional relay. Recipient verification,
quotas, deduplication and retries still apply. A connection test or provider
acceptance is separate from inbox delivery. [Templates and synthetic previews](docs/EMAIL_TEMPLATES.md).

## Talk to your server

Open **Server AI** in the dashboard. Connect your own **Grok, OpenAI, Claude,
Gemini, DeepSeek or Mistral** key once in **Notifications & connections → AI
providers**, then type a question:

> “What changed in the last 20 minutes?”
>
> “Son 3 saatte sunucu neden yavaşladı?”

Replies are short and follow the question's Turkish or English language. Time
ranges are understood from your text; dates are shown in UTC. No MCP setup,
model field, uploads or audio are required. Chat shares bounded monitoring
summaries with your chosen provider only when you send. The panel shows the
remaining **20 provider attempts per UTC day**; provider charges apply.

AI can explain collected observations, configured checks and recorded process
snapshots. Missing history stays unknown. A possible cause is not a proven cause,
and a current process list does not identify what caused yesterday's spike.
Chat has no command runner or server-changing tools. [Chat guide and limits](docs/SERVER_AI_CHAT.md)

### Which AI answers: my client or the server's provider?

| Where you ask | Who interprets the evidence | Credential and billing |
| --- | --- | --- |
| **Server AI** in the dashboard, or optional incident analysis | The BYOK provider connected on this server, such as Grok | Server-saved provider key; that provider's API charges |
| **Codex or another client through MCP** | The model selected in that AI client | Client's own account/model billing; separate scoped MCP key only authorizes data reads |

An MCP read does **not** automatically call the server's Grok key. Running a
desktop client on a Mac does not mean its language model runs locally on the Mac;
model execution follows that client's selected provider and configuration.

## From an alert to an explanation

Local rules handle routine detection without an AI request. When you need a deeper
explanation, optional AI analysis uses **your provider and API key**, with the
model selected automatically.
The result separates observed evidence, possible explanations and suggested checks.

![Incident analysis: local detection and bounded, sanitized evidence lead to optional provider analysis and an explanation. Critical alerts do not wait for AI.](website/public/assets/readme-ai-analysis.svg)

Manual event analysis has its own 10-request daily budget. Automatic explanations
are off by default and can be enabled separately for eligible important events;
routine CPU, memory and disk rules do not need AI. Chat never enables automatic
analysis. Provider limits and charges are separate; monitoring continues without
an AI connection. [Provider setup](docs/AI_CONNECTIONS.md)

## Ask your server a better question

Connect **Codex or another MCP-compatible assistant** from
**Setup guide → Set up MCP connection**. Then try:

> “Show CPU usage over the last 20 minutes, including missing measurements.”
>
> “When did CPU and memory rise together in the last 3 hours?”
>
> “Explain the evidence behind the latest disk alert.”

![MCP question and answer flow: your assistant requests a permitted tool, Metric Owl reads local evidence through scoped access, and your assistant interprets the returned context.](website/public/assets/readme-mcp.svg)

MCP provides **10 scoped, read-only tools**; your assistant does the reasoning.
Each client has its own permissions and revocable key. It reads only this
installation's host through local or SSH **stdio**, with no HTTP MCP endpoint.
History includes coverage and missing-data information. Process history uses
recorded snapshots, not reconstructed past activity. No tool runs commands,
arbitrary SQL, file requests or server changes. An external client's own AI usage
and fees are separate from the dashboard's provider connection.
[Connection guide and example questions](docs/MCP_CONNECTIONS.md)

## Know where every connection goes

Your browser opens the IP-based HTTPS address printed by your installer.
Optional AI and email connect to the providers **you configure**. MCP uses a
separate local or SSH stdio connection.

![Network map: HTTPS proxy to the loopback application, optional SSH MCP, configured outbound AI and SMTP, and TCP 80 for certificate validation.](website/public/assets/readme-network.svg)

<details>
<summary><strong>See the actual application screens</strong></summary>

Actual application interface with illustrative data, not access to a live server.

![Metric Owl dashboard with illustrative data](website/public/assets/dashboard.jpg)
![Ready-made alert rules](website/public/assets/rules.jpg)
![MCP connection guide](website/public/assets/mcp.jpg)
![Light dashboard](website/public/assets/dashboard-light.jpg)

</details>

<details>
<summary><strong>Technical boundaries, removal and further reading</strong></summary>

- Rust agent and API, embedded React dashboard, local SQLite history.
- Application listener: loopback only. Browser access: the installed HTTPS proxy.
- MCP: local or SSH stdio, bounded queries, per-client scopes and revocation.
- AI: optional, sanitized and size-bounded evidence, request budgets, no execution tools.
- Monitoring needs the host and agent online. Whole-host outage alerts need an independent monitor.
- Platform support and validation limits are listed in [Preview scope](docs/PREVIEW.md).
- macOS is a development/local-validation path, not a supported public production
  installer. Linux ARM64 and Windows packages are not published.
- Source features, a checksum-verified release, and a working installation on
  your server are separate evidence. A passing build or test does not prove
  that a release was published or that your panel is reachable.

**Uninstall:** `sudo bash /usr/share/doc/server-guard/uninstall-linux.sh`.
Your account and history are retained. Permanent removal and forgotten-password
recovery require explicit root-only operator steps; reinstalling is not a password
reset. [Recovery](docs/OPERATIONS.md#recover-administrator-access) ·
[Removal details](docs/OPERATIONS.md#remove-the-application)

[AI operator guide](docs/AI_OPERATOR_GUIDE.md) · [Advanced setup](docs/OPERATIONS.md) · [Email](docs/NOTIFICATIONS.md) · [Security](docs/SECURITY.md) · [Development](docs/DEVELOPMENT.md)

</details>

## Open source, on your terms

The free edition is licensed under [Apache License 2.0](LICENSE). Each installation
monitors its own host; you may run independent copies on multiple servers. There is
no license limit on server count. [Contributions are welcome](CONTRIBUTING.md).

Optional AI provider fees are paid by the user. Email provider charges may also apply.
Future Pro fleet management and team features are planned for a separate repository;
Pro is not available yet. Third-party components and media retain their own licenses.
