# Connect your AI client

Open the dashboard's **Setup guide → Set up MCP connection**, or **Notifications & connections → MCP clients**.

1. Choose **This computer** when the AI client runs on the same machine as this dashboard's agent. Choose **A remote server** when your AI client reaches this agent through SSH.
2. For SSH, enter this server's IP address or hostname, SSH username, and SSH port. Linux installations can prefill these from the installer's connection hints. These are SSH credentials, not dashboard credentials.
3. Select **Codex** or **Other MCP client (JSON)**, name the connection, and click **Create connection**. Read permissions can be reduced in the collapsed permissions section.
4. Copy the generated configuration into your AI client. For Codex, add the block to `~/.codex/config.toml`, or enter its command and arguments in the MCP settings. Reconnect the client and ask **“Show my server information.”**

The wizard prepares configuration; successful configuration generation is not a connection test. Your first successful tool result confirms that the AI client can reach the agent. The dashboard URL is not an HTTP MCP endpoint.

## SSH prerequisites

The AI client cannot type an SSH password. Key-based SSH access must already work from the computer running the AI client, and the server's identity must be verified in a terminal before the first unattended connection. The selected SSH account must be permitted to read the agent's private data directory. The wizard does not create SSH keys, grant access, modify `sshd`, or open ports.

Use the address of **the server running the dashboard**, because the generated binary, data-directory, and key-file paths belong to that server. To connect a different server, open that server's dashboard and create its connection there.

## Private keys and revocation

Each connection has an independent secret in a `0600` file inside the agent's `0700` `mcp-keys` directory. Generated configurations reference that file with `mcp --key-file`; they contain no raw secret. The file stays on the server for SSH connections. SSH arguments use unattended authentication, a connection timeout, and a shell-quoted remote command.

Use **Show configuration** to retrieve a current configuration or change its transport. Use **Revoke** to disable the key in SQLite and remove its managed file. Database revocation remains effective even if file cleanup fails. Older clients using `GUARD_MCP_KEY` remain supported; connections created before managed key files existed need a new wizard connection to use the ready configurations. `--key-file` and the legacy key/environment option are mutually exclusive.

The six available tools are `server_info`, `current_metrics`, `history`, `list_incidents`, `incident_detail`, and `log_summary`. They are scoped, bounded, and read-only. They cannot run commands, arbitrary SQL, service actions, or filesystem mutations. History only contains measurements that have actually been collected and retained.

## Installer hints

The optional, non-secret `connection-hints.json` in the agent's data directory contains `ssh_host`, `ssh_user`, and `ssh_port`. The API reads at most 4 KiB, rejects symbolic links or invalid values, and otherwise falls back to editable defaults. The wizard never uses hints as authorization; SSH and MCP credentials must still authenticate independently.
