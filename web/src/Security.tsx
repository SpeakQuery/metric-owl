import AuthenticatorSetup from './AuthenticatorSetup';
import {useEffect, useState} from 'react';
import {ShieldCheck, LockKeyhole, Database, LogOut} from 'lucide-react';
import {api, bytes, time} from './lib';
import {MIN_PASSWORD_CHARACTERS, validatePassword} from './password';

type Props = {onLogout: () => void; onRecoveryChange: (pending: boolean) => void};

export default function Security({onLogout, onRecoveryChange}: Props) {
  const [data, setData] = useState<any>(null);
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [creds, setCreds] = useState({username: 'admin', password: '', code: ''});
  const [password, setPassword] = useState('');
  const [mfa, setMfa] = useState<{secret: string; uri: string} | null>(null);
  const [mfaCode, setMfaCode] = useState('');
  const [recovery, setRecovery] = useState<string[]>([]);
  const [confirmDisable, setConfirmDisable] = useState(false);
  const [busy, setBusy] = useState(false);
  const [quota, setQuota] = useState(1024);

  useEffect(() => {api('security').then(setData).catch(cause => setError(cause.message))}, []);
  useEffect(() => () => onRecoveryChange(false), [onRecoveryChange]);
  async function action(fn: () => Promise<void>) {
    setError(''); setMessage(''); setBusy(true);
    try {await fn()} catch (cause) {setError((cause as Error).message)}
    finally {setBusy(false)}
  }
  async function finishMfa() {
    // Stop the live stream before session revocation so recovery codes remain visible.
    onRecoveryChange(true);
    try {
      const result = await api('auth/mfa/finish', 'POST', {code: mfaCode});
      setRecovery(result.recovery_codes); setMfa(null); setMfaCode('');
    } catch (cause) {onRecoveryChange(false); throw cause}
  }

  if (!data) return <div className="empty">Loading security status…</div>;
  if (recovery.length) return <section className="panel form-panel"><h2>Save your recovery codes</h2><p>Two-factor authentication is enabled. All sessions have ended. Save these one-time codes before signing in again.</p><div className="recovery-codes">{recovery.map(value=><code key={value}>{value}</code>)}</div><button className="secondary" onClick={()=>action(async()=>{await navigator.clipboard.writeText(recovery.join('\n'));setMessage('Recovery codes copied.')})}>Copy recovery codes</button>{message&&<p role="status">{message}</p>}{error&&<p role="alert">{error}</p>}<button className="primary" onClick={()=>{setRecovery([]);onLogout()}}>Codes saved, continue to sign in</button></section>;
  const mfaEnabled = data.mfa_required;

  return <>
    {error&&<div className="notice error" role="alert">{error}</div>}
    {message&&<div className="notice success" role="status">{message}</div>}
    <div className="security-grid">{[[ShieldCheck,'Two-factor authentication',mfaEnabled?'Enabled · TOTP':'Optional · Not enabled'],[LockKeyhole,'Dashboard access',window.location.protocol==='https:'?'HTTPS · Encrypted':'Local / SSH'],[Database,'Storage budget',`${bytes(data.storage_bytes)} / ${bytes(data.quota_bytes)}`]].map(([Icon,title,value]:any)=><section className="panel security-tile" key={title}><Icon/><h3>{title}</h3><strong>{value}</strong></section>)}</div>
    {data.retention_warning&&<div className="notice warning">{data.retention_warning}</div>}
    {data.errors.map((value:string,index:number)=><div key={index} className="notice warning">{value}</div>)}
    <div className="two-col">
      <section className="panel form-panel"><h2>Verify sensitive actions</h2><p className="muted">Re-authentication lasts 5 minutes and protects API keys and security changes.</p><form onSubmit={event=>{event.preventDefault();action(async()=>{await api('auth/reauth','POST',creds);setCreds({...creds,password:'',code:''});setMessage('Identity verified. Sensitive settings are available for 5 minutes.')})}}><label>Username<input autoComplete="username" required value={creds.username} onChange={event=>setCreds({...creds,username:event.target.value})}/></label><label>Current password<input autoComplete="current-password" type="password" required value={creds.password} onChange={event=>setCreds({...creds,password:event.target.value})}/></label>{mfaEnabled&&<label>New verification code<input autoComplete="one-time-code" required value={creds.code} onChange={event=>setCreds({...creds,code:event.target.value})}/></label>}<button disabled={busy} className="primary">Verify my identity</button></form></section>
      <section className="panel form-panel"><h2>Password & sessions</h2><label>New password<input autoComplete="new-password" type="password" minLength={MIN_PASSWORD_CHARACTERS} value={password} onChange={event=>setPassword(event.target.value)} placeholder="At least 8 characters"/></label><button disabled={busy} className="secondary" onClick={()=>action(async()=>{validatePassword(password);await api('auth/password','PUT',{password});setPassword('');onLogout()})}>Change password and end all sessions</button><h3 className="spaced">Active sessions</h3>{data.sessions.map((session:any,index:number)=><div className="session-row" key={index}><LockKeyhole size={16}/><div>Started: {time(session.created)}<small>Last activity: {time(session.last)}</small></div></div>)}<button disabled={busy} className="secondary danger" onClick={()=>action(async()=>{await api('security/revoke-sessions','POST');onLogout()})}><LogOut size={16}/> End all sessions</button><small>Sessions expire after 30 minutes of inactivity or 12 hours in total.</small></section>
    </div>
    <section className="panel form-panel"><h2>{mfaEnabled?'Manage two-factor authentication':'Add an extra layer of security'}</h2><p className="muted">{mfaEnabled?'Your authenticator is enabled. You can replace it or turn off two-factor authentication after verifying your identity.':'Two-factor authentication is optional. Enable it to require a code from your authenticator app when signing in.'} Completing a change ends all sessions.</p>
      {!mfa&&<button disabled={busy} className="secondary" onClick={()=>action(async()=>{setConfirmDisable(false);setMfa(await api('auth/mfa/begin','POST',{}))})}>{mfaEnabled?'Set up new authenticator':'Enable authenticator'}</button>}
      {mfa&&<><AuthenticatorSetup secret={mfa.secret} uri={mfa.uri}/><label>Verification code from the new app<input autoComplete="one-time-code" value={mfaCode} onChange={event=>setMfaCode(event.target.value)}/></label><button disabled={busy} className="primary" onClick={()=>action(finishMfa)}>Enable new authenticator</button><button disabled={busy} className="secondary" onClick={()=>{setMfa(null);setMfaCode('')}}>Cancel authenticator setup</button></>}
      {mfaEnabled&&!mfa&&!confirmDisable&&<button disabled={busy} className="secondary danger" onClick={()=>setConfirmDisable(true)}>Turn off two-factor authentication</button>}
      {confirmDisable&&<div className="notice warning"><p>Future sign-ins will use your username and password only. All current sessions will end.</p><button disabled={busy} className="secondary danger" onClick={()=>action(async()=>{await api('auth/mfa/disable','POST',{});onLogout()})}>Confirm turn off</button><button disabled={busy} className="secondary" onClick={()=>setConfirmDisable(false)}>Keep two-factor authentication</button></div>}
    </section>
    <section className="panel form-panel"><h2>Data retention</h2><label>Storage budget (MiB)<input type="number" min="64" max="10240" value={quota} onChange={event=>setQuota(+event.target.value)}/></label><button disabled={busy} className="secondary" onClick={()=>action(async()=>{const result=await api('settings');await api('settings','PUT',{settings:{...result.settings,quota_bytes:quota*1024*1024}});setMessage('Storage quota updated. It will apply at the next quota check.')})}>Save quota</button><div className="retention-flow">{[['0–24 hours','1 minute'],['1–7 days','5 minutes'],['7–90 days','Hourly'],['90–365 days','Daily']].map(([period,resolution])=><div key={period}><span>{period}</span><strong>{resolution}</strong></div>)}</div><p className="muted">Event details are retained for 30 days; event summaries for 1 year. The storage quota may shorten retention. Source records are removed only after summaries are safely saved.</p></section>
  </>;
}
