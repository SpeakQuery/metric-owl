#!/usr/bin/env bash
set -euo pipefail
launchctl bootout "gui/$(id -u)/com.sunucubekcisi.agent" 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/com.sunucubekcisi.agent.plist"
rm -f "$HOME/Library/Application Support/SunucuBekcisi/bin/server-guard"
printf 'Service removed. History and keys preserved: ~/Library/Application Support/SunucuBekcisi/data\n'
