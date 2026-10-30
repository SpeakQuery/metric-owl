#!/usr/bin/env bash
set -euo pipefail
# Installs an existing verified local release. Does not expose a network port.
bundle="${1:?Usage: install-macos.sh <release-directory>}"
bundle="$(cd "$bundle" && pwd)"
(cd "$bundle" && shasum -a 256 -c SHA256SUMS)
bin_dir="$HOME/Library/Application Support/SunucuBekcisi/bin"
data_dir="$HOME/Library/Application Support/SunucuBekcisi/data"
plist="$HOME/Library/LaunchAgents/com.sunucubekcisi.agent.plist"
mkdir -p "$bin_dir" "$data_dir" "$(dirname "$plist")"
chmod 700 "$data_dir"
launchctl bootout "gui/$(id -u)/com.sunucubekcisi.agent" 2>/dev/null || true
install -m 755 "$bundle/server-guard" "$bin_dir/server-guard.new"
mv "$bin_dir/server-guard.new" "$bin_dir/server-guard"
python3 - "$plist" "$bin_dir/server-guard" "$data_dir" <<'PY'
import plistlib,sys
path,binary,data=sys.argv[1:]
with open(path,'wb') as f:plistlib.dump({'Label':'com.sunucubekcisi.agent','ProgramArguments':[binary,'--data-dir',data,'serve'],'RunAtLoad':True,'KeepAlive':True,'ThrottleInterval':10,'ProcessType':'Background','StandardOutPath':'/dev/null','StandardErrorPath':'/dev/null'},f)
PY
launchctl bootstrap "gui/$(id -u)" "$plist"
service_pid=''
for ((attempt=0; attempt<20; attempt++)); do
  service_pid="$(launchctl print "gui/$(id -u)/com.sunucubekcisi.agent" | sed -n 's/^[[:space:]]*pid = \([0-9]*\)$/\1/p')"
  [[ -n "$service_pid" ]] && break
  sleep 0.5
done
bash "$(dirname "$0")/show-setup.sh" "$data_dir" local 4877 "$service_pid"
