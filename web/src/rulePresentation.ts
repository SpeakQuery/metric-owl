import type { Rule } from './generated/Rule';
import type { Sample } from './generated/Sample';
import type { Target } from './generated/Target';
import { bytes } from './lib';

type Metric = { label: string; help: string; unit: string; scale?: number; source: 'host' | 'disk' | 'device' | 'http' | 'service' | 'tls' | 'job' };
export const metrics: Record<string, Metric> = {
  cpu: { label: 'CPU usage', unit: '%', source: 'host', help: 'How busy the server’s processor is. Sustained high usage can make websites and applications slow.' },
  memory: { label: 'Memory usage', unit: '%', source: 'host', help: 'How much working memory (RAM) is in use. When it fills up, applications can slow down or stop.' },
  disk_used: { label: 'Disk usage', unit: '%', source: 'disk', help: 'How much storage is used on this filesystem. A full disk can stop uploads, logs and database writes.' },
  disk_free: { label: 'Free disk space', unit: 'GiB', scale: 1024 ** 3, source: 'disk', help: 'How much room remains for new files. 1 GiB is about one billion bytes of storage.' },
  network_in: { label: 'Incoming traffic', unit: 'MiB/s', scale: 1024 ** 2, source: 'host', help: 'Data arriving at the server each second. A high value can mean busy traffic or a large download.' },
  network_out: { label: 'Outgoing traffic', unit: 'MiB/s', scale: 1024 ** 2, source: 'host', help: 'Data leaving the server each second, such as website responses or backups.' },
  http: { label: 'Website check failure', unit: 'failure state', source: 'http', help: '1 means the configured website check failed; 0 means it passed. Configure the website first in Applications & storage.' },
  service: { label: 'Service check failure', unit: 'failure state', source: 'service', help: '1 means a monitored service is not running; 0 means it is running. Choose an existing service monitor.' },
  tls_days: { label: 'Certificate time remaining', unit: 'days', source: 'tls', help: 'Days until a website’s HTTPS certificate expires. An expired certificate can prevent visitors from opening the site.' },
  tls_failure: { label: 'Certificate check failure', unit: 'failure state', source: 'tls', help: '1 means the HTTPS certificate could not be verified; 0 means verification passed.' },
  job: { label: 'Scheduled job failure', unit: 'failure state', source: 'job', help: '1 means a job report failed, is overdue or exceeded its runtime limit; 0 means its configured checks passed.' },
  swap_used: { label: 'Swap space used', unit: 'GiB', scale: 1024 ** 3, source: 'host', help: 'Disk space used as extra memory. Some usage is normal; combine this with memory stalls before concluding the server needs more RAM.' },
  swap_total: { label: 'Swap capacity', unit: 'GiB', scale: 1024 ** 3, source: 'host', help: 'Total disk space reserved as extra memory. This is capacity, not current usage.' },
  swap_used_percent: { label: 'Swap usage', unit: '%', source: 'host', help: 'The share of configured swap in use. Swap can stay occupied after memory pressure has passed.' },
  memory_pressure: { label: 'Memory stalls', unit: '%', source: 'host', help: 'Time applications spend waiting for memory. High values indicate delays, rather than just a large amount of RAM in use.' },
  cpu_iowait: { label: 'CPU waiting for storage', unit: '%', source: 'host', help: 'Time the processor spends waiting for disk operations. High values can help explain slow applications.' },
  oom_kills_5m: { label: 'Apps stopped by low memory', unit: 'stops', source: 'host', help: 'Operating-system kills observed in the last 5 minutes because memory ran out. A value above 0.5 means at least one observed stop.' },
  disk_io_latency: { label: 'Disk response time', unit: 'ms', source: 'device', help: 'Average time a completed disk operation takes. Longer delays can slow databases and file access. Idle disks have no latency reading.' },
  disk_io_utilization: { label: 'Disk busy time', unit: '%', source: 'device', help: 'How much time a storage device is busy. Being busy alone does not prove the disk is overloaded.' },
  inode_used: { label: 'File entry capacity', unit: '%', source: 'disk', help: 'The share of available file entries (inodes) already used. Too many small files can prevent new files, even when storage space remains.' },
};
export const comparison: Record<string, string> = { '>': 'above', '>=': 'at or above', '<': 'below', '<=': 'at or below' };
const number = new Intl.NumberFormat('en-GB', { maximumFractionDigits: 2 });
export function secondsLabel(seconds: number): string {
  if (seconds >= 3600 && seconds % 3600 === 0) return `${seconds / 3600} hour${seconds === 3600 ? '' : 's'}`;
  if (seconds >= 60) { const minutes = Math.floor(seconds / 60); const rest = Math.round((seconds % 60) * 100) / 100; return `${minutes} minute${minutes === 1 ? '' : 's'}${rest ? ' ' + number.format(rest) + ' seconds' : ''}`; }
  return `${number.format(seconds)} seconds`;
}
export function thresholdLabel(rule: Rule, value = rule.threshold) {
  const metric = metrics[rule.metric];
  if (metric?.unit === 'GiB') return bytes(value);
  return `${number.format(value / (metric?.scale || 1))}${metric?.unit === '%' ? '%' : ' ' + (metric?.unit || '')}`.trim();
}
export function conditionText(rule: Rule) {
  return `${metrics[rule.metric]?.label || rule.metric} ${comparison[rule.operator] || rule.operator} ${thresholdLabel(rule)}`;
}
export function timingText(rule: Rule) {
  return rule.fraction === 1 ? `For ${secondsLabel(rule.window_seconds)}` : `For at least ${secondsLabel(rule.window_seconds * rule.fraction)} within ${secondsLabel(rule.window_seconds)}`;
}
export function sourceLabel(source: string) {
  if (source === 'host') return 'This server';
  if (source === '/') return 'Main disk (/)';
  if (source === '/tmp' || source === '/var/tmp') return `Temporary files (${source})`;
  if (source === '/boot' || source === '/boot/efi') return `Startup files (${source})`;
  if (source === '/var/lib/server-guard') return 'Monitoring data (/var/lib/server-guard)';
  return source;
}
export function sourceHelp(rule: Rule) {
  if (metrics[rule.metric]?.source !== 'disk') return '';
  if (['/tmp', '/var/tmp'].includes(rule.source)) return 'Temporary-file storage, not temperature. This checks the entire filesystem mounted here, not a folder-size limit. Several paths can share the same disk.';
  return 'This checks the filesystem mounted at this path. It is not a size limit for an individual folder; several paths can share the same disk.';
}
export function recoveryFor(rule: Rule, threshold: number, operator = rule.operator) {
  const existingGap = Math.abs(rule.threshold - rule.recovery_threshold);
  const gap = existingGap > 0 ? existingGap : Math.max(Math.abs(threshold) * .05, .1);
  return threshold + (operator.startsWith('>') ? -gap : gap);
}
export function newRule(metric = 'cpu', source = 'host'): Rule {
  const low = ['disk_free', 'tls_days'].includes(metric);
  const failure = ['http', 'service', 'job', 'tls_failure', 'oom_kills_5m'].includes(metric);
  const threshold = metric === 'disk_free' ? 1024 ** 3 : metric === 'tls_days' ? 14 : failure ? .5 : metric === 'disk_used' ? 85 : metric === 'memory' ? 90 : 80;
  return { id: crypto.randomUUID(), name: metric === 'cpu' ? 'New CPU rule' : `New ${metrics[metric]?.label.toLowerCase() || metric} alert`, source, metric,
    operator: low ? '<' : '>', threshold, window_seconds: metric.startsWith('disk_') ? 30 : 300, fraction: 1,
    severity: 'warning', recovery_threshold: low ? threshold * 1.3 : failure ? .1 : threshold - 5,
    recovery_seconds: metric.startsWith('disk_') ? 30 : 120, enabled: true, notify: true };
}
export function sourceOptions(metric: string, sample: Sample | null, targets: Target[], rules: Rule[]) {
  const kind = metrics[metric]?.source;
  const options = new Map<string, string>();
  if (kind === 'host') options.set('host', 'This server');
  if (kind === 'disk') for (const disk of sample?.disks || []) options.set(disk.id, sourceLabel(disk.mount));
  if (kind === 'device' || metric === 'inode_used') for (const diagnostic of sample?.diagnostics || []) if (diagnostic.metric === metric) options.set(diagnostic.source, sourceLabel(diagnostic.source));
  for (const target of targets) if (target.kind === kind) options.set(target.id, `${target.name}${target.enabled ? '' : ' (monitor paused)'}`);
  for (const rule of rules) if (rule.metric === metric && !options.has(rule.source)) options.set(rule.source, sourceLabel(rule.source));
  return [...options].map(([value, label]) => ({ value, label }));
}

// Basic is a view of saved rules, not a separate engine or an automatic reset.
export function basicRules(rules: Rule[], sample: Sample | null) {
  const root = rules.some(r => r.metric === 'disk_used' && r.source === '/') ? '/' : sample?.disks.find(disk => disk.mount === '/')?.id || sample?.disks[0]?.id || rules.find(r => r.metric === 'disk_used')?.source;
  return ['cpu', 'memory', 'disk_used'].flatMap(metric => ['warning', 'critical'].map(severity => {
    const source = metric === 'disk_used' ? root : 'host';
    const match = (r: Rule) => r.metric === metric && r.source === source && r.severity === severity;
    return { metric, severity, source, rule: rules.find(r => r.id === `${metric}-${severity}` && match(r)) || rules.find(match) };
  }));
}
