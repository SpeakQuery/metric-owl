import { useEffect, useRef, useState } from 'react';
import { ArrowUp, ArrowUpRight, CheckCircle2, Clock3, MessageSquare, Plus, RefreshCw, ShieldCheck, Sparkles } from 'lucide-react';
import { api, csrf, time } from './lib';
import './ServerChat.css';

type Usage = { used: number; limit: number; remaining: number; resets: string };
type Status = { connected: boolean; key_saved: boolean; provider: { id: string; name: string } | null; usage: Usage; cost_notice: string };
type Reply = { answer: string; refused: boolean; charged: boolean; period?: { from: number; to: number }; usage?: Usage; label?: string; data_notice?: string | null };
type Turn = { id: number; question: string; reply?: Reply; error?: string; code?: string };
const examples = ['How is this server doing?', 'What changed in the last 20 minutes?', 'Was CPU high in the last 3 hours?', 'Son 20 dakikada sunucu nasıldı?'];

export default function ServerChat({ active, server, onConnect }: { active: boolean; server: string; onConnect: () => void }) {
  const [status, setStatus] = useState<Status | null>(null);
  const [statusError, setStatusError] = useState('');
  const [question, setQuestion] = useState('');
  const [turns, setTurns] = useState<Turn[]>([]);
  const [busy, setBusy] = useState(false);
  const sequence = useRef(0);
  const pending = useRef<AbortController | null>(null);
  const bottom = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  async function loadStatus() {
    setStatusError('');
    try { setStatus(await api<Status>('ai/chat/status')); }
    catch (error) { setStatusError((error as Error).message); }
  }
  useEffect(() => { if (active) void loadStatus(); }, [active]);
  useEffect(() => () => pending.current?.abort(), []);
  useEffect(() => { if (active && turns.length) bottom.current?.scrollIntoView({ block: 'nearest', behavior: 'instant' }); }, [turns, active]);

  async function send(text: string, retryId?: number) {
    text = text.trim();
    if (!text || busy || pending.current || statusError || !status?.connected || status.usage.remaining === 0) return;
    const id = retryId ?? ++sequence.current;
    const history = turns.filter(turn => turn.reply && !turn.reply.refused && turn.id !== retryId).slice(-6).map(turn => ({ question: turn.question, answer: turn.reply!.answer }));
    setBusy(true); setQuestion('');
    setTurns(current => retryId ? current.map(turn => turn.id === retryId ? { id, question: text } : turn) : [...current.slice(-39), { id, question: text }]);
    const controller = new AbortController(); pending.current = controller;
    const timeout = window.setTimeout(() => controller.abort(), 60000);
    try {
      const response = await fetch('/api/v1/ai/chat', { method: 'POST', credentials: 'same-origin', signal: controller.signal, headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ question: text, history }) });
      const result = await response.json();
      if (!response.ok) {
        setTurns(current => current.map(turn => turn.id === id ? { ...turn, error: result.error || 'The request could not be completed.', code: result.code } : turn));
        if (result.code === 'not_connected') setStatus(current => current && { ...current, connected: false });
      } else {
        setTurns(current => current.map(turn => turn.id === id ? { ...turn, reply: result } : turn));
        if (result.usage) setStatus(current => current && { ...current, usage: result.usage });
      }
      // Includes attempts that failed at the provider. Never retry automatically.
      void loadStatus();
    } catch (error) {
      if (!controller.signal.aborted || pending.current === controller) setTurns(current => current.map(turn => turn.id === id ? { ...turn, error: controller.signal.aborted ? 'The request timed out. Your provider may have counted this attempt. Check usage before retrying.' : 'Could not reach your server. Check your connection and try again.', code: 'network' } : turn));
      if (pending.current === controller) void loadStatus();
    } finally {
      window.clearTimeout(timeout); if (pending.current === controller) pending.current = null;
      setBusy(false);
    }
  }

  const ready = !!status?.connected && status.usage.remaining > 0 && !statusError;
  return <section className="server-chat" aria-label="Server AI chat">
    <div className="chat-toolbar">
      <div className="chat-scope"><span className="chat-server-mark"><ShieldCheck size={17} /></span><div><strong>{server}</strong><small>One server. Read-only insight.</small></div></div>
      <div className="chat-toolbar-actions"><span className={'chat-provider ' + (status?.connected ? 'is-connected' : '')}>{status?.connected ? <CheckCircle2 size={12} /> : <span className="dot offline" />}{status?.provider?.name || 'Your AI provider'} · {status?.connected ? 'Connected' : status ? 'Not connected' : 'Loading'}</span><button className="secondary compact" disabled={busy || !turns.length} onClick={() => { setTurns([]); setQuestion(''); input.current?.focus(); }}><Plus size={14} /> New chat</button></div>
    </div>
    {statusError ? <div className="notice error chat-status-error" role="alert"><span>{statusError}</span><button className="text-button" onClick={loadStatus}><RefreshCw size={13} /> Retry</button></div> : null}
    {status && !status.connected ? <div className="chat-connect-note"><Sparkles size={19} /><div><strong>{status.key_saved ? 'Check your saved AI connection.' : 'Bring your AI provider.'}</strong><p>{status.key_saved ? 'Verify your saved provider once to start chatting.' : 'Connect Grok or another supported provider to ask about this server.'} Your API key stays encrypted on this server.</p></div><button className="secondary compact" onClick={onConnect}>{status.key_saved ? 'Check connection' : 'Connect AI provider'}<ArrowUpRight size={13} /></button></div> : null}
    <div className={'chat-conversation ' + (!turns.length ? 'is-empty' : '')}>
      {!turns.length ? <div className="chat-welcome"><div className="chat-welcome-mark"><MessageSquare size={26} /><Sparkles size={13} /></div><span className="eyebrow">A CLEARER VIEW OF YOUR SERVER</span><h2>Ask about your server.</h2><p>Understand performance, spot changes, and explore the history you have. Ask in English or Turkish.</p><div className="chat-prompts">{examples.map((example, index) => <button key={example} disabled={!ready || busy} onClick={() => { setQuestion(example); input.current?.focus(); }}><span className="mono">0{index + 1}</span><span>{example}</span><ArrowUpRight size={14} /></button>)}</div><span className="chat-boundary"><ShieldCheck size={13} /> Monitoring context only · No server changes</span></div> : <div className="chat-transcript" role="log" aria-label="Conversation" aria-live="polite" aria-busy={busy}>{turns.map(turn => <article className="chat-turn" key={turn.id}>
        <div className="chat-question"><span className="chat-person">You</span><p>{turn.question}</p></div>
        <div className="chat-answer"><span className="chat-answer-mark"><Sparkles size={16} /></span><div className="chat-answer-content"><span className="chat-person">Server AI</span>{turn.reply ? <><p>{turn.reply.answer}</p>{turn.reply.data_notice ? <div className="chat-data-notice"><Clock3 size={12} /><span>{turn.reply.data_notice}</span></div> : null}<div className="chat-answer-meta">{turn.reply.refused ? <span>No provider call · Scope limited to this server</span> : <><span>{turn.reply.label || 'AI interpretation · verify important findings'}</span>{turn.reply.period ? <span><Clock3 size={11} /> {time(turn.reply.period.from)} – {time(turn.reply.period.to)}</span> : null}</>}</div></> : turn.error ? <div className="chat-failure" role="alert"><p>{turn.error}</p>{ready && !['daily_limit', 'not_connected'].includes(turn.code || '') ? <button className="text-button" disabled={busy} onClick={() => void send(turn.question, turn.id)}><RefreshCw size={12} /> Retry question</button> : null}</div> : <div className="chat-thinking" role="status"><span /><span /><span /><small>Reading server measurements…</small></div>}</div></div>
      </article>)}<div ref={bottom} /></div>}
    </div>
    <div className="chat-compose-area">
      {status?.usage.remaining === 0 ? <div className="chat-limit-note" role="status">Daily limit reached. Your 20 questions reset at 00:00 UTC.</div> : null}
      <form className="chat-composer" onSubmit={event => { event.preventDefault(); void send(question); }}>
        <label htmlFor="server-question" className="sr-only">Ask about this server</label>
        <textarea ref={input} id="server-question" rows={2} maxLength={1200} value={question} disabled={!ready || busy} placeholder={busy ? 'Waiting for your provider…' : 'Ask about this server…'} onChange={event => setQuestion(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send(question); } }} aria-describedby="chat-cost-note" />
        <div className="chat-composer-bottom"><span className="chat-auto-context"><Clock3 size={12} /> Time range from your question</span><span className="chat-character-count">{question.length}/1,200</span><button type="submit" className="chat-send" disabled={!ready || busy || !question.trim()} aria-label={busy ? 'Waiting for reply' : 'Send question'}><ArrowUp size={18} /></button></div>
      </form>
      <div className="chat-cost-note" id="chat-cost-note"><span>Each question uses your provider’s paid API. {status?.cost_notice ? 'Failed attempts count toward your daily limit.' : ''}</span><span>{status ? `${status.usage.remaining}/${status.usage.limit} questions left today · resets 00:00 UTC` : '20 questions per day · resets 00:00 UTC'}</span></div>
      <p className="chat-privacy-note">Monitoring summaries are shared when you send. This conversation stays in this page session.</p>
    </div>
  </section>;
}
