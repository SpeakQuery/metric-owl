#!/usr/bin/env bash
# Non-secret installer hints for the authenticated MCP connection wizard.
# This file can be sourced by show-setup.sh or invoked with: write <data-directory>.

guard_valid_host() {
  [[ ${#1} -ge 1 && ${#1} -le 253 && "$1" =~ ^[A-Za-z0-9:][A-Za-z0-9.:-]*$ ]]
}
guard_valid_user() { [[ "$1" =~ ^[A-Za-z_][A-Za-z0-9_.-]*$ ]]; }
guard_valid_port() { [[ "$1" =~ ^[0-9]{1,5}$ ]] && ((10#$1 > 0 && 10#$1 < 65536)); }

guard_read_hint() {
  local file="$1" key="$2"
  [[ -f "$file" && ! -L "$file" ]] || return 0
  [[ "$(wc -c < "$file")" -le 4096 ]] || return 0
  case "$key" in
    ssh_host|ssh_user) sed -n "s/^[[:space:]]*\"$key\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$file" | head -n 1;;
    ssh_port) sed -n 's/^[[:space:]]*"ssh_port"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p' "$file" | head -n 1;;
  esac
}

guard_connection_values() {
  local data_dir="$1" client_ip client_port server_ip server_port
  local file="$data_dir/connection-hints.json"
  guard_ssh_host="$(guard_read_hint "$file" ssh_host)"
  guard_ssh_user="$(guard_read_hint "$file" ssh_user)"
  guard_ssh_port="$(guard_read_hint "$file" ssh_port)"
  guard_valid_host "$guard_ssh_host" || guard_ssh_host=''
  guard_valid_user "$guard_ssh_user" || guard_ssh_user="${SUDO_USER:-${USER:-root}}"
  guard_valid_user "$guard_ssh_user" || guard_ssh_user=root
  guard_valid_port "$guard_ssh_port" || guard_ssh_port=22
  read -r client_ip client_port server_ip server_port <<< "${SSH_CONNECTION:-}"
  if guard_valid_host "${server_ip:-}"; then
    guard_ssh_host="$server_ip"
    guard_ssh_user="${SUDO_USER:-${USER:-root}}"
    guard_valid_user "$guard_ssh_user" || guard_ssh_user=root
    if guard_valid_port "${server_port:-}"; then guard_ssh_port=$((10#$server_port)); fi
  fi
  if [[ -n "${GUARD_SSH_HOST:-}" ]]; then
    guard_valid_host "$GUARD_SSH_HOST" || { printf 'Invalid GUARD_SSH_HOST.\n' >&2; return 1; }
    guard_ssh_host="$GUARD_SSH_HOST"
  fi
  if [[ -n "${GUARD_SSH_USER:-}" ]]; then
    guard_valid_user "$GUARD_SSH_USER" || { printf 'Invalid GUARD_SSH_USER.\n' >&2; return 1; }
    guard_ssh_user="$GUARD_SSH_USER"
  fi
  if [[ -n "${GUARD_SSH_PORT:-}" ]]; then
    guard_valid_port "$GUARD_SSH_PORT" || { printf 'Invalid GUARD_SSH_PORT.\n' >&2; return 1; }
    guard_ssh_port=$((10#$GUARD_SSH_PORT))
  fi
}

guard_write_connection_hints() {
  local data_dir="$1" temporary
  guard_connection_values "$data_dir"
  # Missing SSH context stays unknown; never invent a publicly reachable address.
  [[ -n "$guard_ssh_host" ]] || return 0
  temporary="$(mktemp "$data_dir/.connection-hints.XXXXXX")"
  chmod 600 "$temporary"
  printf '{\n  "ssh_host": "%s",\n  "ssh_user": "%s",\n  "ssh_port": %s\n}\n' "$guard_ssh_host" "$guard_ssh_user" "$guard_ssh_port" > "$temporary"
  chown server-guard:server-guard "$temporary"
  mv -f "$temporary" "$data_dir/connection-hints.json"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  set +x
  [[ "${1:-}" == write && $# == 2 ]] || { printf 'Usage: connection-hints.sh write <data-directory>\n' >&2; exit 1; }
  guard_write_connection_hints "$2"
fi
