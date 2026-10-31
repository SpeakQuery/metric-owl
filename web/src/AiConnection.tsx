import { useEffect, useState } from 'react';
import { CheckCircle2, KeyRound, PlugZap, ShieldCheck, Sparkles } from 'lucide-react';
import { api } from './lib';
import './AiConnection.css';

type State = { providers: { id: string; name: string }[]; selected_provider: string | null; key_saved: boolean; verified: boolean };
export default function AiConnection({ onConnected, onReauthenticate }: { onConnected: () => void; onReauthenticate: () => void }) {
  const [state, setState] = useState<State | null>(null);
  const [provider, setProvider] = useState('');
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ ok: boolean; text: string } | null>(null);
  async function load() {
    try {
      const result = await api<State>('ai/providers');
      setState(result); setProvider(result.selected_provider || (result.key_saved ? '' : result.providers[0]?.id || ''));
    } catch (error) { setFeedback({ ok: false, text: (error as Error).message }); }
  }
  useEffect(() => { void load(); }, []);
  const sameProvider = !!provider && state?.selected_provider === provider;
  const saved = sameProvider && state?.key_saved;
  const connected = sameProvider && state?.verified && !key && feedback?.ok !== false;
  async function connect(event: React.FormEvent) {
    event.preventDefault();
    if (busy) return;
    setBusy(true); setFeedback(null);
    try {
      const result = await api<{ message: string }>('ai/connect', 'POST', { provider, key: key.trim() });
      setKey('');
      setState(current => current && { ...current, selected_provider: provider, key_saved: true, verified: true });
      setFeedback({ ok: true, text: result.message });
      onConnected();
    } catch (error) {
      const text = (error as Error).message;
      setFeedback({ ok: false, text });
      if (/re-authenticat|verify your identity again/i.test(text)) onReauthenticate();
    } finally { setBusy(false); }
  }
  return <section className="panel ai-connect">
    <div className="ai-connect-intro"><div className="section-icon tone-ai"><Sparkles /></div><span className="eyebrow">YOUR PROVIDER. YOUR KEY.</span><h2>Connect your AI provider</h2><p>Choose a provider, paste your API key, and connect. We take care of the rest.</p></div>
    {feedback && <div className={'notice ' + (feedback.ok ? 'success' : 'error')} role={feedback.ok ? 'status' : 'alert'}>{feedback.ok ? <CheckCircle2 size={18} /> : <KeyRound size={18} />}<span>{feedback.text}</span></div>}
    {!state ? <div className="empty">{feedback ? <button className="secondary" onClick={load}>Retry</button> : 'Loading providers…'}</div> : <form onSubmit={connect} className="ai-connect-form">
      <label htmlFor="ai-provider">AI provider<select id="ai-provider" value={provider} disabled={busy} onChange={event => { setProvider(event.target.value); setKey(''); setFeedback(null); }}>
        {!provider && <option value="" disabled>Choose a provider</option>}
        {state.providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
      </select></label>
      <label htmlFor="ai-api-key">API key<div className="input-status"><input id="ai-api-key" aria-label="API key" type="password" autoComplete="new-password" spellCheck={false} autoCapitalize="none" maxLength={4096} value={key} disabled={busy} required={!saved} onChange={event => { setKey(event.target.value); setFeedback(null); }} placeholder={saved ? 'Saved securely · paste a new key to replace it' : 'Paste your provider’s API key'} />{saved && <span className="badge good">Saved</span>}</div></label>
      {state.key_saved && !state.selected_provider && <p className="muted">Your existing custom connection is kept until you connect a provider here.</p>}
      <button className="primary ai-connect-submit" disabled={busy || !provider || (!key.trim() && !saved)} type="submit">{connected ? <CheckCircle2 size={17} /> : <PlugZap size={17} />}{busy ? 'Connecting…' : connected ? 'Check connection' : 'Connect'}</button>
      <p className="ai-connect-note">Connecting sends a short test message to your provider. Its API usage rates apply.</p>
      {connected && <div className="ai-connect-ready"><CheckCircle2 size={18} /><span>Ready. Open an event and choose <strong>Analyze evidence with AI</strong>.</span></div>}
    </form>}
    <div className="ai-connect-footer"><ShieldCheck size={17} /><span>Your key is encrypted on this server. AI can analyze events, with no permission to change your server.</span></div>
  </section>;
}
