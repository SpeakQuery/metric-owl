// Compiled UI acceptance with synthetic data and intercepted API requests.
// Run from the repository root: node web/tests/sources-ui.mjs
// No credentials, external probes, job execution or production writes occur.
import { chromium, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { readFile, mkdir } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import assert from 'node:assert/strict';

const root = resolve('web/dist');
const output = resolve('artifacts/screenshots');
const mime = { '.js': 'text/javascript', '.css': 'text/css', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.png': 'image/png', '.html': 'text/html' };
const server = createServer(async (request, response) => {
  const pathname = new URL(request.url, 'http://localhost').pathname;
  const assetPath = pathname.startsWith('/admin/assets/') ? pathname.slice('/admin'.length) : pathname;
  const file = pathname === '/admin' || pathname === '/admin/' ? resolve(root, 'index.html') : resolve(root, '.' + assetPath);
  if (!file.startsWith(root + '/')) { response.writeHead(404).end(); return; }
  try { response.setHeader('Content-Type', mime[extname(file)] || 'application/octet-stream'); response.end(await readFile(file)); }
  catch { response.writeHead(404).end(); }
});
await new Promise(done => server.listen(0, '127.0.0.1', done));
const base = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch();
const id = '184a8478-5bcf-4230-9378-7fdb017ad141';
const tlsId = '184a8478-5bcf-4230-9378-7fdb017ad142';
const jobOptions = { job_interval_seconds: 86400, job_grace_seconds: 3600, job_max_duration_seconds: 3600, job_added_at: 100 };
let targets = [
  { id, kind: 'job', name: 'Nightly database backup', address: `/var/lib/server-guard/job-reports/${id}.json`, enabled: true, options: jobOptions },
  { id: tlsId, kind: 'tls', name: 'Application certificate', address: 'https://app.example.com', enabled: true },
];
let jobState = 'running', rejectSave = false, writes = 0, failTargets = false;
const errors = [], externalRequests = [];
let page;
try {
  page = await browser.newPage({ viewport: { width: 1440, height: 1060 }, permissions: ['clipboard-read', 'clipboard-write'] });
  page.setDefaultTimeout(8000);
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (!request.url().startsWith(base)) externalRequests.push(request.url()); });
  await page.addInitScript(() => {
    localStorage.setItem('guard-guide-dismissed', '1');
    const reading = value => ({ value, unit: '%', quality: 'good', reason: null });
    window.EventSource = class extends EventTarget {
      constructor() { super(); this.timer = setInterval(() => this.dispatchEvent(new MessageEvent('sample', { data: JSON.stringify({ source_id: 'fixture', at: Date.now() / 1000, uptime: 86400, cpu: reading(23), cores: [24, 22], memory: reading(42), memory_used: 3.36 * 1024 ** 3, memory_total: 8 * 1024 ** 3, network_in: reading(3200), network_out: reading(1200), disks: [{ id: '/', mount: '/', total: 100 * 1024 ** 3, available: 68 * 1024 ** 3, used_percent: reading(32) }], diagnostics: [
          ['host', 'swap_used', 'Swap used', 256 * 1024 ** 2, 'B'], ['host', 'swap_total', 'Swap capacity', 2 * 1024 ** 3, 'B'], ['host', 'swap_used_percent', 'Swap usage', 12.5, '%'], ['host', 'memory_pressure', 'Memory pressure · PSI some avg10', 2.4, '%'], ['host', 'cpu_iowait', 'CPU I/O wait', 3.2, '%'], ['host', 'oom_kills_5m', 'Observed OOM kills · last 5 minutes', 0, 'count'], ['/dev/vda', 'disk_io_latency', 'Disk read/write latency', null, 'ms'], ['/dev/vda', 'disk_io_utilization', 'Disk I/O busy time', 0, '%'], ['/', 'inode_used', 'Filesystem inodes used', 21.5, '%']
        ].map(([source, metric, label, value, unit]) => ({ source, metric, label, reading: { value, unit, quality: value == null ? 'unavailable' : 'ok', reason: value == null ? 'No completed read/write operations in this interval' : null } })), processes: [], agent_memory: 20 * 1024 ** 2, agent_cpu: .4 }) })), 150); }
      close() { clearInterval(this.timer); }
    };
  });
  await page.route('**/api/v1/**', async route => {
    const path = new URL(route.request().url()).pathname.replace('/api/v1/', '');
    const respond = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    if (path === 'targets') {
      if (route.request().method() === 'PUT') {
        writes++;
        if (rejectSave) return respond({ error: 'Monitor settings could not be saved. Try again.' }, 503);
        targets = route.request().postDataJSON().map(target => target.kind === 'job' && !target.address ? { ...target, address: `/var/lib/server-guard/job-reports/${target.id}.json`, options: { ...target.options, job_added_at: 100 } } : target);
        return respond({ ok: true, targets });
      }
      if (failTargets) return respond({ error: 'Fixture read unavailable' }, 503);
      return respond({ targets, status: [
        { id, kind: 'job', name: 'Nightly database backup', at: Date.now() / 1000, status: jobState, detail: jobState === 'running' ? 'Job is running; runtime is within its configured limit.' : 'Waiting for the first successful job report.', measurements: { job_failure: 0 } },
        { id: tlsId, kind: 'tls', name: 'Application certificate', at: Date.now() / 1000, status: 'unavailable', detail: 'Certificate verification failed; expiry could not be verified.', measurements: { tls_failure: 1 } },
      ] });
    }
    const data = {
      'auth/status': { configured: true, session: { csrf: 'fixture-csrf' } },
      host: { id: 'fixture', name: 'production-01', os: 'Ubuntu 24.04 LTS', cpu_model: 'Virtual CPU', cores: 2, memory_total: 8 * 1024 ** 3, version: '0.1.0' },
      incidents: { items: [] }, forecasts: [{ source: '/', days: null }],
      settings: { settings: { recipient: '', recipient_verified: false, ai_model: '', webhook_url: '' }, keys: { ai: false, relay: false, webhook: false } },
      rules: [], 'mcp/clients': [],
      'mcp/connection': { command: '/usr/local/bin/server-guard', args: ['--data-dir', '/var/lib/server-guard', 'mcp'], key_variable: 'GUARD_MCP_KEY' },
      inventory: { sites: [{ hostname: 'app.example.com', web_server: 'Nginx', config_file: '/etc/nginx/sites-enabled/application.conf', wildcard: false }, { hostname: '*.example.com', web_server: 'Nginx', config_file: '/etc/nginx/sites-enabled/wildcard.conf', wildcard: true }], services: [{ name: 'Nginx', kind: 'web', process_count: 3, cpu_percent: 1.8, memory_rss_bytes: 48 * 1024 ** 2, memory_note: 'Sum of process RSS; shared pages may be counted more than once.', database_details: null }, { name: 'PostgreSQL', kind: 'database', process_count: 7, cpu_percent: 8.2, memory_rss_bytes: 540 * 1024 ** 2, memory_note: 'Sum of process RSS; shared pages may be counted more than once.', database_details: 'Engine detected from running processes. Database names, sizes and connection counts require a separately configured read-only database connection; none is configured.' }], site_notes: ['Configuration declarations are suggestions. Enable monitoring explicitly.'], sites_checked_at: Date.now() / 1000, services_checked_at: Date.now() / 1000 },
    }[path];
    return respond(data ?? { error: `Unexpected fixture endpoint: ${path}` }, data === undefined ? 404 : 200);
  });
  await mkdir(output, { recursive: true });
  await page.goto(base + '/admin');
  await page.getByRole('button', { name: 'Applications & storage', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Hosted websites' })).toBeVisible();
  await expect(page.getByText('Job running', { exact: true })).toBeVisible();
  await expect(page.locator('.source-selected').getByText('Unavailable', { exact: true })).toBeVisible();
  await expect(page.locator('.source-selected').getByText('Failed', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Bottleneck diagnostics' })).toBeVisible();
  await expect(page.locator('.source-diagnostic-card').filter({ hasText: 'memory_pressure' })).toContainText('2.4%');
  await page.locator('.source-storage-diagnostics > summary').click();
  await expect(page.locator('.source-diagnostic-card').filter({ hasText: 'disk_io_latency' })).toContainText('No completed read/write operations');
  await expect(page.locator('.source-diagnostic-card').filter({ hasText: 'inode_used' })).toContainText('21.5%');
  await expect(page.getByText('68.0 GiB free / 100.0 GiB', { exact: false })).toBeVisible();
  const wildcard = page.getByRole('row').filter({ hasText: '*.example.com' });
  await expect(wildcard.getByRole('button', { name: 'Configure HTTP' })).toBeDisabled();
  await expect(wildcard.getByRole('button', { name: 'Configure TLS' })).toBeDisabled();
  await page.getByRole('row').filter({ hasText: 'app.example.com' }).getByRole('button', { name: 'Configure HTTP' }).click();
  await expect(page.getByLabel('Health check URL')).toHaveValue('https://app.example.com/');
  assert.equal(writes, 0, 'Discovery configuration must not start checks or save implicitly');
  await page.getByLabel('Inspect a JSON response').check();
  await page.getByLabel('JSON pointer').fill('');
  await page.getByLabel('Require an exact JSON value').check();
  await page.getByLabel('Expected JSON value').fill('not-json');
  await page.getByRole('button', { name: 'Add to monitoring' }).click();
  await expect(page.getByRole('alert')).toContainText('valid JSON value');
  assert.equal(writes, 0);
  await expect(page.getByLabel('Display name')).toHaveValue('app.example.com health');
  await page.getByLabel('Expected JSON value').fill('true');
  await page.getByLabel('Expected HTTP status').fill('200');
  await page.getByLabel('Maximum response time').fill('800');
  rejectSave = true;
  await page.getByRole('button', { name: 'Add to monitoring' }).click();
  await expect(page.getByRole('alert')).toContainText('could not be saved');
  await expect(page.getByRole('heading', { name: 'Verify your identity' })).toHaveCount(0);
  await expect(page.getByLabel('Display name')).toHaveValue('app.example.com health');
  rejectSave = false;
  await page.getByRole('button', { name: 'Add to monitoring' }).click();
  await expect(page.getByRole('heading', { name: 'app.example.com health', exact: true })).toBeVisible();
  assert.equal(targets.at(-1).options.json_pointer, '');
  assert.equal(targets.at(-1).options.json_expected, 'true');
  assert.equal(targets.at(-1).options.expected_status, 200);
  await page.getByLabel('Source type').selectOption('job');
  await page.getByLabel('Display name').fill('Hourly export');
  await page.getByRole('button', { name: 'Hourly', exact: true }).click();
  await page.getByRole('button', { name: 'Add to monitoring' }).click();
  const addedJob = targets.at(-1);
  assert.equal(addedJob.options.job_interval_seconds, 3600);
  const jobCard = page.locator('.source-monitor').filter({ has: page.getByRole('heading', { name: 'Hourly export', exact: true }) });
  await expect(jobCard.locator('.source-address')).toHaveText(`/var/lib/server-guard/job-reports/${addedJob.id}.json`);
  await jobCard.locator('summary').click();
  await expect(jobCard.locator('.code-block')).toContainText('runuser -u server-guard --');
  await expect(jobCard.locator('.code-block')).toContainText(`'--id' '${addedJob.id}' '--state' 'success'`);
  await jobCard.getByRole('button', { name: 'When it starts' }).click();
  await expect(jobCard.locator('.code-block')).toContainText("'--state' 'running'");
  const beforeCopy = writes;
  await jobCard.getByRole('button', { name: 'Copy command' }).click();
  await expect(jobCard.getByRole('button', { name: 'Command copied' })).toBeVisible();
  assert.equal(writes, beforeCopy, 'Copy must not execute jobs or mutate monitoring');
  await page.getByRole('button', { name: 'Hourly export pause', exact: true }).click();
  await expect(jobCard.getByText('Paused', { exact: true })).toBeVisible();
  await expect(jobCard.getByRole('button', { name: 'Command copied' })).toBeDisabled();
  jobState = 'pending';
  await page.getByRole('button', { name: 'Refresh results' }).click();
  await expect(page.getByText('Waiting for report', { exact: true })).toBeVisible();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: `${output}/sources-dark.png`, fullPage: true, animations: 'disabled' });
  await page.getByRole('button', { name: 'Switch theme', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.screenshot({ path: `${output}/sources-light.png`, fullPage: true, animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, 'Mobile page must not overflow horizontally');
  await page.screenshot({ path: `${output}/sources-mobile.png`, fullPage: true, animations: 'disabled' });
  assert.deepEqual(errors, []);
  assert.deepEqual(externalRequests, []);
  // A failed initial read must not turn an empty local list into a destructive replacement.
  failTargets = true;
  await page.reload();
  await page.getByRole('button', { name: 'Open menu' }).click();
  await page.getByRole('button', { name: 'Applications & storage', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Could not refresh monitors');
  await expect(page.getByRole('button', { name: 'Add to monitoring' })).toBeDisabled();
  console.log('Sources UI acceptance passed: discovery consent, JSON validation, retained re-auth form, normalized managed jobs, report command copy, paused/pending/unavailable states, dark/light/mobile, and failed-read write protection.');
} catch (error) { if (page) await page.screenshot({ path: `${output}/sources-failure.png`, fullPage: true }).catch(() => {}); throw error; } finally { await browser.close(); server.close(); }
