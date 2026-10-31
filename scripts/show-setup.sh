#!/usr/bin/env bash
# Called by installers in the invoking terminal, never by the background service.
set -euo pipefail
set +x
data_dir="${1:?Usage: show-setup.sh <data-directory> [local|ssh] [port]}"
mode="${2:-ssh}"
port="${3:-4877}"
expected_pid="${4:?The service process ID is required}"
[[ "$expected_pid" =~ ^[0-9]+$ ]] && ((expected_pid > 0)) || exit 1
[[ "$port" =~ ^[0-9]+$ ]] && ((port > 0 && port < 65536)) || exit 1
status=''
for ((attempt=0; attempt<20; attempt++)); do
  status="$(curl --noproxy '*' --fail --silent --max-time 2 "http://127.0.0.1:$port/api/v1/auth/status" 2>/dev/null || true)"
  if [[ "$status" =~ \"process_id\"[[:space:]]*:[[:space:]]*${expected_pid}[,\}] ]] && [[ "$status" =~ \"configured\"[[:space:]]*:[[:space:]]*(true|false) ]]; then break; fi
  status=''
  sleep 0.5
done
if [[ ! "$status" =~ \"configured\"[[:space:]]*:[[:space:]]*(true|false) ]]; then
  printf 'Service startup could not be verified. Setup details were not displayed.\nCheck the Metric Owl service before continuing.\n' >&2
  exit 1
fi
configured="${BASH_REMATCH[1]}"
enrollment_ready=false
if [[ "$status" =~ \"enrollment_ready\"[[:space:]]*:[[:space:]]*true ]]; then enrollment_ready=true; fi
mfa_required=false
if [[ "$status" =~ \"mfa_required\"[[:space:]]*:[[:space:]]*true ]]; then mfa_required=true; fi
panel_origin="${GUARD_PUBLIC_ORIGIN:-}"
if [[ -z "$panel_origin" && "$mode" != local && -f /etc/metric-owl/panel-access.json ]]; then
  panel_origin="$(python3 - <<'PY'
import json,pathlib,re
p=pathlib.Path('/etc/metric-owl/panel-access.json')
if p.is_symlink() or p.stat().st_size>4096:raise SystemExit('Invalid HTTPS access marker.')
data=json.loads(p.read_text());value=data.get('public_origin','')
if data.get('managed') not in ('operator','installer'):raise SystemExit('Invalid HTTPS access marker.')
if not re.fullmatch(r'https://[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?(?::[1-9][0-9]{0,4})?',value):raise SystemExit('Invalid HTTPS panel address.')
print(value)
PY
)"
fi
if [[ -n "$panel_origin" && ! "$panel_origin" =~ ^https://[a-z0-9]([a-z0-9.-]*[a-z0-9])?(:[1-9][0-9]{0,4})?$ ]]; then
  printf 'Invalid HTTPS panel address; setup details were not displayed.\n' >&2; exit 1;
fi
setup_key=''
if [[ "$configured" == false && "$enrollment_ready" == false && "${GUARD_HIDE_SETUP_KEY:-0}" != 1 ]]; then
  [[ -f "$data_dir/setup-key" && ! -L "$data_dir/setup-key" && -r "$data_dir/setup-key" ]] || {
    printf 'The service is running, but its setup key could not be read.\n' >&2; exit 1;
  }
  setup_key="$(< "$data_dir/setup-key")"
  [[ "$setup_key" =~ ^[a-f0-9]{64}$ ]] || { printf 'Invalid setup key file.\n' >&2; exit 1; }
fi
printf '\nInstallation complete. Metric Owl is running.\n\n'
if [[ "$configured" == false && "$enrollment_ready" == true ]]; then
  printf 'Your administrator details are ready.\nSign in with the username and password you chose during installation.\nNo setup key is needed.\n\n'
elif [[ "$configured" == false && "${GUARD_HIDE_SETUP_KEY:-0}" != 1 ]]; then
  printf 'Setup key: %s\n\n' "$setup_key"
  printf 'Please copy this setup key and enter it during first-time setup.\nIt expires 30 minutes after generation and is removed after setup.\nKeep it private: it allows creation of the first administrator.\n\n'
elif [[ "$configured" == false ]]; then
  printf 'Initial setup is required. The key was not printed into package-manager logs.\n'
  printf 'After the package manager exits, run this in your SSH terminal:\n'
  printf '  sudo bash /usr/share/doc/server-guard/show-setup.sh /var/lib/server-guard ssh %s %s\n\n' "$port" "$expected_pid"
else
  printf 'Your administrator account is ready. Sign in with your username and password.\nNo setup key is needed. Your account and history have been preserved.\n\n'
fi
if [[ -n "$panel_origin" ]]; then
  printf 'Admin panel: %s/admin\n\n' "$panel_origin"
  printf 'Open this address from any computer or phone. No local installation or SSH tunnel is needed.\n'
elif [[ "$mode" == ssh ]]; then
  source "$(dirname "$0")/connection-hints.sh"
  guard_connection_values "$data_dir"
  ssh_host="${guard_ssh_host:-YOUR_SERVER_IP}"
  tunnel_port="${GUARD_LOCAL_PORT:-14877}"
  guard_valid_port "$tunnel_port" || { printf 'Invalid GUARD_LOCAL_PORT.\n' >&2; exit 1; }
  tunnel_port=$((10#$tunnel_port))
  printf 'Your server: %s\nSSH username: %s\nSSH port: %s\n\n' "$ssh_host" "$guard_ssh_user" "$guard_ssh_port"
  printf 'From your own computer, open an SSH tunnel and leave it running:\n'
  printf '  ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:%s:127.0.0.1:%s -p %s %s@%s\n\n' "$tunnel_port" "$port" "$guard_ssh_port" "$guard_ssh_user" "$ssh_host"
  printf 'Open in your browser: http://127.0.0.1:%s/admin\n' "$tunnel_port"
else
  printf 'Open in your browser: http://127.0.0.1:%s/admin\n' "$port"
fi
if [[ "$configured" == false && "$enrollment_ready" == true ]]; then
  printf 'Scan the QR code with your authenticator app, enter its verification code, and save your recovery codes.\n'
elif [[ "$configured" == false ]]; then
  printf 'Paste the setup key, choose a username, and enter your new password twice.\nScan the QR code with your authenticator app and save the recovery codes.\n'
fi
if [[ -n "$panel_origin" ]]; then
  if [[ "$mfa_required" == true ]]; then
    printf '\nYour existing two-factor protection remains enabled. Enter your authenticator code when asked.\n'
  elif [[ "$configured" == true ]]; then
    printf '\nYou can optionally enable two-factor authentication from Security & system.\n'
  fi
  printf 'HTTPS protects browser access. The monitoring service remains behind the HTTPS proxy.\n'
else
  printf '\nConnect your AI agent after sign-in:\n'
  printf '  Setup guide -> Set up MCP connection -> Create connection -> Copy configuration.\n'
  printf 'Remote MCP requires working SSH key authentication; your SSH password is not embedded in the configuration.\n'
  printf '\nExisting websites, domains, certificates, and ports 80/443 were not changed.\nThe dashboard accepts local connections only. The root URL does not display it.\n'
fi
