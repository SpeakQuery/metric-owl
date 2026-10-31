import { useEffect, useState } from 'react';
import { ArrowRight, Check, Copy, KeyRound, Laptop, PlugZap, RefreshCw, Server, ShieldCheck, Terminal, Trash2 } from 'lucide-react';
import { api } from './lib';
import './McpGuide.css';

type Options = { transport: 'local' | 'ssh'; host: string; user: string; port: number };
type Metadata = { command: string; args: string[]; tools: string[]; suggested_transport: 'local' | 'ssh'; ssh: { host: string; user: string; port: number } };
type Configuration = { server_name: string; transport: string; command: string; args: string[]; codex: string; custom: string; key_file: string; ssh_prerequisite: boolean; note: string };
type Client = { id: string; name: string; scopes: string[]; revoked: boolean };
const tools: Record<string, string> = { server_info: 'Server details', current_metrics: 'Current metrics', history: 'Performance history', metric_report: 'Period summaries & comparisons', process_history: 'Historical process snapshots', monitored_sources: 'Website, service & backup status', monitoring_health: 'Collection health & data coverage', list_incidents: 'Incident list', incident_detail: 'Incident evidence', log_summary: 'Sanitized log summaries' };

export default function McpGuide() {
  const [metadata, setMetadata] = useState<Metadata | null>(null);
  const [options, setOptions] = useState<Options>({ transport: 'local', host: '', user: 'root', port: 22 });
  const [format, setFormat] = useState<'codex' | 'custom'>('codex');
  const [name, setName] = useState('My server');
  const [scopes, setScopes] = useState(Object.keys(tools));
  const [clients, setClients] = useState<Client[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [configuration, setConfiguration] = useState<Configuration | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [permissionClient, setPermissionClient] = useState<Client | null>(null);
  const [permissionScopes, setPermissionScopes] = useState<string[]>([]);

  async function load() {
    setError('');
    try {
      const [connection, existing] = await Promise.all([api<Metadata>('mcp/connection'), api<Client[]>('mcp/clients')]);
      setMetadata(connection); setClients(existing);
      setOptions({ transport: connection.suggested_transport || 'local', host: connection.ssh?.host || '', user: connection.ssh?.user || 'root', port: connection.ssh?.port || 22 });
      if (connection.ssh?.host) setName(connection.ssh.host);
    } catch (e) { setError((e as Error).message); }
  }
  useEffect(() => { void load(); }, []);
  function fail(e: unknown) {
    setError((e as Error).message);
  }
  function change(next: Partial<Options>) { setOptions(previous => ({ ...previous, ...next })); setConfiguration(null); setCopied(false); setNotice(''); }
  function valid(existingId?: string) {
    if (options.transport === 'ssh' && (!options.host.trim() || !options.user.trim() || !Number.isInteger(options.port) || options.port < 1 || options.port > 65535)) {
      setError('Enter your SSH server address, username, and a port between 1 and 65535.'); return false;
    }
    if (!selected && !existingId && (!name.trim() || !scopes.length)) { setError('Give the connection a name and select at least one read permission.'); return false; }
    return true;
  }
  async function prepare(existingId?: string) {
    if (!valid(existingId)) return;
    setBusy(true); setError(''); setNotice(''); setCopied(false);
    const normalized = { ...options, host: options.host.trim(), user: options.user.trim() };
    try {
      const id = existingId || selected;
      if (id) {
        const result = await api<Configuration>(`mcp/clients/${id}/configuration`, 'POST', normalized);
        setConfiguration(result); setSelected(id);
      } else {
        const result = await api<{ id: string; configuration: Configuration }>('mcp/clients', 'POST', { name: name.trim(), scopes, connection: normalized });
        setConfiguration(result.configuration); setSelected(result.id);
      }
      setClients(await api<Client[]>('mcp/clients'));
      setNotice('Your configuration is ready. Copy it into your AI client to connect.');
    } catch (e) { fail(e); }
    finally { setBusy(false); }
  }
  async function revoke(client: Client) {
    setBusy(true); setError('');
    try {
      const result = await api<{ note?: string }>(`mcp/clients/${client.id}`, 'DELETE');
      if (selected === client.id) { setSelected(null); setConfiguration(null); setCopied(false); }
      setClients(await api<Client[]>('mcp/clients'));
      setNotice(result.note || 'Connection revoked.');
    } catch (e) { fail(e); }
    finally { setBusy(false); }
  }
  const text = configuration ? configuration[format] : '';
  async function savePermissions() {
    if (!permissionClient || !permissionScopes.length) return;
    setBusy(true); setError('');
    try {
      await api(`mcp/clients/${permissionClient.id}`, 'PUT', { scopes: permissionScopes });
      setClients(await api<Client[]>('mcp/clients')); setPermissionClient(null);
      setNotice('Read permissions saved. Reconnect your AI client to refresh its available tools.');
    } catch (e) { fail(e); }
    finally { setBusy(false); }
  }
  async function copy() {
    try { await navigator.clipboard.writeText(text); setCopied(true); setError(''); }
    catch { setError('Clipboard access is unavailable. Select the configuration below and copy it manually.'); }
  }
  const currentClient = clients.find(client => client.id === selected);
  return <div className="mcp-onboarding">
    <section className="panel mcp-wizard">
      <header className="mcp-wizard-heading"><div className="section-icon tone-blue"><PlugZap /></div><div><span className="eyebrow">YOUR SERVER. YOUR AI CLIENT.</span><h2>Connect an external AI app</h2><p>Use Codex or another desktop app to read your server through MCP. Provider API keys belong in the AI providers tab.</p></div><span className="badge good"><ShieldCheck size={13} /> Read-only</span></header>
      {error && <div className="notice error" role="alert">{error}</div>}
      {notice && <div className="notice success" role="status"><Check size={17} />{notice}</div>}
      {!metadata ? <div className="mcp-loading"><p>{error ? 'Connection details could not be loaded.' : 'Loading your connection details…'}</p>{error && <button className="secondary" onClick={load}><RefreshCw size={15} /> Try again</button>}</div> : <>
        <div className="mcp-step-title"><span>1</span><div><h3>Where is Metric Owl installed?</h3><p>Connect to the server running this dashboard, locally or through SSH.</p></div></div>
        <div className="mcp-location-options" role="group" aria-label="Metric Owl installation location">
          <button className={options.transport === 'local' ? 'selected' : ''} aria-pressed={options.transport === 'local'} onClick={() => change({ transport: 'local' })}><Laptop size={22} /><span><strong>This computer</strong><small>AI client and Metric Owl on the same machine</small></span>{options.transport === 'local' && <Check size={17} />}</button>
          <button className={options.transport === 'ssh' ? 'selected' : ''} aria-pressed={options.transport === 'ssh'} onClick={() => change({ transport: 'ssh' })}><Server size={22} /><span><strong>A remote server</strong><small>Connect to your server through SSH</small></span>{options.transport === 'ssh' && <Check size={17} />}</button>
        </div>
        {options.transport === 'ssh' && <div className="mcp-remote-settings"><div className="mcp-remote-fields"><label>Server IP or hostname<input aria-label="Server IP or hostname" aria-describedby="mcp-ssh-host-help" value={options.host} onChange={e => change({ host: e.target.value })} placeholder="203.0.113.10 or server.example.com" autoCapitalize="none" spellCheck={false} /><small id="mcp-ssh-host-help">The SSH address of the server running this dashboard.</small></label><label>SSH username<input aria-label="SSH username" aria-describedby="mcp-ssh-user-help" value={options.user} onChange={e => change({ user: e.target.value })} placeholder="root" autoCapitalize="none" spellCheck={false} /><small id="mcp-ssh-user-help">Your SSH account, not your dashboard username.</small></label><label>SSH port<input type="number" min="1" max="65535" value={options.port || ''} onChange={e => change({ port: Number(e.target.value) })} /></label></div><div className="mcp-prerequisite"><KeyRound size={17} /><div><strong>SSH key sign-in is required.</strong><p>Your AI client cannot answer an interactive SSH password prompt. This account must also be able to read Metric Owl’s private data directory.</p><details><summary>I normally enter an SSH password</summary><p>Set up key-based SSH sign-in from the computer running your AI client first, or ask your server administrator for help. Connect once in a terminal to verify the server’s identity. This wizard does not create SSH keys, grant root access, or change your SSH settings.</p></details></div></div></div>}
        {options.transport === 'local' && <p className="mcp-local-note">Use the same operating-system account that runs Metric Owl, or an account already permitted to read its private data directory.</p>}
        <div className="mcp-step-title"><span>2</span><div><h3>Choose your AI client</h3><p>Pick a format. The server connection and private key path are filled in automatically.</p></div></div>
        <div className="mcp-client-fields"><label>AI client<select value={format} onChange={e => { setFormat(e.target.value as 'codex' | 'custom'); setCopied(false); }}><option value="codex">Codex</option><option value="custom">Other MCP client (JSON)</option></select></label><label>Connection name<input value={currentClient?.name || name} onChange={e => setName(e.target.value)} disabled={!!selected} maxLength={100} placeholder="My production server" /></label></div>
        {!selected && <details className="mcp-advanced"><summary>Read permissions <span>{scopes.length} tools selected</span></summary><div className="mcp-scope-grid">{metadata.tools.map(scope => <label className="check-label" key={scope}><input type="checkbox" checked={scopes.includes(scope)} onChange={e => setScopes(previous => e.target.checked ? [...previous, scope] : previous.filter(value => value !== scope))} />{tools[scope] || scope}</label>)}</div><small>No tool can execute commands, edit files, run arbitrary SQL, or control services.</small></details>}
        {!configuration && <div className="mcp-prepare"><button className="primary" disabled={busy || (!selected && !scopes.length)} onClick={() => prepare()}>{busy ? 'Preparing…' : selected ? 'Update configuration' : 'Create connection'}<ArrowRight size={16} /></button><span>The secret key stays on your server.</span></div>}
        {configuration && <section className="mcp-ready" aria-label="Ready-to-use MCP configuration"><div className="mcp-ready-heading"><div><span className="eyebrow">CONFIGURATION READY</span><h3>Add this to {format === 'codex' ? 'Codex' : 'your MCP client'}</h3></div><button className="primary" onClick={copy}>{copied ? <Check size={16} /> : <Copy size={16} />}{copied ? 'Configuration copied' : 'Copy configuration'}</button></div><p>{format === 'codex' ? <>Paste this block into <code>~/.codex/config.toml</code>, or use these command and arguments in Codex’s MCP settings.</> : <>Paste this into your client’s MCP configuration. If it uses a form, choose <strong>stdio</strong> and enter the command and arguments shown below.</>}</p><pre className="code-block" tabIndex={0}>{text}</pre><div className="mcp-ready-next"><Terminal size={18} /><div><strong>Reconnect your AI client, then ask:</strong><p>“Show my server information.”</p><small>Configuration has been generated. The connection is only confirmed when your client successfully reads a result.</small></div></div><button className="text-button" onClick={() => { setSelected(null); setConfiguration(null); setCopied(false); setNotice(''); }}>Create another connection <ArrowRight size={14} /></button></section>}
      </>}
    </section>
    <section className="panel mcp-connections"><div className="panel-head"><div><h2>Your connections</h2><p>Each client has its own permissions and revocable key.</p></div><span className="tag">{clients.filter(client => !client.revoked).length} ACTIVE</span></div><div className="table-scroll"><table><thead><tr><th>Connection</th><th>Access</th><th>Status</th><th>Actions</th></tr></thead><tbody>{clients.map(client => <tr key={client.id}><td><strong>{client.name}</strong></td><td>{client.scopes.length} read tools</td><td><span className={'badge ' + (client.revoked ? 'neutral' : 'good')}>{client.revoked ? 'Revoked' : 'Active'}</span></td><td><div className="mcp-row-actions"><button className="secondary compact" disabled={busy || client.revoked} onClick={() => prepare(client.id)}>Show configuration</button><button className="secondary compact" disabled={busy || client.revoked} aria-label={`Edit permissions for ${client.name}`} onClick={() => { setPermissionClient(client); setPermissionScopes([...client.scopes]); }}>Permissions</button><button className="icon-button danger" disabled={busy || client.revoked} aria-label={`Revoke ${client.name}`} onClick={() => revoke(client)}><Trash2 size={16} /></button></div></td></tr>)}</tbody></table>{!clients.length && <p className="mcp-no-connections">No connections yet. Create your first one above.</p>}</div></section>
    {permissionClient && <section className="panel mcp-wizard" aria-label="Connection permissions"><h3>Read permissions · {permissionClient.name}</h3><p className="muted">Choose what this existing connection may read. Its private key and server connection stay the same.</p><div className="mcp-scope-grid">{(metadata?.tools || Object.keys(tools)).map(scope => <label className="check-label" key={scope}><input type="checkbox" checked={permissionScopes.includes(scope)} onChange={event => setPermissionScopes(previous => event.target.checked ? [...previous, scope] : previous.filter(value => value !== scope))}/>{tools[scope] || scope}</label>)}</div><div className="mcp-prepare"><button className="primary" disabled={busy || !permissionScopes.length} onClick={() => void savePermissions()}>Save permissions</button><button className="secondary" disabled={busy} onClick={() => setPermissionClient(null)}>Cancel</button></div></section>}
    <section className="panel mcp-questions"><span className="eyebrow">MORE THAN A HEALTH CHECK</span><h3>Try a real question.</h3><div><p>“Show a table of CPU usage over the last 24 hours.”</p><p>“Compare CPU and memory rises and falls.”</p><p>“Explain the most recent critical event.”</p></div><small>Only collected history is available. MCP uses local or SSH stdio; the dashboard URL is not an HTTP MCP endpoint.</small></section>
  </div>;
}
