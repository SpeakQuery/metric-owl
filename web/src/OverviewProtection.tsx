import {useEffect, useState} from 'react';
import {ArrowUpRight, Cpu, HardDrive, Mail, MemoryStick, PlugZap, Plus, RefreshCw, ShieldCheck, SlidersHorizontal, Sparkles} from 'lucide-react';
import type {Rule} from './generated/Rule';
import type {ConnectionTab} from './SetupGuide';
import {api, bytes, severity} from './lib';
import './OverviewProtection.css';

type Props = {
  onManage: () => void;
  onCreate: () => void;
  onConnection: (tab: ConnectionTab) => void;
};
type ConnectionSettings = {
  settings: {
    email_transport?: string;
    relay_url?: string;
    smtp_host?: string;
    smtp_username?: string;
    smtp_from?: string;
    recipient?: string;
    recipient_verified?: boolean;
    instant?: boolean;
  };
  keys: {ai?: boolean; smtp?: boolean; relay?: boolean};
};

const metrics: Record<string, {label: string; group: 'cpu' | 'memory' | 'disk'; unit: '%' | 'bytes' | 'ms' | 'count'}> = {
  cpu: {label: 'CPU usage', group: 'cpu', unit: '%'},
  cpu_iowait: {label: 'CPU I/O wait', group: 'cpu', unit: '%'},
  memory: {label: 'RAM usage', group: 'memory', unit: '%'},
  memory_pressure: {label: 'Memory pressure', group: 'memory', unit: '%'},
  swap_used: {label: 'Swap used', group: 'memory', unit: 'bytes'},
  swap_total: {label: 'Swap capacity', group: 'memory', unit: 'bytes'},
  swap_used_percent: {label: 'Swap usage', group: 'memory', unit: '%'},
  oom_kills_5m: {label: 'Out-of-memory kills', group: 'memory', unit: 'count'},
  disk_used: {label: 'Disk usage', group: 'disk', unit: '%'},
  disk_free: {label: 'Disk free space', group: 'disk', unit: 'bytes'},
  inode_used: {label: 'Inode usage', group: 'disk', unit: '%'},
  disk_io_latency: {label: 'Disk I/O latency', group: 'disk', unit: 'ms'},
  disk_io_utilization: {label: 'Disk I/O utilization', group: 'disk', unit: '%'},
};
const number = new Intl.NumberFormat('en-GB', {maximumFractionDigits: 2});
const icons = {cpu: Cpu, memory: MemoryStick, disk: HardDrive};
function windowLabel(seconds: number) {
  if (seconds >= 3600 && seconds % 3600 === 0) return `${number.format(seconds / 3600)} hr`;
  if (seconds >= 60 && seconds % 60 === 0) return `${number.format(seconds / 60)} min`;
  return `${number.format(seconds)} sec`;
}
function threshold(rule: Rule) {
  const unit = metrics[rule.metric].unit;
  return `${rule.operator} ${unit === 'bytes' ? bytes(rule.threshold) : number.format(rule.threshold) + (unit === '%' ? '%' : unit === 'ms' ? ' ms' : '')}`;
}
function emailStatus(data: ConnectionSettings | null) {
  if (!data) return 'Status unavailable';
  const s = data.settings;
  const transport = s.email_transport || (s.relay_url ? 'relay' : 'disabled');
  if (transport === 'disabled') return 'Not configured · email off';
  const configured = transport === 'smtp'
    ? s.smtp_host && s.smtp_username && s.smtp_from && data.keys.smtp
    : transport === 'relay' && s.relay_url && data.keys.relay;
  if (!configured || !s.recipient) return 'Setup incomplete';
  if (!s.recipient_verified) return 'Verify your recipient';
  if (!s.instant) return 'Instant alerts off';
  return 'Recipient verified';
}

export default function OverviewProtection({onManage, onCreate, onConnection}: Props) {
  const [rules, setRules] = useState<Rule[] | null>(null);
  const [settings, setSettings] = useState<ConnectionSettings | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let current = true;
    async function load() {
      setLoading(true);
      const [savedRules, savedSettings] = await Promise.allSettled([
        api<Rule[]>('rules'), api<ConnectionSettings>('settings'),
      ]);
      if (!current) return;
      setRules(savedRules.status === 'fulfilled' ? savedRules.value : null);
      setSettings(savedSettings.status === 'fulfilled' ? savedSettings.value : null);
      setError(savedRules.status === 'rejected' ? 'Alert rules could not be loaded. Retry to see their saved status.' : '');
      setLoading(false);
    }
    void load();
    window.addEventListener('focus', load);
    return () => {current = false; window.removeEventListener('focus', load);};
  }, [retry]);

  const included = (rules || []).filter(rule => metrics[rule.metric]);
  const groupOrder = {cpu: 0, memory: 1, disk: 2};
  const rows = [...included].sort((a, b) => groupOrder[metrics[a.metric].group] - groupOrder[metrics[b.metric].group]
    || a.source.localeCompare(b.source) || a.name.localeCompare(b.name));
  const enabled = rows.filter(rule => rule.enabled).length;
  const other = (rules?.length || 0) - rows.length;
  return <section className="panel overview-protection" aria-label="Alert coverage" aria-busy={loading}>
    <div className="protection-heading">
      <div className="protection-title"><span className="protection-mark"><ShieldCheck size={20}/></span><div><h2>Your alert coverage</h2><p>Saved rules watch your CPU, memory and disks automatically when enabled.</p></div></div>
      <div className="protection-actions"><button className="icon-button" aria-label="Refresh alert coverage" disabled={loading} onClick={() => setRetry(value => value + 1)}><RefreshCw size={15}/></button><button className="secondary compact" onClick={onManage}><SlidersHorizontal size={14}/>Manage alerts</button><button className="primary compact" onClick={onCreate}><Plus size={14}/>Add custom rule</button></div>
    </div>
    <div className="protection-summary"><span className="protection-count">{rules ? <><strong>{enabled}</strong> of {rows.length} rules enabled</> : loading ? 'Loading saved rules…' : 'Rule status unavailable'}</span><span>Local detection · no AI needed</span></div>
    {error ? <div className="protection-empty" role="alert">{error}</div> : !rules ? <p className="protection-empty">Loading your saved alert rules…</p> : !rows.length ? <div className="protection-empty"><strong>No CPU, memory or disk rules configured.</strong><p>Open Manage alerts to add the rules you want.</p></div> : <div className="protection-table-scroll" tabIndex={0} role="region" aria-label="Scrollable alert rules">
      <table className="protection-table" aria-label="CPU, memory and disk alert rules" aria-describedby="protection-status-note"><thead><tr><th scope="col">Metric / rule</th><th scope="col">Trigger</th><th scope="col">Severity</th><th scope="col">Status</th></tr></thead><tbody>{rows.map(rule => {
        const metric = metrics[rule.metric];
        const Icon = icons[metric.group];
        return <tr key={rule.id} data-rule-id={rule.id} data-rule-enabled={String(rule.enabled)}>
          <th scope="row"><div className={`protection-metric ${metric.group}`}><Icon size={17}/><div><strong>{metric.label}</strong><span>{rule.name}</span><small>{rule.source === 'host' ? 'This host' : rule.source}</small></div></div></th>
          <td className="protection-trigger"><strong>{threshold(rule)}</strong><span>{number.format(rule.fraction * 100)}% of {windowLabel(rule.window_seconds)}</span></td>
          <td><span className={`protection-severity ${rule.severity === 'critical' ? 'critical' : rule.severity === 'warning' ? 'warning' : 'neutral'}`}><i aria-hidden="true"/>{severity[rule.severity] || rule.severity}</span></td>
          <td><span className={`protection-state ${rule.enabled ? 'enabled' : 'disabled'}`}><span aria-hidden="true">{rule.enabled ? '●' : '○'}</span>{rule.enabled ? 'Enabled' : 'Disabled'}</span></td>
        </tr>;
      })}</tbody></table>
    </div>}
    <div className="protection-footnote" id="protection-status-note"><span>Enabled rules detect incidents locally. Email delivery needs a configured connection.</span>{other > 0 && <button className="text-button" onClick={onManage}>{other} other {other === 1 ? 'rule' : 'rules'}<ArrowUpRight size={13}/></button>}</div>
    <div className="protection-connections" aria-label="Optional connections">
      <div className="protection-connections-title"><span>OPTIONAL CONNECTIONS</span><p>Rules run locally. Add email or AI when you need them.</p></div>
      <button className="protection-connection email" onClick={() => onConnection('email')} aria-label="Configure email"><Mail size={17}/><span><strong>Configure email</strong><small>{loading ? 'Checking settings…' : emailStatus(settings)}</small></span><ArrowUpRight size={14}/></button>
      <button className="protection-connection ai" onClick={() => onConnection('ai')} aria-label="Connect AI"><Sparkles size={17}/><span><strong>Connect AI</strong><small>{loading ? 'Checking settings…' : settings ? settings.keys.ai ? 'Key saved · optional analysis' : 'Optional analysis' : 'Status unavailable'}</small></span><ArrowUpRight size={14}/></button>
      <button className="protection-connection mcp" onClick={() => onConnection('mcp')} aria-label="Connect MCP"><PlugZap size={17}/><span><strong>Connect MCP</strong><small>Read-only access for your agent</small></span><ArrowUpRight size={14}/></button>
    </div>
  </section>;
}
