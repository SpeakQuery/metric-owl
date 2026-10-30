export function OwlMark({size=30}:{size?:number}) {
  return <svg width={size} height={size} viewBox="0 0 40 40" fill="none" aria-hidden="true">
    <path d="M7 8 14 12Q20 9 26 12L33 8V24Q32 33 20 36Q8 33 7 24Z" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round"/>
    <path d="M10 24Q12 30 17 32M30 24Q28 30 23 32" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" opacity=".5"/>
    <circle cx="14" cy="20" r="6" stroke="currentColor" strokeWidth="1.7"/>
    <circle cx="26" cy="20" r="6" stroke="currentColor" strokeWidth="1.7"/>
    <circle cx="14" cy="20" r="2.1" fill="currentColor"/>
    <circle cx="26" cy="20" r="2.1" fill="currentColor"/>
    <path d="m17.5 26 2.5 3 2.5-3" fill="var(--ai)"/>
    <path d="M20 4v4M18 6h4" stroke="var(--ai)" strokeWidth="1.4" strokeLinecap="round"/>
  </svg>
}

export default function Brand() {
  return <div className="brand" aria-label="Metric Owl — AI-assisted server monitoring">
    <div className="brand-mark"><OwlMark/></div>
    <div className="brand-copy"><div className="brand-name">Metric <strong>Owl</strong><span className="brand-ai">AI</span></div>
      <small>AI-ASSISTED SERVER MONITORING</small></div>
  </div>
}
