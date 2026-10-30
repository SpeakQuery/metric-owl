import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Activity, Check, Clock3, Copy, Database, FolderOpen, Globe2, HardDrive, Pause, Pencil, Play, Plus, RefreshCw, Server, ShieldCheck, Terminal, Trash2, X } from 'lucide-react';
import type { Sample } from './generated/Sample';
import type { Diagnostic } from './generated/Diagnostic';
import type { Target } from './generated/Target';
import type { TargetOptions } from './generated/TargetOptions';
import type { TargetStatus } from './generated/TargetStatus';
import { api, bytes, percent, time } from './lib';

type Site = { hostname: string; web_server: string; config_file: string; wildcard: boolean };
type Service = { name: string; kind: string; process_count: number; cpu_percent: number | null; memory_rss_bytes: number | null; memory_note: string; database_details: string | null };
type Inventory = { sites: Site[]; services: Service[]; site_notes: string[]; sites_checked_at: number | null; services_checked_at: number | null };
type Forecast = { source: string; days: number | null };
type Connection = { command: string; args: string[] };
type SourceKind = 'http' | 'tls' | 'job' | 'folder' | 'log' | 'service' | 'journal' | 'docker';
type SourceForm = {
  kind: SourceKind; name: string; address: string; expectedStatus: string; expectedText: string; maxLatency: string;
  inspectJson: boolean; jsonPointer: string; compareJson: boolean; jsonExpected: string;
  managedJob: boolean; jobInterval: string; jobGrace: string; jobRuntime: string;
};
const kinds: Record<SourceKind, { title: string; address: string; placeholder: string; help: string }> = {
  http: { title: 'HTTP health check', address: 'Health check URL', placeholder: 'https://app.example.com/health', help: 'Checked every 30 seconds. Redirects are not followed. The default alert requires three consecutive failed checks.' },
  tls: { title: 'TLS certificate', address: 'HTTPS URL', placeholder: 'https://app.example.com', help: 'Verified every 5 minutes. Rules at 30, 14 and 7 days, plus a verification-failure rule, are added automatically.' },
  job: { title: 'Scheduled job / backup', address: 'JSON report file', placeholder: '/absolute/path/job-report.json', help: 'Checks reports every 30 seconds. Metric Owl observes results; it never schedules or executes your job.' },
  folder: { title: 'Folder size', address: 'Folder path', placeholder: '/var/www/my-app', help: 'Scanned every 15 minutes with time and file-count limits. Symbolic links are not followed.' },
  log: { title: 'Text / JSON log file', address: 'Log file path', placeholder: '/var/log/my-app/application.log', help: 'Reads new entries every 5 seconds. The monitoring account must have permission to read the file.' },
  service: { title: 'Linux systemd service', address: 'Service unit name', placeholder: 'nginx.service', help: 'Checks state every 5 seconds. Two failed checks trigger the default rule; the service is never restarted.' },
  journal: { title: 'Linux service logs', address: 'Service unit name', placeholder: 'nginx.service', help: 'Reads recent service journal entries when the monitoring account has permission.' },
  docker: { title: 'Docker read-only helper', address: 'Helper socket path', placeholder: '/run/metric-owl/docker-helper.sock', help: 'Use the restricted Metric Owl helper, not the Docker administration socket. The helper must be installed separately.' },
};
export const blankSourceForm = (kind: SourceKind = 'http'): SourceForm => ({ kind, name: '', address: '', expectedStatus: '', expectedText: '', maxLatency: '', inspectJson: false, jsonPointer: '/status', compareJson: false, jsonExpected: '"ok"', managedJob: true, jobInterval: '1440', jobGrace: '60', jobRuntime: '60' });
const emptyOptions = (): TargetOptions => ({ expected_status: null, expected_text: null, json_pointer: null, json_expected: null, max_latency_ms: null, job_interval_seconds: null, job_grace_seconds: null, job_max_duration_seconds: null, job_added_at: null });
const byteLength = (value: string) => new TextEncoder().encode(value).length;
function numberField(value: string, name: string, minimum: number, maximum: number): number {
  const parsed = Number(value);
  if (!value.trim() || !Number.isInteger(parsed) || parsed < minimum || parsed > maximum) throw new Error(`${name} must be a whole number from ${minimum.toLocaleString('en-US')} to ${maximum.toLocaleString('en-US')}.`);
  return parsed;
}
// Validation only. Network requests begin after the user saves the source.
export function targetFromForm(form: SourceForm, id: string, enabled = true): Target {
  const name = form.name.trim();
  const address = form.kind === 'job' && form.managedJob ? '' : form.address.trim();
  if (!name || byteLength(name) >= 160) throw new Error('Give this monitor a name shorter than 160 bytes.');
  if (byteLength(address) >= 2048) throw new Error('The source address is too long.');
  if (['http', 'tls'].includes(form.kind)) {
    let url: URL;
    try { url = new URL(address); } catch { throw new Error('Enter a complete URL, including https:// or http://.'); }
    if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || address.includes('#')) throw new Error('Use an HTTP or HTTPS URL without embedded credentials or a fragment.');
    if (form.kind === 'tls' && url.protocol !== 'https:') throw new Error('Certificate monitoring requires an HTTPS URL.');
  } else if (['service', 'journal'].includes(form.kind)) {
    if (!/^[A-Za-z0-9_.@][A-Za-z0-9_.@-]*$/.test(address) || address.length >= 200) throw new Error('Enter a service unit name, such as nginx.service.');
  } else if (!(form.kind === 'job' && form.managedJob) && (!address.startsWith('/') || address.includes('\0'))) throw new Error('Enter an absolute file or socket path.');
  const target: Target = { id, name, address, kind: form.kind, enabled };
  if (form.kind === 'http') {
    const options = emptyOptions();
    if (form.expectedStatus.trim()) options.expected_status = numberField(form.expectedStatus, 'HTTP status', 100, 599);
    if (form.maxLatency.trim()) options.max_latency_ms = numberField(form.maxLatency, 'Response time', 50, 8000);
    if (form.expectedText) {
      if (byteLength(form.expectedText) > 1024) throw new Error('Expected text must fit within 1,024 bytes.');
      options.expected_text = form.expectedText;
    }
    if (form.inspectJson) {
      if (byteLength(form.jsonPointer) > 256 || (form.jsonPointer && !form.jsonPointer.startsWith('/'))) throw new Error('Use a JSON pointer such as /status, or leave it empty for the whole document.');
      options.json_pointer = form.jsonPointer;
      if (form.compareJson) {
        if (byteLength(form.jsonExpected) > 1024) throw new Error('The expected JSON value must fit within 1,024 bytes.');
        try { JSON.parse(form.jsonExpected); } catch { throw new Error('Enter a valid JSON value: for example "ok", true, 42, or {"ready":true}.'); }
        options.json_expected = form.jsonExpected;
      }
    }
    target.options = options;
  } else if (form.kind === 'job') target.options = { ...emptyOptions(), job_interval_seconds: numberField(form.jobInterval, 'Expected interval in minutes', 1, 44640) * 60, job_grace_seconds: numberField(form.jobGrace, 'Grace period in minutes', 0, 1440) * 60, job_max_duration_seconds: numberField(form.jobRuntime, 'Maximum runtime in minutes', 1, 10080) * 60 };
  return target;
}
function managedJobPath(connection: Connection | null, id: string): string | null {
  const index = connection?.args.indexOf('--data-dir') ?? -1;
  const directory = index >= 0 ? connection?.args[index + 1] : null;
  return directory?.startsWith('/') ? `${directory.replace(/\/+$/, '')}/job-reports/${id}.json` : null;
}
export function jobReportCommand(connection: Connection | null, id: string, state: string, linux: boolean): string | null {
  const index = connection?.args.indexOf('--data-dir') ?? -1;
  const directory = index >= 0 ? connection?.args[index + 1] : null;
  if (!connection?.command.startsWith('/') || !directory?.startsWith('/') || !/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(id) || !['running', 'success', 'failure'].includes(state)) return null;
  const quote = (value: string) => `'${value.replace(/'/g, "'\\''")}'`;
  const command = [connection.command, '--data-dir', directory, 'report-job', '--id', id, '--state', state].map(quote).join(' ');
  return linux ? `runuser -u server-guard -- ${command}` : command;
}
function formFromTarget(target: Target, connection: Connection | null): SourceForm {
  const options = target.options;
  return { ...blankSourceForm(target.kind as SourceKind), name: target.name, address: target.address,
    expectedStatus: options?.expected_status?.toString() ?? '', expectedText: options?.expected_text ?? '', maxLatency: options?.max_latency_ms?.toString() ?? '',
    inspectJson: options?.json_pointer != null, jsonPointer: options?.json_pointer ?? '/status', compareJson: options?.json_expected != null, jsonExpected: options?.json_expected ?? '"ok"',
    managedJob: target.kind === 'job' && target.address === managedJobPath(connection, target.id),
    jobInterval: ((options?.job_interval_seconds ?? 86400) / 60).toString(), jobGrace: ((options?.job_grace_seconds ?? 3600) / 60).toString(), jobRuntime: ((options?.job_max_duration_seconds ?? 3600) / 60).toString(),
  };
}
const statusNames: Record<string, string> = { ok: 'Healthy', error: 'Issue detected', critical: 'Critical', warning: 'Warning', unavailable: 'Unavailable', partial: 'Partial data', pending: 'Waiting for report', running: 'Job running' };
const statusTone: Record<string, string> = { ok: 'good', error: 'critical', critical: 'critical', warning: 'warning', unavailable: 'neutral', partial: 'warning', pending: 'neutral', running: 'source-running' };

export default function Sources({ sample }: { sample: Sample | null }) {
  const [targets, setTargets] = useState<Target[]>([]);
  const [statuses, setStatuses] = useState<TargetStatus[]>([]);
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [inventoryError, setInventoryError] = useState('');
  const [forecasts, setForecasts] = useState<Forecast[]>([]);
  const [forecastError, setForecastError] = useState('');
  const [connection, setConnection] = useState<Connection | null>(null);
  const [linux, setLinux] = useState<boolean | null>(null);
  const [connectionError, setConnectionError] = useState('');
  const [form, setForm] = useState<SourceForm>(blankSourceForm);
  const [editing, setEditing] = useState<string | null>(null);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadedTargets, setLoadedTargets] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [showAuth, setShowAuth] = useState(false);
  const [authBusy, setAuthBusy] = useState(false);
  const [credentials, setCredentials] = useState({ username: 'admin', password: '', code: '' });
  const formPanel = useRef<HTMLElement>(null);
  const mutation = useRef(false);
  const generation = useRef(0);

  async function refresh() {
    if (mutation.current) return;
    const currentGeneration = ++generation.current;
    setRefreshing(true);
    const results = await Promise.allSettled([api<{ targets: Target[]; status: TargetStatus[] }>('targets'), api<Inventory>('inventory'), api<Forecast[]>('forecasts')]);
    if (currentGeneration !== generation.current) return;
    const [sources, discovered, forecast] = results;
    if (sources.status === 'fulfilled') { setTargets(sources.value.targets); setStatuses(sources.value.status); setLoadedTargets(true); } else setError(`Could not refresh monitors: ${sources.reason.message}`);
    if (discovered.status === 'fulfilled') { setInventory(discovered.value); setInventoryError(''); } else setInventoryError('Discovery is unavailable. Existing monitors continue independently.');
    if (forecast.status === 'fulfilled') { setForecasts(forecast.value); setForecastError(''); } else setForecastError('Disk forecasts could not be refreshed.');
    setLoading(false); setRefreshing(false);
  }
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 15000);
    Promise.all([api<Connection>('mcp/connection'), api<{ os: string }>('host')]).then(([details, host]) => { setConnection(details); setLinux(/linux|ubuntu|debian/i.test(host.os)); }).catch(() => setConnectionError('Installation details are unavailable. Refresh this page before copying job report commands.'));
    return () => { clearInterval(timer); generation.current++; };
  }, []);

  async function save(next: Target[], success: string): Promise<boolean> {
    if (mutation.current || !loadedTargets) return false;
    mutation.current = true; generation.current++; setBusy(true); setError(''); setMessage('');
    try {
      const saved = await api<{ ok: boolean; targets: Target[] }>('targets', 'PUT', next);
      if (!Array.isArray(saved.targets)) throw new Error('The server did not return the saved sources. Refresh to check the result.');
      setTargets(saved.targets);
      setStatuses(current => current.filter(status => {
        const previous = targets.find(t => t.id === status.id);
        const target = saved.targets.find(t => t.id === status.id);
        return target?.enabled && JSON.stringify(previous) === JSON.stringify(target);
      }));
      setMessage(success); return true;
    } catch (cause) { const message = (cause as Error).message; setError(message); if (/re-authenticat|verify your identity/i.test(message)) setShowAuth(true); return false; }
    finally { mutation.current = false; setBusy(false); setRefreshing(false); }
  }
  function focusForm(next: SourceForm, id: string | null = null) {
    setForm(next); setEditing(id); setMessage(''); setError('');
    formPanel.current?.scrollIntoView({ behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth', block: 'start' });
  }
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setError('');
    try {
      if (!editing && targets.length >= 30) throw new Error('This installation supports up to 30 additional sources. Remove a source before adding another.');
      const existing = targets.find(t => t.id === editing);
      const target = targetFromForm(form, editing || crypto.randomUUID(), existing?.enabled ?? true);
      const next = editing ? targets.map(t => t.id === editing ? target : t) : [...targets, target];
      if (await save(next, editing ? 'Monitor updated. Its next check will use these settings.' : 'Monitor added. The first automatic check will appear shortly.')) { setForm(blankSourceForm(form.kind)); setEditing(null); }
    } catch (cause) { setError((cause as Error).message); }
  }
  const field = <K extends keyof SourceForm>(key: K, value: SourceForm[K]) => setForm(current => ({ ...current, [key]: value }));
  const kind = kinds[form.kind];
  const activeCount = targets.filter(t => t.enabled).length;
  return <div className="sources-page">
    {error && <div role="alert" className="notice error"><span>{error}{/re-authenticat|verify your identity/i.test(error) && ' Verify your identity below, then retry. Your form has been kept.'}</span><button className="icon-button" aria-label="Dismiss source error" onClick={() => setError('')}><X size={15} /></button></div>}
    {message && <div role="status" className="notice success"><Check size={17} />{message}</div>}
    {showAuth && <section className="panel form-panel source-reauth"><h2>Verify your identity</h2><p>Your monitor settings stay in the form. Verification unlocks sensitive changes for 5 minutes.</p><form onSubmit={async event => { event.preventDefault(); setAuthBusy(true); setError(''); try { await api('auth/reauth', 'POST', credentials); setCredentials({ username: 'admin', password: '', code: '' }); setShowAuth(false); setMessage('Identity verified. You can retry your monitor change.'); } catch (cause) { setError((cause as Error).message); setCredentials(current => ({ ...current, password: '', code: '' })); } finally { setAuthBusy(false); } }}><fieldset disabled={authBusy}><div className="three-col"><label>Administrator username<input autoComplete="username" required value={credentials.username} onChange={event => setCredentials({ ...credentials, username: event.target.value })} /></label><label>Current password<input type="password" autoComplete="current-password" required value={credentials.password} onChange={event => setCredentials({ ...credentials, password: event.target.value })} /></label><label>New verification code<input autoComplete="one-time-code" required value={credentials.code} onChange={event => setCredentials({ ...credentials, code: event.target.value })} /></label></div><button className="primary">{authBusy ? 'Verifying…' : 'Verify identity'}</button></fieldset></form></section>}
    <div className="source-overview"><div><Globe2 /><strong>{inventory ? inventory.sites.length : '—'}</strong><span>domain declarations</span></div><div><Server /><strong>{inventory ? inventory.services.length : '—'}</strong><span>observed service groups</span></div><div><ShieldCheck /><strong>{loading ? '—' : activeCount}</strong><span>enabled monitors</span></div><button className="secondary compact" disabled={refreshing || busy} onClick={() => void refresh()}><RefreshCw size={14} />{refreshing ? 'Refreshing…' : 'Refresh results'}</button></div>

    <section className="panel"><div className="panel-head"><h2>Disks & available space</h2><span className="tag">EVERY 15 SECONDS</span></div><div className="disk-grid">{sample?.disks.length ? sample.disks.map(disk => {
      const forecast = forecasts.find(f => f.source === disk.id); const used = disk.used_percent.value;
      return <div className="disk-card" key={disk.id}><HardDrive /><div><strong>{disk.mount}</strong><p>{bytes(disk.available)} free / {bytes(disk.total)}</p>{used == null ? <div className="source-unknown-meter" aria-label="Disk measurement unavailable" /> : <div className="meter"><i style={{ width: `${Math.max(0, Math.min(100, used))}%`, background: used >= 85 ? 'var(--amber)' : 'var(--teal)' }} /></div>}<small>{percent(used)} used · {disk.used_percent.reason || (used == null ? 'Measurement unavailable' : 'Measurement available')}</small><small>{forecastError || (forecast?.days != null ? `Estimated full in ${forecast.days.toFixed(0)} days` : 'Full-disk forecast: waiting for enough history')}</small></div></div>;
    }) : <p className="source-empty">Waiting for disk measurements. Unavailable readings are not treated as empty disks.</p>}</div><div className="chart-footer">A forecast requires at least 7 days of history, sufficient coverage and consistent growth.</div></section>

    <section className="panel source-diagnostics"><div className="panel-head"><div><div className="source-title"><Activity /><h2>Bottleneck diagnostics</h2></div><p>Memory stalls, I/O waiting and storage limits provide context beyond CPU and RAM totals.</p></div><span className="tag">EVERY 5 SECONDS</span></div>{sample?.diagnostics?.length ? <><div className="source-diagnostic-grid">{sample.diagnostics.filter(diagnostic => diagnostic.source === 'host').map(diagnostic => <DiagnosticCard key={`${diagnostic.source}:${diagnostic.metric}`} diagnostic={diagnostic} />)}</div>{sample.diagnostics.some(diagnostic => diagnostic.source !== 'host') && <details className="source-storage-diagnostics"><summary>Device & filesystem diagnostics ({sample.diagnostics.filter(diagnostic => diagnostic.source !== 'host').length})</summary><div className="source-diagnostic-grid">{sample.diagnostics.filter(diagnostic => diagnostic.source !== 'host').map(diagnostic => <DiagnosticCard key={`${diagnostic.source}:${diagnostic.metric}`} diagnostic={diagnostic} />)}</div></details>}</> : <p className="source-empty">Waiting for diagnostic measurements.</p>}<div className="chart-footer">Linux exposes PSI, OOM, I/O and inode counters. Unsupported or stale readings remain unavailable. Busy time alone does not prove disk saturation.</div></section>

    <section className="panel source-inventory"><div className="panel-head"><div><div className="source-title"><Globe2 /><h2>Hosted websites</h2><span className="badge neutral">Discovery</span></div><p>Readable Nginx and Apache declarations. Review a hostname before enabling checks.</p></div><small>Last scan: {time(inventory?.sites_checked_at)}</small></div>
      <div className="source-discovery-note"><ShieldCheck size={16} /><p>Discovery does not prove a website is enabled, reachable or owned by this server. No HTTP request is sent until you add a monitor. Existing web configuration is never changed.</p></div>
      {inventoryError && <div className="notice warning">{inventoryError}</div>}
      {inventory?.sites.length ? <div className="table-scroll"><table className="site-table"><thead><tr><th>Domain / subdomain</th><th>Declared by</th><th>Monitoring</th></tr></thead><tbody>{inventory.sites.map(site => {
        const existing = targets.filter(t => ['http', 'tls'].includes(t.kind) && (() => { try { return new URL(t.address).hostname === site.hostname; } catch { return false; } })());
        return <tr key={`${site.web_server}:${site.hostname}`}><td><strong>{site.hostname}</strong>{site.wildcard && <small>Wildcard: choose a concrete hostname in the form below.</small>}</td><td>{site.web_server}<details className="source-config-details"><summary>Configuration path</summary><code>{site.config_file}</code></details></td><td><div className="source-site-actions"><button className="secondary compact" disabled={site.wildcard || busy} onClick={() => focusForm({ ...blankSourceForm('http'), name: `${site.hostname} health`, address: `https://${site.hostname}/` })}><Plus size={13} />Configure HTTP</button><button className="secondary compact" disabled={site.wildcard || busy} onClick={() => focusForm({ ...blankSourceForm('tls'), name: `${site.hostname} certificate`, address: `https://${site.hostname}/` })}><ShieldCheck size={13} />Configure TLS</button></div>{existing.length > 0 && <small>{existing.length} configured {existing.length === 1 ? 'monitor' : 'monitors'} · {existing.filter(t => t.enabled).length} enabled</small>}</td></tr>;
      })}</tbody></table></div> : <div className="source-empty"><Globe2 size={25} /><strong>{inventory ? 'No readable website declarations found' : 'Waiting for discovery'}</strong><p>{inventory ? 'You can still add a URL manually. Discovery depends on the web server and the monitoring account’s read permissions.' : 'Known configuration directories are scanned locally.'}</p></div>}
      {!!inventory?.site_notes.length && <details className="source-scan-notes"><summary>Discovery notes ({inventory.site_notes.length})</summary><ul>{inventory.site_notes.map((note, i) => <li key={i}>{note}</li>)}</ul></details>}
    </section>

    <section className="panel source-inventory"><div className="panel-head"><div><div className="source-title"><Server /><h2>Services & database engines</h2></div><p>Groups detected from running processes. This is a resource snapshot, not a service health test.</p></div><small>Last snapshot: {time(inventory?.services_checked_at)}</small></div>
      {inventory?.services.length ? <div className="source-service-grid">{inventory.services.map(service => <article className={'source-service-card ' + (service.kind === 'database' ? 'source-database' : '')} key={`${service.kind}:${service.name}`}><div className="source-service-heading">{service.kind === 'database' ? <Database size={20} /> : <Server size={20} />}<div><h3>{service.name}</h3><small>{service.process_count} {service.process_count === 1 ? 'process' : 'processes'} detected</small></div><span className="tag">{service.kind}</span></div><dl className="source-service-metrics"><div><dt>CPU · per-core basis</dt><dd>{percent(service.cpu_percent)}</dd></div><div><dt>Memory · process RSS</dt><dd>{bytes(service.memory_rss_bytes)}</dd></div></dl><small>{service.memory_note}</small>{service.database_details && <p className="source-database-note"><Database size={14} /><span>{service.database_details}</span></p>}</article>)}</div> : <div className="source-empty"><Server size={25} /><strong>{inventory ? 'No known service groups observed' : 'Waiting for process inventory'}</strong><p>Only recognized, readable processes are listed. Add a systemd service monitor for explicit running-state checks.</p></div>}
    </section>

    <div className="source-monitor-layout"><section ref={formPanel} className="panel form-panel source-form-panel" aria-labelledby="source-form-title"><div className="section-icon tone-ai"><FolderOpen /></div><h2 id="source-form-title">{editing ? 'Edit your monitor' : 'Add a source to monitor'}</h2><p className="muted">Choose what to observe and what a healthy result looks like.</p><form onSubmit={submit}>
      <fieldset disabled={busy}><label>Source type<select value={form.kind} disabled={!!editing} onChange={event => setForm(blankSourceForm(event.target.value as SourceKind))}>{Object.entries(kinds).map(([value, config]) => <option key={value} value={value}>{config.title}</option>)}</select></label>
        <label>Display name<input required maxLength={159} value={form.name} onChange={event => field('name', event.target.value)} placeholder="Production website" /></label>
        {form.kind === 'job' && <label className="source-managed-choice"><input type="checkbox" checked={form.managedJob} onChange={event => field('managedJob', event.target.checked)} /><span>Use a managed job report<small>Recommended. Metric Owl chooses a private report path and provides copyable reporting commands.</small></span></label>}
        {!(form.kind === 'job' && form.managedJob) && <label>{kind.address}<input required value={form.address} maxLength={2047} onChange={event => field('address', event.target.value)} placeholder={kind.placeholder} spellCheck={false} autoCapitalize="none" /></label>}
        {form.kind === 'http' && <div className="source-options"><div className="source-field-grid"><label>Expected HTTP status<input inputMode="numeric" type="number" min="100" max="599" step="1" value={form.expectedStatus} onChange={event => field('expectedStatus', event.target.value)} placeholder="Any 2xx response" /><small>Leave empty to accept any successful 2xx status.</small></label><label>Maximum response time (ms)<input inputMode="numeric" type="number" min="50" max="8000" step="1" value={form.maxLatency} onChange={event => field('maxLatency', event.target.value)} placeholder="Optional · 50–8,000" /><small>Includes reading the body when content checks are enabled.</small></label></div><label>Required response text<input value={form.expectedText} onChange={event => field('expectedText', event.target.value)} placeholder="Optional · e.g. service ready" /><small>Case-sensitive text match. Response inspection is limited to 64 KiB.</small></label><label className="check-label"><input type="checkbox" checked={form.inspectJson} onChange={event => field('inspectJson', event.target.checked)} />Inspect a JSON response</label>{form.inspectJson && <div className="source-json-options"><label>JSON pointer<input value={form.jsonPointer} onChange={event => field('jsonPointer', event.target.value)} placeholder="/status" spellCheck={false} /><small>Use /status or /checks/database. Empty selects the entire JSON document.</small></label><label className="check-label"><input type="checkbox" checked={form.compareJson} onChange={event => field('compareJson', event.target.checked)} />Require an exact JSON value</label>{form.compareJson && <label>Expected JSON value<input value={form.jsonExpected} onChange={event => field('jsonExpected', event.target.value)} spellCheck={false} /><small>Strings need quotes: "ok". Numbers, true, false, null, objects and arrays are also accepted.</small></label>}</div>}</div>}
        {form.kind === 'tls' && <div className="source-tls-presets"><ShieldCheck size={18} /><div><strong>Expiry alerts are included</strong><div><span>30 days</span><span>14 days</span><span>7 days · critical</span></div><small>Certificate and hostname verification stay enabled. An unreadable certificate is not assigned an invented expiry date.</small></div></div>}
        {form.kind === 'job' && <div className="source-options"><div className="source-job-presets"><span>Expected schedule</span>{[['Hourly', '60'], ['Daily', '1440'], ['Weekly', '10080']].map(([title, minutes]) => <button className={'secondary compact ' + (form.jobInterval === minutes ? 'source-preset-selected' : '')} type="button" key={title} onClick={() => field('jobInterval', minutes)}>{title}</button>)}</div><label>Expected interval (minutes)<input required inputMode="numeric" type="number" min="1" max="44640" step="1" value={form.jobInterval} onChange={event => field('jobInterval', event.target.value)} /><small>Maximum time between successful reports. A daily job uses 1,440 minutes.</small></label><div className="source-field-grid"><label>Grace period (minutes)<input required inputMode="numeric" type="number" min="0" max="1440" step="1" value={form.jobGrace} onChange={event => field('jobGrace', event.target.value)} /><small>Extra time before a successful report is considered overdue.</small></label><label>Maximum runtime (minutes)<input required inputMode="numeric" type="number" min="1" max="10080" step="1" value={form.jobRuntime} onChange={event => field('jobRuntime', event.target.value)} /><small>Applies after your job reports that it is running.</small></label></div></div>}
        <div className="source-check-note"><Clock3 size={15} /><span>{kind.help}</span></div><div className="button-row"><button className="primary" disabled={busy || !loadedTargets || (!editing && targets.length >= 30)}><Plus size={16} />{busy ? 'Saving…' : editing ? 'Save monitor' : 'Add to monitoring'}</button>{editing && <button className="secondary" type="button" onClick={() => { setEditing(null); setForm(blankSourceForm(form.kind)); }}>Cancel edit</button>}</div>
      </fieldset></form><small>Changes require recent identity verification. If prompted, verify on this page; your form will stay filled in.</small></section>

      <section className="panel source-selected" aria-label="Selected sources"><div className="panel-head"><div><h2>Selected sources</h2><p>{activeCount} enabled · {targets.length} of 30 available</p></div><span className="badge neutral">Read-only</span></div>{!targets.length && <div className="source-empty"><ShieldCheck size={27} /><strong>{loading ? 'Loading your monitors…' : 'Core monitoring is already on'}</strong><p>Add website, certificate, service or job checks when you need them. CPU, RAM, disk and network do not need extra sources.</p></div>}
        {targets.map(target => { const status = statuses.find(item => item.id === target.id); const managed = target.kind === 'job' && target.address === managedJobPath(connection, target.id); return <article className="source-row source-monitor" key={target.id}><div className="source-monitor-heading"><div><span className="source-kind-label">{kinds[target.kind as SourceKind]?.title || target.kind}</span><h3>{target.name}</h3></div><div className="source-monitor-actions"><button className="icon-button" disabled={busy} aria-label={`${target.name} ${target.enabled ? 'pause' : 'resume'}`} onClick={() => void save(targets.map(item => item.id === target.id ? { ...item, enabled: !item.enabled } : item), target.enabled ? 'Monitor paused. Its configuration has been kept.' : 'Monitor resumed. Waiting for its next check.')}>{target.enabled ? <Pause size={15} /> : <Play size={15} />}</button><button className="icon-button" disabled={busy} aria-label={`${target.name} edit`} onClick={() => focusForm(formFromTarget(target, connection), target.id)}><Pencil size={15} /></button><button className="icon-button danger" disabled={busy} aria-label={`${target.name} remove`} onClick={async () => { if (await save(targets.filter(item => item.id !== target.id), 'Monitor removed. Its source was not modified.') && editing === target.id) { setEditing(null); setForm(blankSourceForm()); } }}><Trash2 size={15} /></button></div></div><code className="source-address">{target.address}</code><div className="source-status-line"><span className={'badge ' + (!target.enabled ? 'neutral' : statusTone[status?.status || ''] || 'neutral')}>{!target.enabled ? 'Paused' : statusNames[status?.status || ''] || 'Waiting for first check'}</span><small>{target.enabled && status ? `Checked ${time(status.at)}` : target.enabled ? 'First check starts automatically' : 'No new checks while paused'}</small></div>
          {target.enabled && status && <><MeasurementChips status={status} /><pre className="log-summary">{status.detail || 'No new log entries.'}</pre></>}
          {target.kind === 'job' && <JobGuide target={target} managed={managed} connection={connection} linux={linux} connectionError={connectionError} />}
        </article>; })}</section></div>
  </div>;
}

function DiagnosticCard({ diagnostic }: { diagnostic: Diagnostic }) {
  const { reading, source, metric, label } = diagnostic;
  const value = reading.value == null ? '—' : reading.unit === 'B' ? bytes(reading.value) : reading.unit === '%' ? percent(reading.value) : `${reading.value.toFixed(reading.unit === 'count' ? 0 : 1)} ${reading.unit}`;
  return <article className={'source-diagnostic-card ' + (reading.value == null ? 'source-diagnostic-unavailable' : '')}><div><h3>{label}</h3><span className="badge neutral">{reading.value == null ? 'Unavailable' : 'Measured'}</span></div><strong>{value}</strong><code>{source} · {metric}</code>{reading.reason && <p>{reading.reason}</p>}{metric === 'memory_pressure' && reading.value != null && <p>Share of time with tasks stalled for memory, averaged over 10 seconds.</p>}{metric === 'oom_kills_5m' && reading.value != null && <p>Observed kernel counter increases retained for 5 minutes; monitoring gaps are not reconstructed.</p>}{metric === 'disk_io_latency' && reading.value != null && <p>Read/write time per completed operation; idle intervals have no latency estimate.</p>}{metric === 'swap_used_percent' && reading.value != null && <p>Swap can remain used after pressure has passed. Compare with stalls and application performance.</p>}</article>;
}
function MeasurementChips({ status }: { status: TargetStatus }) {
  const measurements = status.measurements || {}; const values: [string, string][] = [];
  if (measurements.http_status != null) values.push(['HTTP status', String(measurements.http_status)]);
  if (measurements.http_latency != null) values.push(['Response', `${measurements.http_latency.toFixed(0)} ms`]);
  if (measurements.tls_days != null) values.push(['Certificate remaining', `${measurements.tls_days.toFixed(1)} days`]);
  if (measurements.tls_failure != null) values.push(['TLS verification', measurements.tls_failure > 0 ? 'Failed' : 'Verified']);
  if (measurements.tls_expires_at != null) values.push(['Certificate expires', time(measurements.tls_expires_at)]);
  if (measurements.job_success_age != null && status.status === 'ok') values.push(['Last success', `${Math.floor(measurements.job_success_age / 60)} min ago`]);
  return values.length ? <dl className="source-measurements">{values.map(([label, value]) => <div key={label}><dt>{label}</dt><dd className={value === 'Failed' ? 'source-verification-failed' : undefined}>{value}</dd></div>)}</dl> : null;
}
function JobGuide({ target, managed, connection, linux, connectionError }: { target: Target; managed: boolean; connection: Connection | null; linux: boolean | null; connectionError: string }) {
  const [state, setState] = useState('success'); const [copied, setCopied] = useState(false); const [error, setError] = useState('');
  const command = linux == null ? null : jobReportCommand(connection, target.id, state, linux);
  return <details className="source-job-guide"><summary><Terminal size={14} />How to report this job’s result</summary>{!connection ? <p>{connectionError || 'Loading installation details…'}</p> : managed ? <><p>Add reporting to your existing job script. Each command records a status; it does not run the job. Use one active execution per monitor ID.</p><div className="source-report-states" role="group" aria-label={`${target.name} report state`}>{[['running', 'When it starts'], ['success', 'On success'], ['failure', 'On failure']].map(([value, title]) => <button key={value} type="button" className={'secondary compact ' + (state === value ? 'source-preset-selected' : '')} aria-pressed={state === value} onClick={() => { setState(value); setCopied(false); setError(''); }}>{title}</button>)}</div>{command ? <><pre className="code-block">{command}</pre><button className="secondary compact" disabled={!target.enabled} onClick={async () => { try { await navigator.clipboard.writeText(command); setCopied(true); setError(''); } catch { setError('Clipboard unavailable. Select and copy the command above.'); } }}>{copied ? <Check size={13} /> : <Copy size={13} />}{copied ? 'Command copied' : 'Copy command'}</button><small>{linux ? 'Linux example: run from a privileged shell. runuser switches to the server-guard account that owns the application data.' : 'Run as the same macOS account that owns this Metric Owl installation. Do not run it as root.'}</small></> : <p>{connectionError || 'Loading the installation path for reporting commands…'}</p>}</> : <><p>{connectionError || (!connection ? 'Loading installation details…' : 'This monitor reads a custom JSON report file. Your existing job must write this file atomically and allow the monitoring account to read it.')}</p><pre className="code-block">{'{\n  "state": "success",\n  "last_success_at": <current Unix timestamp in seconds>,\n  "started_at": null\n}'}</pre><small>This is a template, not a complete report. Replace the timestamp with the current UTC Unix time. Supported states: running, success, failure. A running report requires started_at. Keep the file under 16 KiB and do not use symbolic links.</small></>}{!target.enabled && <small>Resume this monitor before sending managed job reports.</small>}<small>A successful report confirms the job’s reported result, not that a backup can be restored.</small>{error && <p className="notice error" role="alert">{error}</p>}</details>;
}
