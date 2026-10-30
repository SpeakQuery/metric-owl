import { useEffect, useState } from 'react';
import { ArrowRight, CheckCircle2, Mail, PlugZap, ShieldCheck, SlidersHorizontal, Sparkles } from 'lucide-react';
import { api } from './lib';
export type ConnectionTab = 'email' | 'ai' | 'mcp' | 'webhook';
type Actions = { onConnection: (tab: ConnectionTab) => void; onRules: () => void };
export function QuickSetup({ onConnection, onRules }: Actions) {
  return <div className="quick-setup" aria-label="Quick setup shortcuts"><span>QUICK SETUP</span><button onClick={() => onConnection('mcp')}><PlugZap size={15} /> Connect Codex / MCP</button><button onClick={() => onConnection('ai')}><Sparkles size={15} /> AI key</button><button onClick={() => onConnection('email')}><Mail size={15} /> Email reports</button><button onClick={onRules}><SlidersHorizontal size={15} /> Alert rules</button></div>;
}
export default function SetupGuide({ onConnection, onRules, onDone }: Actions & { onDone: () => void }) {
  const [state, setState] = useState<any>(null);
  const [error, setError] = useState('');
  useEffect(() => { Promise.all([api('settings'), api('mcp/clients'), api('rules')]).then(([settings, clients, rules]) => setState({ settings, clients, rules })).catch(e => setError(e.message)); }, []);
  const cards = [
    { id: 'mcp', icon: PlugZap, title: 'Connect your AI agent', description: 'Let Codex or another MCP client ask about your server, recent incidents, and CPU or memory history.', action: 'Set up MCP connection', configured: state?.clients.some((c: any) => !c.revoked), status: 'Client key created', run: () => onConnection('mcp') },
    { id: 'email', icon: Mail, title: 'Choose where alerts go', description: 'Verify an email address, choose report times, and test your notification connection.', action: 'Set up email reports', configured: state?.settings.settings.recipient_verified, status: 'Recipient verified', run: () => onConnection('email') },
    { id: 'ai', icon: Sparkles, title: 'Add optional AI insight', description: 'Use your own provider key for deeper incident analysis. Standard monitoring works without it.', action: 'Add and test AI key', configured: state?.settings.keys.ai, status: 'Key saved · test available', run: () => onConnection('ai') },
    { id: 'rules', icon: SlidersHorizontal, title: 'Make alerts work for you', description: 'CPU, memory, and disk protections are ready. Pick a rule pack or adjust thresholds and durations.', action: 'Review ready-made rules', configured: state?.rules.some((r: any) => r.enabled), status: 'Monitoring rules enabled', run: onRules },
  ];
  return <div className="setup-guide"><section className="panel setup-welcome"><div className="section-icon"><ShieldCheck /></div><div><span className="eyebrow">A FEW MINUTES TO MAKE IT YOURS</span><h2>Your guard is already watching.</h2><p>Live monitoring starts automatically. Set up the connections you need, in any order. These steps are optional.</p></div><span className="badge good">Read-only by design</span></section>{error && <div className="notice error" role="alert">{error}</div>}<div className="setup-grid">{cards.map(c => <section className="panel setup-step" data-setup={c.id} key={c.id}><div className={'section-icon ' + (c.id === 'ai' || c.id === 'rules' ? 'tone-ai' : 'tone-blue')}><c.icon /></div><span className={'badge ' + (c.configured ? 'good' : 'neutral')}>{state ? c.configured ? c.status : 'Optional · not configured' : 'Checking settings…'}</span><h2>{c.title}</h2><p>{c.description}</p><button className="secondary" onClick={c.run}>{c.action}<ArrowRight size={15} /></button></section>)}</div><div className="setup-finish"><span><CheckCircle2 size={18} /> You can return to this guide from the dashboard at any time.</span><button className="primary" onClick={onDone}>Continue to dashboard <ArrowRight size={16} /></button></div></div>;
}
