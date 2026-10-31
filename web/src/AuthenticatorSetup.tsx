import {useState} from 'react';
import {QRCodeSVG} from 'qrcode.react';
import {Check, Copy, KeyRound, ScanLine} from 'lucide-react';

type Props = {secret: string; uri: string; account?: string};

/** The QR is generated entirely in this page. The secret never goes to a QR service. */
export default function AuthenticatorSetup({secret, uri, account}: Props) {
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const validUri = uri.startsWith('otpauth://totp/');
  return <section className="totp-box authenticator-setup" aria-label="Connect your authenticator">
    <div className="authenticator-title"><ScanLine size={20}/><strong>Scan with your authenticator</strong></div>
    <p>Open Google Authenticator, Microsoft Authenticator, Apple Passwords, or another app that supports verification codes. Choose <strong>Add account</strong> or <strong>Set up code</strong>, then scan this QR code.</p>
    {validUri && <div className="authenticator-qr"><QRCodeSVG value={uri} size={216} level="M" marginSize={4} bgColor="#ffffff" fgColor="#111111" title="Authenticator setup QR code" role="img" aria-label="Authenticator setup QR code"/></div>}
    {account && <small className="authenticator-account">Account: <strong>{account}</strong></small>}
    <p className="authenticator-confirm">Enter the six-digit code from your app below to finish. Keep the QR code and setup key private.</p>
    {validUri && <><a className="secondary authenticator-open" href={uri}><KeyRound size={15}/> Open in an authenticator app</a><small>On this device, a compatible app may open. If it does not, scan the QR code or enter the key manually.</small></>}
    <details className="authenticator-manual"><summary>Can’t scan? Enter the setup key</summary><p>Choose a time-based account in your app and enter this key.</p><code>{secret}</code><button type="button" className="text-button" onClick={async()=>{try{await navigator.clipboard.writeText(secret);setCopied(true);setCopyError(false)}catch{setCopyError(true)}}}>{copied?<Check size={15}/>:<Copy size={15}/>} {copied?'Key copied':'Copy setup key'}</button>{copyError&&<small role="alert">Copy is unavailable. Select the key above and copy it manually.</small>}</details>
  </section>;
}
