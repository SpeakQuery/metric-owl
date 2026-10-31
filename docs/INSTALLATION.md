# Install Metric Owl

You need **Ubuntu or Debian x86-64 with a public IPv4 address** and SSH access to
the server. The installer accepts Ubuntu 22.04 / 24.04 / 26.04 LTS and Debian 12 / 13;
see the [validation matrix](PREVIEW.md) for the checks completed on each. A domain and a Metric Owl cloud account are not required.

## 1. Run the GitHub installer

Paste this into your **server’s terminal**:

```bash
curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.5/scripts/bootstrap-linux.sh | sudo bash
```

The versioned script and application package come from GitHub. It checks release
checksums, detects the OS and architecture locally, installs the monitoring service
and prepares HTTPS access. OS detection does not upload a machine inventory to GitHub.
[Inspect the script first](https://github.com/SpeakQuery/metric-owl/blob/v0.1.0-preview.5/scripts/bootstrap-linux.sh).

## 2. Choose your account

Follow the certificate prompts, choose your administrator username, and enter
your password twice. The installer prints the panel address when it finishes.
Existing installations keep their account, MFA settings and history.

## 3. Open your dashboard

Open the printed address, **`https://YOUR_SERVER_IP:PORT/admin`**, in any browser.
Sign in with the username and password you chose. No setup key, authenticator
app or software installation on your viewing device is required. You can enable
optional two-factor authentication later in **Security & system**.

## Where everything runs

Your application, web dashboard, administrator account, metrics and SQLite history
stay on **your server**. The public Metric Owl website provides product information
and documentation; it has no shared login or dashboard.
Optional AI, email, MCP and webhooks send selected data to the connections you enable.

## Connection requirements

TCP **80** for Metric Owl certificate validation and the chosen panel port must be reachable. The installer selects
**8443**, then **9443** or **10443** if occupied, and prints the exact address.
[Firewall and certificate details](OPERATIONS.md#network-access).

Existing hosted domains and their certificates are preserved. Conflicting port
or web-server configurations stop setup with an explanation rather than silently
replacing your sites. Manual/domain HTTPS, local-only access, SSH, upgrades and
removal are covered in [advanced operations](OPERATIONS.md).

Email is optional. Configure your own SMTP provider under **Email & reports**;
a central notification service is not required. [Email setup](NOTIFICATIONS.md)
explains verification and the difference between provider acceptance and inbox delivery.

## Uninstall

Run on your server: `sudo bash /usr/share/doc/server-guard/uninstall-linux.sh`

Your account and history are retained. [Removal and private backups](OPERATIONS.md#remove-the-application).

[Preview scope](PREVIEW.md) · [Security](SECURITY.md) · [Binary license](../BINARY-LICENSE.txt)
