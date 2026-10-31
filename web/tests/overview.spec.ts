import { test, expect, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import type { Rule } from '../src/generated/Rule';

const defaults: Rule[] = [
  { id: 'cpu-warning', name: 'High CPU', source: 'host', metric: 'cpu', operator: '>', threshold: 80, window_seconds: 300, fraction: .8, severity: 'warning', recovery_threshold: 75, recovery_seconds: 120, enabled: true, notify: true },
  { id: 'cpu-critical', name: 'Critical CPU', source: 'host', metric: 'cpu', operator: '>', threshold: 90, window_seconds: 180, fraction: .8, severity: 'critical', recovery_threshold: 85, recovery_seconds: 120, enabled: true, notify: true },
  { id: 'memory-warning', name: 'High memory', source: 'host', metric: 'memory', operator: '>', threshold: 90, window_seconds: 300, fraction: .8, severity: 'warning', recovery_threshold: 85, recovery_seconds: 120, enabled: true, notify: true },
  { id: 'disk-warning', name: 'Disk space /', source: '/', metric: 'disk_used', operator: '>=', threshold: 85, window_seconds: 30, fraction: 1, severity: 'warning', recovery_threshold: 82, recovery_seconds: 30, enabled: true, notify: true },
];

// UI fixtures only. Default creation and persistence are covered by Rust tests.
async function fixture(page: Page, rules = defaults, failRules = false) {
  const writes: string[] = [];
  await page.route('**/api/v1/**', async route => {
    const path = new URL(route.request().url()).pathname.replace('/api/v1/', '');
    if (route.request().method() !== 'GET') writes.push(path);
    const readings = (value: number, unit = '%') => ({ value, unit, quality: 'good', reason: null });
    const data: Record<string, unknown> = {
      'auth/status': { configured: true, mfa_required: false, session: { csrf: 'synthetic-test-csrf' } },
      host: { id: 'fixture-host', name: 'Example server · UI test', os: 'Ubuntu · illustrative data', cpu_model: 'Example processor', cores: 4, memory_total: 8589934592, version: '0.1.0' },
      incidents: { items: [] },
      rules,
      'rules/presets': [],
      settings: { settings: { email_transport: 'disabled', recipient_verified: false, recipient: '', alias: '', timezone: 'UTC', report_times: ['09:00'], instant: true, reports: true, recovery: true, relay_url: '', webhook_url: '' }, keys: { ai: false, smtp: false, relay: false, webhook: false } },
      'mcp/clients': [],
      notifications: { items: [] },
    };
    if (path === 'live') {
      const sample = { source_id: 'fixture-host', at: Date.now() / 1000, uptime: 86400, cpu: readings(22), cores: [20, 24, 19, 25], memory: readings(48), memory_used: 4123168604, memory_total: 8589934592, network_in: readings(10240, 'B/s'), network_out: readings(2048, 'B/s'), disks: [{ id: '/', mount: '/', total: 107374182400, available: 64424509440, used_percent: readings(40) }], diagnostics: [], processes: [], agent_memory: 10000000, agent_cpu: .1 };
      return route.fulfill({ contentType: 'text/event-stream', body: `event: sample\ndata: ${JSON.stringify(sample)}\n\n` });
    }
    if (path === 'rules' && failRules) return route.fulfill({ status: 503, json: { error: 'Rule information is unavailable' } });
    return route.fulfill({ status: path in data ? 200 : 404, json: data[path] ?? { error: 'Unmocked endpoint' } });
  });
  return writes;
}

test('first sign-in opens measurements and existing rules without applying a pack', async ({ page }) => {
  const writes = await fixture(page);
  await page.goto('/admin/');
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();
  const table = page.getByRole('table', { name: 'CPU, memory and disk alert rules' });
  await expect(table.locator('tbody tr')).toHaveCount(defaults.length);
  await expect(table.locator('[data-rule-id="cpu-warning"]')).toContainText('80');
  await expect(table.locator('[data-rule-id="cpu-warning"]')).toHaveAttribute('data-rule-enabled', 'true');
  await expect(page.getByRole('button', { name: 'Apply pack', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Configure email', exact: true })).toBeVisible();
  expect(writes).toEqual([]);
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();
});

test('disabled and customized rules are displayed without replacing user preferences', async ({ page }) => {
  const customized = defaults.map(rule => rule.id === 'cpu-warning' ? { ...rule, enabled: false, threshold: 63 } : rule);
  const writes = await fixture(page, customized);
  await page.goto('/admin/');
  const row = page.locator('[data-rule-id="cpu-warning"]');
  await expect(row).toHaveAttribute('data-rule-enabled', 'false');
  await expect(row).toContainText('63');
  await expect(row).toContainText(/disabled|off/i);
  expect(writes).toEqual([]);
});

test('unavailable rules are not presented as enabled coverage', async ({ page }) => {
  await fixture(page, defaults, true);
  await page.goto('/admin/');
  const coverage = page.getByRole('region', { name: 'Alert coverage' });
  await expect(coverage.getByRole('alert')).toBeVisible();
  await expect(coverage.locator('[data-rule-enabled="true"]')).toHaveCount(0);
});

test('an intentionally empty rule list is not silently reseeded', async ({ page }) => {
  const writes = await fixture(page, []);
  await page.goto('/admin/');
  const coverage = page.getByRole('region', { name: 'Alert coverage' });
  await expect(coverage).toContainText(/no .*rules|no .*alerts|0 .*enabled/i);
  await expect(coverage.locator('[data-rule-id]')).toHaveCount(0);
  expect(writes).toEqual([]);
});

test('adding a rule opens the editor without changing existing alarms', async ({ page }) => {
  const writes = await fixture(page);
  await page.goto('/admin/');
  await page.getByRole('button', { name: 'Add custom rule', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await expect(page.getByLabel('Rule name', { exact: true })).toHaveValue('New CPU rule');
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  expect(writes).toEqual([]);
});

test('a new rule cannot overwrite existing rules while they are still loading', async ({ page }) => {
  const writes = await fixture(page);
  let blockRuleLoad = false;
  let continueLoading: () => void = () => {};
  const pending = new Promise<void>(resolve => { continueLoading = resolve; });
  await page.route('**/api/v1/rules', async route => {
    if (blockRuleLoad) await pending;
    await route.fulfill({ json: defaults });
  });
  await page.goto('/admin/');
  await expect(page.getByRole('table', { name: 'CPU, memory and disk alert rules' })).toBeVisible();
  blockRuleLoad = true;
  await page.getByRole('button', { name: 'Add custom rule', exact: true }).click();
  const save = page.getByRole('dialog').getByRole('button', { name: 'Save', exact: true });
  await expect(save).toBeDisabled();
  continueLoading();
  await expect(save).toBeEnabled();
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  expect(writes).toEqual([]);
});

test('overview remains readable on desktop and mobile in both themes', async ({ page }) => {
  await fixture(page);
  await mkdir('../artifacts/validation/overview', { recursive: true });
  await page.goto('/admin/');
  await expect(page.getByRole('table', { name: 'CPU, memory and disk alert rules' })).toBeVisible();
  for (const theme of ['dark', 'light']) {
    if (await page.locator('html').getAttribute('data-theme') !== theme) await page.getByRole('button', { name: 'Switch theme' }).click();
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      await page.screenshot({ path: `../artifacts/validation/overview/${theme}-${width}.png`, fullPage: true, animations: 'disabled' });
    }
  }
});
