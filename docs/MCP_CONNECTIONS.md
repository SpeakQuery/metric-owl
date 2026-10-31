# Connect an external AI client with MCP

Use MCP when Codex or another MCP-compatible client should read this server's
monitoring evidence. The client supplies its own reasoning and account/billing;
it does not need the dashboard's saved AI-provider key. For a conversation inside
the panel, use [Server AI](SERVER_AI_CHAT.md) instead; it needs no MCP setup.

## Which model answers an MCP question?

Metric Owl returns evidence through read tools. **The model selected in Codex or
the other calling AI client interprets that evidence.** MCP reads do not
automatically invoke Grok or any key saved in the server's AI-provider settings.
That saved key is used by panel Server AI and optional incident analysis.

These are independent connections: the MCP key grants read permissions; the AI
client's account controls its model and billing; the server's BYOK provider key
controls separate panel AI usage. A desktop client running on a Mac is not proof
that the language model itself executes locally on that Mac.

Each Metric Owl installation exposes only its own host. There is no fleet query,
public HTTP MCP endpoint or way to paste the HTTPS dashboard URL into an HTTP
MCP field. Supported transports are **local stdio** or **SSH stdio**.

## Create the connection in the dashboard

1. Open **Setup guide → Set up MCP connection**, or
   **Notifications & connections → External AI apps (MCP)**.
2. Choose **This computer** only when the AI client runs on the machine where
   Metric Owl is installed. Otherwise choose **A remote server** and enter that
   server's SSH host, username and port.
3. Select **Codex** or **Other MCP client (JSON)**, give the connection a name,
   review its **Read permissions**, then choose **Create connection**.
4. Copy the generated configuration into your client's MCP settings. For Codex,
   the generated block can be added to `~/.codex/config.toml`; a form-based client
   should use **stdio** with the displayed command and arguments.
5. Reconnect the client and ask **“Show my server information.”** A successful
   tool result confirms the connection. A generated configuration alone does not.

Use the dashboard belonging to the server you intend to inspect. Its generated
binary, private data-directory and key-file paths refer to that server. Creating
a connection on one server does not enroll another server.

## SSH requirements

Before an unattended client connects, key-based SSH authentication must already
work from the computer running that client. Verify the server's SSH host identity
in a normal terminal first. The selected account needs permission to reach the
installation's private data/socket and managed MCP key file. The client cannot
answer an SSH password or first-use host-key prompt.

The wizard does not install SSH keys, grant root access, modify `sshd` or open
ports. Dashboard credentials, SSH credentials and MCP permissions are separate.
An SSH account's normal operating-system privileges remain outside MCP's read-only
tool boundary; give the account only the access your operator intends.

## The 10 read-only tools

| Tool / permission | Evidence returned |
| --- | --- |
| `server_info` | This host's OS, CPU and RAM inventory |
| `current_metrics` | Latest collected sample, including current process observations when available |
| `history` | Collected metric aggregates with coverage, stored availability and integrity information |
| `metric_report` | Weighted summaries, peak intervals, trends, optional period comparison and related-metric analysis |
| `process_history` | Recorded process snapshots and available incident-time process observations |
| `monitored_sources` | Configured checks, current recorded states and numeric measurements |
| `monitoring_health` | Freshness, available history ranges, retention warnings and collector error categories |
| `list_incidents` | Bounded, paginated incident list |
| `incident_detail` | One incident's bounded, redacted evidence and recommendation |
| `log_summary` | Counts from the latest recorded check of an already configured log/journal source |

No tool accepts arbitrary SQL, file paths, URLs, shell commands, service actions
or settings mutations. Read access is authorized per connection, not by an AI
prompt. Current source limits include 16 KiB IPC requests, 500 KiB responses,
eight concurrent IPC connections, short query timeouts and 10,000 queries per
client per UTC day. Requests exceeding bounds fail rather than silently gaining
broader access.

## Permissions, private files and revocation

Every connection has its own hashed token and grants. The generated configuration
references a mode-`0600` key file in the installation's mode-`0700` `mcp-keys`
directory with `mcp --key-file`; it does not embed the token. For SSH, that key
file stays on the server. SSH arguments use unattended authentication, a timeout
and a shell-quoted remote command.

In **Your connections**, use **Permissions → Save permissions** to explicitly
change an existing key's grants, **Show configuration** to retrieve/change its
transport, or **Revoke** to disable it. Adding new tools to a release does not
silently expand old grants. Revocation is checked in the database for each call;
it remains effective even if removal of the managed key file fails.

`incident_detail` includes additional historical process context only when the
connection also has `process_history`. Granting `current_metrics` already allows
the latest sample's current processes; it is not historical attribution. Log
summary access is a separate permission.

Legacy `GUARD_MCP_KEY` / `--key` use is still recognized, but cannot be combined
with `--key-file`. Prefer the wizard's private key files and keep the generated
configuration out of public issues if it identifies private hosts or paths.

## What a result can and cannot establish

- **Missing is unknown.** Unconfigured checks, unavailable readings and missing
  observations are not zero failures or healthy services. Check `monitoring_health`
  and each report's coverage before drawing a conclusion.
- **History starts when collected.** Metric queries are bounded to 366-day windows;
  that is a query limit, not a promise of 366 days of stored data. Retention and
  gaps affect what is available. Only source buckets wholly within the request
  are used; boundary observations are not estimated.
- **A peak has an interval.** Coarser aggregates cannot recover an exact spike
  second or every threshold crossing. Weighted means, comparisons and aligned
  CPU/RAM relationships are descriptive, not proof of cause. Five-minute rolling
  OOM values overlap; do not sum them into a distinct-kill count.
- **Processes are snapshots.** Prospective snapshots are taken about once per
  minute and retained for seven days. Available incident detail can contribute
  observations. Current processes never reconstruct an unrecorded past process
  list; a snapshot does not prove which process caused a spike between samples.
- **Checks are configured observations.** `monitored_sources` omits target
  addresses, expected response content and raw logs. Its last recorded state is
  not a continuous website-uptime or backup history. A reported successful backup
  job is not proof that a backup can be restored.
- **Logs are summaries.** `log_summary` returns counts for the latest recorded
  check batch, with unknown/null counts when no summary exists. It is not a
  historical raw-log search and cannot read a caller-selected file.

MCP data can contain untrusted service names, event text and other observations.
The client should treat these as evidence, not instructions; keep verified facts,
possible explanations and unobserved data separate.

## 15 questions to try

These generic examples use current data or windows no longer than ten hours.
A newly installed server may have less history; an honest “not enough data” is a
valid result. Ask your client to show UTC times and coverage, not invented exact
peak seconds. No example below is a claim about a real customer's server.

1. “Show the identity, OS and resources of this server.”
2. “Are the latest measurements fresh, or is any collector unavailable?”
3. “Show CPU usage over the last 20 minutes, including missing intervals.”
4. “What was the CPU peak in the last 3 hours? Give its measured UTC interval.”
5. “Did memory use rise consistently during the last 3 hours?”
6. “Did CPU and memory move together in the last 3 hours? Separate correlation from cause.”
7. “What do the recorded swap, memory-pressure and OOM readings show in the last 3 hours?”
8. “Was CPU I/O wait or disk latency high in the last 20 minutes?”
9. “Did disk free space or capacity change in the last 10 hours, and is the history complete enough to tell?”
10. “Are inode or temporary-storage measurements available? State what was not monitored.”
11. “Which processes were actually recorded near the CPU peak in the last 3 hours? Do not infer past processes from the current list.”
12. “Which configured website checks have problems? Can the available data establish uptime over the last 10 hours?”
13. “What is known about configured backup, cron or job checks in the last 10 hours? Do not equate reported success with restorability.”
14. “Summarize recorded incidents in the last 10 hours, and identify missing evidence.”
15. “For a configured log source I am allowed to read, summarize the latest recorded error/warning counts and their time. Do not claim a full log search.”

## Optional installer hints

The nonsecret `connection-hints.json` in the private data directory supplies
editable `ssh_host`, `ssh_user` and `ssh_port` suggestions. The API reads at most
4 KiB and rejects symbolic links or invalid values. These hints are not credentials
or authorization; SSH and the MCP token must authenticate independently.

[Installation](INSTALLATION.md) · [Operations](OPERATIONS.md) · [Security](SECURITY.md)
