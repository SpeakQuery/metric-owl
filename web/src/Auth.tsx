import Brand from './Brand';
import AuthenticatorSetup from './AuthenticatorSetup';
import {useState} from 'react';
import {ShieldCheck, ArrowRight, Check, Copy, LockKeyhole, Sparkles} from 'lucide-react';
import {api, setCsrf} from './lib';
import {MIN_PASSWORD_CHARACTERS, validatePassword} from './password';

type Step = 'setup' | 'enroll' | 'totp' | 'recovery' | 'login';
type Props = {configured: boolean; enrollmentReady?: boolean; mfaRequired?: boolean; onLogin: () => void};

export default function Auth({configured, enrollmentReady = false, mfaRequired = true, onLogin}: Props) {
  const [step, setStep] = useState<Step>(configured ? 'login' : enrollmentReady ? 'enroll' : 'setup');
  const [user, setUser] = useState('admin');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [key, setKey] = useState('');
  const [code, setCode] = useState('');
  const [totp, setTotp] = useState<{secret: string; uri: string} | null>(null);
  const [codes, setCodes] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  async function submit(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError('');
    try {
      if (step === 'enroll') {
        const result = await api('auth/setup/enroll', 'POST', {username: user, password});
        setTotp({secret: result.secret, uri: result.uri});
        setKey(result.key); setPassword(''); setStep('totp');
      } else if (step === 'setup') {
        validatePassword(password);
        if (password !== confirmPassword) throw new Error('Passwords do not match. Enter the same password in both fields.');
        setTotp(await api('auth/setup/begin', 'POST', {key, username: user, password}));
        setPassword(''); setConfirmPassword(''); setStep('totp');
      } else if (step === 'totp') {
        const result = await api('auth/setup/finish', 'POST', {key, code});
        setCodes(result.recovery_codes); setTotp(null); setKey(''); setCode(''); setStep('recovery'); onLogin();
      } else {
        const result = await api('auth/login', 'POST', {username: user, password, code});
        setCsrf(result.session.csrf); setPassword(''); setCode(''); onLogin();
      }
    } catch (cause) {setError((cause as Error).message)}
    finally {setBusy(false)}
  }

  const heading = step === 'setup' ? 'Put your guard on duty' : step === 'enroll' ? 'Finish your account setup' : step === 'totp' ? 'Your second layer of security' : step === 'recovery' ? 'Your recovery codes' : 'Welcome back';
  const introduction = step === 'setup' ? 'First-time setup. Choose a username and create a new password.' : step === 'enroll' ? 'Use the username and password you chose during installation. Next, connect your authenticator.' : step === 'totp' ? 'Scan the QR code with your phone, then enter the code from your authenticator.' : step === 'recovery' ? 'Save these codes before you continue.' : mfaRequired ? 'Secure access with your password and authenticator app.' : 'Sign in with the username and password you chose during installation.';

  return <div className="auth-layout"><div className="auth-story"><Brand/><div className="auth-mobile-ai"><Sparkles size={13}/>AI-assisted monitoring<span>Optional insights</span></div><div className="auth-pitch"><span className="eyebrow">PEACE OF MIND FOR YOUR SERVER</span><h1>Everything.<br/>Under control.</h1><p>Real-time metrics. Clear alerts.<br/>A watchful companion for your server.</p><figure className="auth-illustration"><img src={`${import.meta.env.BASE_URL}assets/ai-server-guardian.png`} width="1536" height="1024" alt="An AI guardian watching a computer and server health metrics"/><figcaption><Sparkles size={13}/><span>AI-assisted monitoring</span><small>Optional insights</small></figcaption></figure></div><div className="auth-foot"><LockKeyhole size={15}/>Your data stays on your server. Read-only monitoring on your machine.</div></div><main className="auth-card"><span className="eyebrow">SECURE CONTROL CENTER</span><h2>{heading}</h2><p className="muted">{introduction}</p>{error&&<div role="alert" className="notice error">{error}</div>}
    {step === 'recovery' ? <><p>Store these one-time codes somewhere safe. Use one to sign in if you lose access to your authenticator.</p><div className="recovery-codes">{codes.map(value=><code key={value}>{value}</code>)}</div><button className="secondary" onClick={()=>navigator.clipboard.writeText(codes.join('\n'))}><Copy size={16}/>Copy codes</button><button className="primary full" onClick={()=>{setCodes([]); setStep('login')}}><Check size={16}/>Saved, continue to sign in</button></> : <form onSubmit={submit}>
      {step === 'setup' && <label>Setup key<input aria-label="Setup key" autoComplete="off" type="password" required value={key} onChange={event=>setKey(event.target.value)}/><small>For manual installations only: paste the one-time key shown by the installer.</small></label>}
      {step !== 'totp' && <><label>Username<input autoComplete="username" value={user} required onChange={event=>setUser(event.target.value)}/></label><label>{step==='setup'?'Create password':'Password'}<input aria-label={step==='setup'?'Create password':'Password'} autoComplete={step==='setup'?'new-password':'current-password'} type="password" minLength={MIN_PASSWORD_CHARACTERS} required value={password} onChange={event=>setPassword(event.target.value)}/>{step==='setup'&&<small>Choose a new password with at least 8 characters.</small>}</label>{step==='setup'&&<label>Confirm password<input aria-label="Confirm password" autoComplete="new-password" type="password" minLength={MIN_PASSWORD_CHARACTERS} required value={confirmPassword} onChange={event=>setConfirmPassword(event.target.value)}/><small>Enter your new password again to confirm it.</small></label>}</>}
      {step==='totp'&&totp&&<AuthenticatorSetup secret={totp.secret} uri={totp.uri} account={user}/>}
      {(step==='totp'||(step==='login'&&mfaRequired))&&<label>{step==='totp'?'Authenticator code':'Verification or recovery code'}<input aria-label={step==='totp'?'Authenticator code':'Verification or recovery code'} autoComplete="one-time-code" required value={code} onChange={event=>setCode(event.target.value)} placeholder="6-digit verification code"/><small>Use a fresh code. Previously used TOTP codes are not accepted again.</small></label>}
      <button disabled={busy} className="primary full" type="submit">{busy?'Verifying…':step==='setup'?'Create account':step==='enroll'?'Continue to authenticator':step==='totp'?'Verify and finish':'Secure sign in'}<ArrowRight size={18}/></button>
      {step==='totp'&&<button type="button" className="text-button full" onClick={()=>{setTotp(null);setKey('');setCode('');setError('');setStep(enrollmentReady?'enroll':'setup')}}>Start this step again</button>}
    </form>}
    <div className="auth-protection"><ShieldCheck size={16}/>Argon2id · {mfaRequired?'Two-factor authentication enabled':'Optional two-factor authentication'} · {window.location.protocol==='https:'?'HTTPS access':'Local access'}</div>
  </main></div>;
}
