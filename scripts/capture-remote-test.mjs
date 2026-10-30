// Real disposable Ubuntu dashboard screenshots. Session cookies arrive only on stdin.
import { chromium, expect } from '../web/node_modules/@playwright/test/index.mjs';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

let browser;
let stage = 'read private input';
try {
  const chunks = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length;
    if (size > 65536) throw new Error('Input exceeds limit');
    chunks.push(chunk);
  }
  const input = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  const origin = new URL(input.origin);
  if (origin.protocol !== 'http:' || origin.hostname !== '127.0.0.1') throw new Error('Loopback tunnel required');
  const output = fileURLToPath(new URL('../artifacts/screenshots/ubuntu/', import.meta.url));
  await mkdir(output, { recursive: true });
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, colorScheme: 'dark' });
  await context.addCookies(input.cookies.map(cookie => ({ ...cookie, url: origin.origin })));
  await context.addInitScript(() => localStorage.setItem('guard-guide-dismissed', '1'));
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.name));
  stage = 'open real overview';
  await page.goto(origin.origin + '/admin');
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();
  await expect(page.locator('.status-pill')).toHaveText('Live data');
  await expect(page.locator('.metric-value').first()).not.toHaveText('—');
  await expect(page.getByText('Total CPU, normalized across all cores', { exact: true })).toBeVisible({ timeout: 20000 });
  const files = [];
  async function capture(name) {
    await page.evaluate(() => window.scrollTo(0, 0));
    const filename = name + '.png';
    await page.screenshot({ path: path.join(output, filename), fullPage: true, animations: 'disabled' });
    files.push('artifacts/screenshots/ubuntu/' + filename);
  }
  stage = 'capture overview themes';
  await capture('overview-dark');
  await page.getByRole('button', { name: 'Switch theme', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await capture('overview-light');
  await page.getByRole('button', { name: 'Switch theme', exact: true }).click();
  stage = 'capture applications and storage';
  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Applications & storage', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Disks & available space', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Hosted websites', exact: true })).toBeVisible();
  await capture('applications-storage');
  stage = 'capture MCP wizard';
  await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Notifications & connections', exact: true }).click();
  await page.getByRole('tab', { name: 'MCP clients', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Connect your AI agent', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'A remote server', exact: false }).click();
  await page.getByLabel('Server IP or hostname', { exact: true }).fill(input.sshHost);
  await expect(page.getByText('SSH key sign-in is required.', { exact: true })).toBeVisible();
  await capture('mcp-wizard');
  if (errors.length) throw new Error('Browser errors');
  process.stdout.write(JSON.stringify({ source: 'actual Ubuntu disposable service', synthetic_data: false, files }));
} catch (error) {
  // Never serialize input, cookie values, or a captured authenticated browser state.
  process.stderr.write(`Remote screenshot capture failed at ${stage} (${error.name}).\n`);
  process.exitCode = 1;
} finally {
  if (browser) await browser.close();
}
