(() => {
  const byId = id => document.getElementById(id);
  const host = byId('server-host'), user = byId('server-user'), port = byId('server-port');
  if (!host || !user || !port) return;
  const copyButtons = [...document.querySelectorAll('[data-copy]')];
  function validHost(value) {
    if (value.length > 253 || !value || /[^A-Za-z0-9.-]/.test(value)) return false;
    if (/^[0-9.]+$/.test(value)) return value.split('.').length === 4 && value.split('.').every(p => /^(0|[1-9][0-9]{0,2})$/.test(p) && Number(p) <= 255);
    return value.split('.').every(label => /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label));
  }
  function render() {
    const h = host.value.trim(), u = user.value.trim(), p = Number(port.value);
    const valid = validHost(h) && /^[A-Za-z_][A-Za-z0-9_-]{0,31}$/.test(u) && Number.isInteger(p) && p >= 1 && p <= 65535;
    byId('connection-error').textContent = valid ? 'Commands ready. Run them on the computer you will use to view your server.' : 'Enter a valid IPv4 address or hostname, an SSH username and a port from 1 to 65535. Do not include https:// or a password.';
    byId('ssh-login').textContent = valid ? `ssh -p ${p} ${u}@${h}` : 'Enter your server address above.';
    byId('ssh-tunnel').textContent = valid ? `ssh -p ${p} -N -o ExitOnForwardFailure=yes -L 127.0.0.1:14877:127.0.0.1:4877 ${u}@${h}` : 'Enter your server address above.';
    const id = byId('mcp-client-id').value.trim().toLowerCase();
    const mcpValid = valid && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id);
    if (mcpValid) {
      const args = ['-T', '-o', 'BatchMode=yes', '-p', String(p), `${u}@${h}`, `/usr/local/bin/server-guard --data-dir /var/lib/server-guard mcp --key-file /var/lib/server-guard/mcp-keys/${id}.key`];
      byId('mcp-config').textContent = byId('mcp-format').value === 'codex' ? `[mcp_servers.metric_owl]\ncommand = "ssh"\nargs = ${JSON.stringify(args, null, 2)}` : JSON.stringify({ mcpServers: { metric_owl: { command: 'ssh', args } } }, null, 2);
      byId('mcp-config-status').textContent = 'Ready for the standard Linux paths. SSH key sign-in and permission to read the private key file are required. Reconnect your AI client to verify access.';
    } else {
      byId('mcp-config').textContent = 'Enter your server address and an existing connection ID.';
      byId('mcp-config-status').textContent = 'No connection is created by this website. Create it in your dashboard first; do not paste any secret here.';
    }
    copyButtons.forEach(button => { button.disabled = button.dataset.copy === 'mcp-config' ? !mcpValid : !valid; button.textContent = button.dataset.copy === 'mcp-config' ? 'Copy configuration' : 'Copy command'; });
  }
  for (const field of [host, user, port, byId('mcp-client-id'), byId('mcp-format')]) field.addEventListener('input', render);
  for (const button of document.querySelectorAll('[data-copy],[data-copy-value]')) button.addEventListener('click', async () => {
    const value = button.dataset.copyValue || byId(button.dataset.copy).textContent;
    try { await navigator.clipboard.writeText(value); button.textContent = 'Copied'; }
    catch { button.textContent = 'Select text to copy'; }
  });
  render();
})();
