import { useEffect, useRef, useState } from 'react';
import { Plus, Pencil, Check, Trash2, ShieldCheck, Cpu, MemoryStick, HardDrive, Info, Sparkles, ArrowUpRight, Search, X } from 'lucide-react';
import type { Rule } from './generated/Rule';
import type { Sample } from './generated/Sample';
import type { Target } from './generated/Target';
import type { ConnectionTab } from './SetupGuide';
import { api } from './lib';
import { basicRules, comparison, conditionText, metrics, newRule, recoveryFor, secondsLabel, sourceHelp, sourceLabel, sourceOptions, thresholdLabel, timingText } from './rulePresentation';
import './Rules.css';

type Pack = { id: string; name: string; description: string; rules: Rule[] };
type ConnectionStatus = { settings: { ai_auto: boolean; email_transport?: string; recipient_verified?: boolean; instant?: boolean }; keys: { ai?: boolean } };
type Props = { initialCreate?: boolean; sample?: Sample | null; onConnection?: (tab: ConnectionTab) => void };
const basicMetrics = ['cpu', 'memory', 'disk_used', 'disk_free'];
const icons = { cpu: Cpu, memory: MemoryStick, disk_used: HardDrive };

export default function Rules({ initialCreate = false, sample = null, onConnection }: Props) {
  const [rules, setRules] = useState<Rule[]>([]);
  const [presets, setPresets] = useState<Pack[]>([]);
  const [targets, setTargets] = useState<Target[]>([]);
  const [settings, setSettings] = useState<ConnectionStatus | null>(null);
  const [mode, setMode] = useState<'basic' | 'advanced'>('basic');
  const [editing, setEditing] = useState<Rule | null>(() => initialCreate ? newRule() : null);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const [loadError, setLoadError] = useState('');
  const [optionalError, setOptionalError] = useState('');
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const [loaded, setLoaded] = useState(false);
  const [retry, setRetry] = useState(0);
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [removed, setRemoved] = useState<Rule | null>(null);
  useEffect(() => {
    let current = true;
    setLoaded(false); setLoadError('');
    api<Rule[]>('rules').then(r => { if (current) { setRules(r); setLoaded(true); } }).catch(e => { if (current) setLoadError(e.message); });
    api<Pack[]>('rules/presets').then(r => { if (current) setPresets(r); }).catch(() => { if (current) setOptionalError('Optional presets are unavailable. Your saved rules are unchanged.'); });
    api<{ targets: Target[] }>('targets').then(r => { if (current) setTargets(r.targets); }).catch(() => {});
    api<ConnectionStatus>('settings').then(r => { if (current) setSettings(r); }).catch(() => {});
    return () => { current = false; };
  }, [retry]);
  async function save(next: Rule[], feedback = 'Alert saved. Enabled rules run automatically.') {
    if (!loaded || saving.current) return false;
    saving.current = true; setBusy(true); setError('');
    try {
      await api('rules', 'PUT', next);
      setRules(next); setMessage(feedback); if (editing) { setQuery(''); setFilter('all'); } setEditing(null); return true;
    } catch (e) { setError((e as Error).message); return false; }
    finally { saving.current = false; setBusy(false); }
  }
  const open = (rule: Rule) => { setError(''); setMessage(''); setEditing({ ...rule }); };
  const toggle = (rule: Rule) => void save(rules.map(r => r.id === rule.id ? { ...r, enabled: !r.enabled } : r), rule.enabled ? 'Alert paused. Its settings are kept.' : 'Alert enabled. It will evaluate new measurements.');
  const basic = basicRules(rules, sample);
  const basicIds = new Set(basic.flatMap(item => item.rule ? [item.rule.id] : []));
  const additional = rules.filter(r => basicMetrics.includes(r.metric) && !basicIds.has(r.id) && !/^(disk-|cpu-|memory-|diag-|sensitive-|sustained-)/.test(r.id));
  const otherEnabled = rules.filter(r => r.enabled && !basicIds.has(r.id) && !additional.some(a => a.id === r.id)).length;
  const visible = rules.filter(r => (filter === 'all' || (filter === 'enabled' ? r.enabled : !r.enabled)) && `${r.name} ${r.source} ${metrics[r.metric]?.label} ${r.metric}`.toLowerCase().includes(query.toLowerCase()));
  const emailReady = settings && settings.settings.email_transport !== 'disabled' && settings.settings.recipient_verified && settings.settings.instant;
  function starter(metric: string, severity: string, source: string) {
    const preset = presets.find(p => p.id === 'balanced')?.rules.find(r => r.metric === metric && r.severity === severity);
    if (preset && !rules.some(r => r.id === preset.id)) return open({ ...preset });
    const r = newRule(metric, source);
    if (severity === 'critical') { r.severity = 'critical'; r.threshold = metric === 'cpu' ? 90 : 95; r.recovery_threshold = r.threshold - (metric === 'disk_used' ? 3 : 5); r.window_seconds = metric === 'disk_used' ? 30 : 180; }
    r.name = `${metrics[metric].label} · ${severity}`;
    open(r);
  }
  function card(rule: Rule, compact = false) {
    return <article key={rule.id} className={'alert-card ' + (compact ? 'alert-extra ' : '') + (!rule.enabled ? 'alert-paused' : '')} data-rule-id={rule.id}>
      <div className="alert-card-top"><span className={'badge ' + rule.severity}>{rule.severity === 'critical' ? 'Critical' : 'Warning'}</span><label className="alert-switch"><input type="checkbox" aria-label={`${rule.name} enabled`} checked={rule.enabled} disabled={busy} onChange={() => toggle(rule)} /><span>{rule.enabled ? 'On' : 'Off'}</span></label></div>
      <h3>{conditionText(rule)}</h3><p className="alert-timing">{timingText(rule)}</p>
      <p className="alert-meaning">{metrics[rule.metric]?.help}</p>
      <div className="alert-card-bottom"><span>{sourceLabel(rule.source)}</span><button className="secondary compact" aria-label={`${rule.name} edit`} disabled={busy} onClick={() => open(rule)}><Pencil size={14} />Edit</button></div>
    </article>;
  }
  return <div className="alerts-workspace">
    <div className="alerts-toolbar"><div className="alerts-mode" role="group" aria-label="Alert view"><button aria-pressed={mode === 'basic'} onClick={() => setMode('basic')}><ShieldCheck size={16} />Basic</button><button aria-pressed={mode === 'advanced'} onClick={() => setMode('advanced')}>Advanced</button></div><button className="primary" disabled={busy || !loaded} onClick={() => open(newRule())}><Plus size={16} />Add alert</button></div>
    {error && !editing && <div className="notice error" role="alert">{error}</div>}
    {message && <div className="notice success" role="status"><Check size={18} /><span>{message}</span>{removed && <button className="text-button" disabled={busy} onClick={async () => { if (await save([...rules, removed], 'Alert restored.')) setRemoved(null); }}>Undo removal</button>}</div>}
    {loadError ? <div className="notice error" role="alert">Could not load your alerts. {loadError}<button className="secondary compact" onClick={() => setRetry(r => r + 1)}>Retry</button></div> : !loaded ? <div className="panel empty" role="status">Loading your alerts…</div> : mode === 'basic' ? <>
      <div className="alerts-intro"><h2>Everyday protection</h2><p>Six essential alerts for a busy processor, low memory and a filling disk. Edit a limit whenever you need.</p></div>
      <div className="alert-basic-groups">{['cpu', 'memory', 'disk_used'].map(metric => { const Icon = icons[metric as keyof typeof icons]; return <section className="alert-basic-group" key={metric} aria-label={`${metrics[metric].label} alerts`}><h2><Icon size={20} />{metric === 'cpu' ? 'Processor' : metric === 'memory' ? 'Memory' : 'Main disk'}</h2>{basic.filter(item => item.metric === metric).map(item => item.rule ? card(item.rule) : <article className="alert-card alert-missing" key={item.severity}><span className="badge neutral">Not added</span><h3>{metrics[metric].label} · {item.severity}</h3><p>{metrics[metric].help}</p><button className="secondary compact" disabled={busy || !item.source} onClick={() => starter(metric, item.severity, item.source!)}>Add this alert</button>{!item.source && <small>Waiting for disk measurements.</small>}</article>)}</section>; })}</div>
      {additional.length > 0 && <section><h2 className="alerts-extra-title">Your extra alerts</h2><div className="alert-extra-grid">{additional.map(r => card(r, true))}</div></section>}
      <div className="alerts-note"><Info size={18} /><p>{otherEnabled > 0 ? <><strong>{otherEnabled} additional protections are enabled.</strong> Other disks, file limits and specialist checks are in Advanced. </> : <>Advanced contains all your saved rules and specialist checks. </>}Changing this view does not turn any alerts on or off.</p><button className="text-button" onClick={() => setMode('advanced')}>View Advanced <ArrowUpRight size={15} /></button></div>
    </> : <>
      <div className="alerts-intro"><h2>All alert rules</h2><p>{rules.filter(r => r.enabled).length} enabled · {rules.length} saved. Every rule includes an explanation of what it watches.</p></div>
      <div className="alert-filters"><label className="alert-search"><Search size={17} /><input aria-label="Search alerts" placeholder="Find an alert, disk or service…" value={query} onChange={e => setQuery(e.target.value)} /></label><select aria-label="Filter alerts" value={filter} onChange={e => setFilter(e.target.value)}><option value="all">All alerts</option><option value="enabled">Enabled</option><option value="paused">Paused</option></select></div>
      <div className="alert-advanced-list">{visible.map(r => <article className="alert-advanced-row" key={r.id} data-rule-id={r.id}><div className="alert-row-copy"><div className="alert-row-heading"><h3>{r.name}</h3><span className={'badge ' + r.severity}>{r.severity === 'critical' ? 'Critical' : 'Warning'}</span></div><p>{conditionText(r)} · {timingText(r)}</p><small>{targets.find(t => t.id === r.source)?.name || sourceLabel(r.source)} · {r.notify ? 'Notify through configured channels' : 'Record in dashboard only'}</small><details className="alert-explanation"><summary>What does this alert mean?</summary><p>{metrics[r.metric]?.help || 'A custom monitoring condition.'}</p>{sourceHelp(r) && <p>{sourceHelp(r)}</p>}<p>Resolves when {metrics[r.metric]?.label.toLowerCase() || r.metric} is {r.operator.startsWith('>') ? 'at or below' : 'at or above'} {thresholdLabel(r, r.recovery_threshold)} for {secondsLabel(r.recovery_seconds)}.</p></details></div><div className="alert-row-actions"><label className="alert-switch"><input type="checkbox" aria-label={`${r.name} enabled`} checked={r.enabled} disabled={busy} onChange={() => toggle(r)} /><span>{r.enabled ? 'On' : 'Off'}</span></label><button className="secondary compact" aria-label={`${r.name} edit`} disabled={busy} onClick={() => open(r)}><Pencil size={14} />Edit</button><button className="icon-button danger" aria-label={`${r.name} delete`} disabled={busy} onClick={async () => { if (await save(rules.filter(x => x.id !== r.id), 'Alert removed. You can undo this until you leave the page.')) setRemoved(r); }}><Trash2 size={16} /></button></div></article>)}</div>
      {!visible.length && <div className="panel empty">{rules.length ? 'No alerts match your search.' : 'No alerts saved. Add an alert to start monitoring a condition.'}</div>}
      <details className="rules-library"><summary>Optional preset packs<small>Review before applying; matching preset IDs are replaced.</small></summary><div className="rules-library-body">{optionalError && <p role="status">{optionalError}</p>}<div className="preset-grid">{presets.map(p => <section className="panel preset" key={p.id}><h3>{p.name}</h3><p>{p.description}</p><div className="preset-examples">{p.rules.map(r => <span key={r.id}>{conditionText(r)}<small>{timingText(r)}</small></span>)}</div><button className="secondary" disabled={busy} onClick={() => void save([...rules.filter(r => !p.rules.some(x => x.id === r.id)), ...p.rules], 'Preset applied. Matching preset rules were updated; other rules were kept.')}>Apply pack</button></section>)}</div></div></details>
    </>}
    <section className="alerts-assistant" aria-label="AI and notifications"><div className="alerts-assistant-icon"><Sparkles size={22} /></div><div><h2>AI helps explain important events</h2><p>{!settings ? 'Connection status is unavailable.' : !settings.keys.ai ? 'Connect your provider once. Then ask AI to explain an event and suggest what to check.' : settings.settings.ai_auto ? 'Automatic analysis is on for eligible, unexplained events. Routine CPU, memory and disk alerts do not call AI.' : 'Your AI key is saved. Open an event to request an explanation, or enable automatic analysis in AI settings.'}</p><small>Alerts keep working independently. AI cannot change your server.</small></div>{onConnection && <button className="secondary compact" onClick={() => onConnection('ai')}>{settings?.keys.ai ? 'AI settings' : 'Connect AI'}<ArrowUpRight size={14} /></button>}</section>
    <div className="alerts-delivery"><span>{!settings ? 'Email status unavailable.' : emailReady ? 'Email recipient verified. Delivery depends on your provider.' : 'Alerts appear in Events. Email notifications need a configured provider and verified recipient.'}</span>{onConnection && <button className="text-button" onClick={() => onConnection('email')}>Notification settings</button>}</div>
    {editing && <RuleEditor key={editing.id} initial={editing} advanced={mode === 'advanced'} existing={rules.some(r => r.id === editing.id)} rules={rules} sample={sample} targets={targets} busy={busy} loaded={loaded} error={error || loadError} onClose={() => { if (!busy) { setEditing(null); setError(''); } }} onSave={rule => void save(rules.some(r => r.id === rule.id) ? rules.map(r => r.id === rule.id ? rule : r) : [...rules, rule])} />}
  </div>;
}

function RuleEditor({ initial, advanced, existing, rules, sample, targets, busy, loaded, error, onClose, onSave }: { initial: Rule; advanced: boolean; existing: boolean; rules: Rule[]; sample: Sample | null; targets: Target[]; busy: boolean; loaded: boolean; error: string; onClose: () => void; onSave: (rule: Rule) => void }) {
  const [rule, setRule] = useState(initial);
  const [manualRecovery, setManualRecovery] = useState(false);
  const [customSource, setCustomSource] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { const node = dialog.current!; const opener = document.activeElement as HTMLElement | null; node.showModal(); node.querySelector('select')?.focus(); return () => { node.close(); if (opener?.isConnected) opener.focus({ preventScroll: true }); }; }, []);
  const info = metrics[rule.metric];
  const scale = info?.scale || 1;
  const sources = sourceOptions(rule.metric, sample, targets, [...rules, rule]);
  const options = advanced ? Object.keys(metrics) : [...new Set([...basicMetrics, rule.metric])];
  const field = <K extends keyof Rule>(key: K, value: Rule[K]) => setRule(current => ({ ...current, [key]: value }));
  function changeMetric(metric: string) {
    const choices = sourceOptions(metric, sample, targets, rules);
    const next = newRule(metric, choices[0]?.value || (metrics[metric].source === 'host' ? 'host' : ''));
    setRule({ ...next, id: rule.id, enabled: rule.enabled, notify: rule.notify, name: existing ? rule.name : next.name });
    setManualRecovery(false); setCustomSource(false);
  }
  function changeCondition(threshold: number, operator = rule.operator) {
    setRule({ ...rule, threshold, operator, recovery_threshold: manualRecovery ? rule.recovery_threshold : recoveryFor(rule, threshold, operator) });
  }
  return <dialog ref={dialog} className="alert-dialog" aria-labelledby="alert-editor-title" onCancel={e => { e.preventDefault(); onClose(); }}>
    <div className="alert-dialog-heading"><div><span className="eyebrow">{existing ? 'YOUR ALERT' : 'NEW ALERT'}</span><h2 id="alert-editor-title">{existing ? 'Edit alert' : 'Add alert'}</h2></div><button className="icon-button" aria-label="Close alert editor" disabled={busy} onClick={onClose}><X size={20} /></button></div>
    <form onSubmit={e => { e.preventDefault(); if (!busy && loaded) onSave(rule); }}>
      <div className="alert-dialog-body"><fieldset disabled={busy}>
        <label>What should we watch?<select value={rule.metric} onChange={e => changeMetric(e.target.value)}>{options.map(metric => <option key={metric} value={metric}>{metrics[metric]?.label || metric}</option>)}</select></label>
        <div className="alert-help"><Info size={18} /><div><strong>What this means</strong><p>{info?.help}</p>{sourceHelp(rule) && <p>{sourceHelp(rule)}</p>}</div></div>
        {info?.source !== 'host' && <label>{info?.source === 'disk' ? 'Disk to watch' : info?.source === 'device' ? 'Storage device' : 'Monitor to watch'}<select required value={customSource ? '__custom' : rule.source} onChange={e => { if (e.target.value === '__custom') setCustomSource(true); else { setCustomSource(false); field('source', e.target.value); } }}><option value="" disabled>Choose a monitored source</option>{sources.filter(s => s.value).map(source => <option key={source.value} value={source.value}>{source.label}</option>)}{advanced && <option value="__custom">Enter a source ID…</option>}</select>{customSource && <input aria-label="Custom source ID" required maxLength={300} value={rule.source} onChange={e => field('source', e.target.value)} />}{sources.every(s => !s.value) && <small>Add a source in Applications & storage first.</small>}</label>}
        <div className="alert-condition-fields"><label>Alert when<select value={rule.operator} onChange={e => changeCondition(rule.threshold, e.target.value)}>{Object.entries(comparison).map(([value, label]) => <option key={value} value={value}>{label.charAt(0).toUpperCase() + label.slice(1)}</option>)}</select></label><label>Limit ({info?.unit || 'value'})<input required type="number" step="any" value={rule.threshold / scale} onChange={e => changeCondition(e.target.valueAsNumber * scale)} /></label><label>Watch for<select value={rule.window_seconds} onChange={e => field('window_seconds', +e.target.value)}>{[...new Set([10, 30, 60, 180, 300, 900, 1800, 3600, rule.window_seconds])].sort((a, b) => a - b).map(seconds => <option value={seconds} key={seconds}>{secondsLabel(seconds)}</option>)}</select></label></div>
        <div className="alert-live-summary" aria-live="polite"><ShieldCheck size={19} /><div><strong>{conditionText(rule)}</strong><p>{timingText(rule)} → {rule.severity === 'critical' ? 'critical alert' : 'warning'}.</p><small>{rule.fraction < 1 ? 'Short dips are allowed within this window. ' : ''}Requires enough recent measurements.</small></div></div>
        <details className="alert-more"><summary>More options<small>Name, severity, recovery and notifications</small></summary><div className="alert-more-fields">
          <label>Rule name<input required maxLength={160} value={rule.name} onChange={e => field('name', e.target.value)} /></label>
          <div className="alert-two-fields"><label>Severity<select value={rule.severity} onChange={e => field('severity', e.target.value)}><option value="warning">Warning — needs attention</option><option value="critical">Critical — act soon</option></select></label><label>Required time above / below limit (%)<input required type="number" min={10} max={100} step="any" value={rule.fraction * 100} onChange={e => field('fraction', +e.target.value / 100)} /><small>100% requires the condition throughout the window; 80% tolerates brief dips.</small></label></div>
          {advanced && <label>Exact window (seconds)<input required type="number" min={2} max={3600} step={1} value={rule.window_seconds} onChange={e => field('window_seconds', +e.target.value)} /><small>Up to one hour. Multi-day recurrence rules are not supported yet.</small></label>}
          <div className="alert-two-fields"><label>Resolve {rule.operator.startsWith('>') ? 'at or below' : 'at or above'} ({info?.unit || 'value'})<input required type="number" step="any" value={rule.recovery_threshold / scale} onChange={e => { setManualRecovery(true); field('recovery_threshold', e.target.valueAsNumber * scale); }} /><small>A separate recovery limit prevents repeated on/off alerts.</small></label><label>Recovery duration (seconds)<input required type="number" min={2} max={3600} step={1} value={rule.recovery_seconds} onChange={e => field('recovery_seconds', +e.target.value)} /></label></div>
          <label className="check-label"><input type="checkbox" checked={rule.notify} onChange={e => field('notify', e.target.checked)} />Notify through configured channels<small>Without a channel, alerts still appear in Events.</small></label>
        </div></details>
      </fieldset>{error && <div className="notice error" role="alert">{error}</div>}{!loaded && <p role="status">Waiting for your saved alerts before allowing changes.</p>}</div>
      <div className="alert-dialog-actions"><button type="button" className="secondary" disabled={busy} onClick={onClose}>Cancel</button><button className="primary" disabled={busy || !loaded || !rule.source}>{busy ? 'Saving…' : 'Save'}</button></div>
    </form>
  </dialog>;
}
