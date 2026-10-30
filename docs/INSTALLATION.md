# Install Metric Owl

The current preview repository and release assets are private. There is no anonymous download command yet. The first packaged target is Ubuntu 24.04 LTS on x86-64; Debian 12 and ARM64 are not supported by this binary.

## Private GitHub installation

On your own computer, authenticate GitHub CLI with an account that can read the repository. Use an existing authorized SSH connection to the target server. From a reviewed checkout, run:

    python3 scripts/install-private-release.py --host root@YOUR_SERVER_IP

The default repository is SpeakQuery/metric-owl and version is v0.1.0-preview.1. Use --port for a different SSH port or --control for an existing SSH control socket. GitHub authentication stays on your computer. The server receives short-lived asset URLs over encrypted SSH input, downloads directly from GitHub, verifies checksums and source metadata, and installs the application. No GitHub token is written to the server.

## Open the control center

The installer prints the initial setup key, connection details and a complete tunnel command. Run the printed SSH command on your own computer, keeping that terminal open. A typical command is:

    ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:14877:127.0.0.1:4877 -p 22 root@YOUR_SERVER_IP

Then open http://127.0.0.1:14877/admin in your browser. The local port differs from the server port so a local Mac installation can remain running.

Create your administrator account using the setup key. Enter your new password twice, configure your authenticator and save the recovery codes. The initial key expires after 30 minutes. Once the account exists, the key is not shown or requested again; subsequent sign-ins use the account and second factor.

If an unused initial key expires, an authorized server administrator can run the application's reset-setup command as the service user, then use the installed show-setup helper. Reset-setup only works before an administrator exists; it is not a password-reset bypass.

The service binds to 127.0.0.1:4877. Installation does not change Nginx, existing domains, certificates, firewall rules, ports 80/443 or other services. The public marketing website is separate from the private control center.

## Connection hints and MCP

After login, open Setup guide, then Set up MCP connection. Create a scoped client key and follow the generated instructions. Remote unattended clients require SSH key authentication. Passwords are not embedded in client configurations.

The installer records only nonsecret SSH host, username and port suggestions in connection-hints.json within the private data directory. It detects the active SSH connection. If detection is unavailable, supply GUARD_SSH_HOST, GUARD_SSH_USER and GUARD_SSH_PORT to the installer. GUARD_LOCAL_PORT changes the local browser tunnel port. All suggestions remain editable in the panel.

## Manual release bundle

An authorized operator may download the versioned archive and SHA256SUMS from the private release page, verify the archive checksum, then unpack it on the supported server. From the unpacked bundle directory run:

    sudo bash scripts/install-linux.sh release

The installer verifies the inner binary checksum and checks that the candidate binary can start before replacing the existing executable. Checksums establish integrity relative to the trusted GitHub release; they are not a separate publisher signature.

## Upgrade and rollback

Re-run the installer with a reviewed newer release. It preserves /var/lib/server-guard, the administrator account, history, encrypted integration settings and systemd drop-ins. Do not delete the data directory when upgrading.

If activation fails, the installer attempts to restore the previous executable, main service unit and service state. It does not reverse database migrations; keep an operator-managed backup of the private data directory before major upgrades. Installation failure is reported rather than resetting credentials.

The executable and systemd unit retain the compatibility name server-guard. Official preview binaries may be used without an application license fee under BINARY-LICENSE.txt. Optional external service charges are separate.

## Remove the application

The supplied uninstall-linux.sh stops and removes the Metric Owl service and executable. Review the script before running it as root. It preserves the data directory by default. Removing private history is a separate, explicit operator action.
