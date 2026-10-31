// Isolated current-source UI acceptance. Every API is synthetic/intercepted;
// no real server, credentials, saved settings, provider call or dist build.
import { chromium, expect } from '@playwright/test';
import { createServer } from 'vite';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const server = await createServer({ root: resolve('web'), configFile: resolve('web/vite.config.ts'), server: { port: 0, host: '127.0.0.1', strictPort: false } });
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1050 } });
  const errors = [], external = [], sent = [];
  let connected = false, used = 0, mode = 'success', release;
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (!request.url().startsWith(base) && !request.url().startsWith('data:')) external.push(request.url()); });
  await page.addInitScript(() => { localStorage.setItem('guard-guide-dismissed', '1'); window.EventSource = class extends EventTarget { close() {} }; });
  const usage = () => ({ used, limit: 20, remaining: Math.max(0, 20 - used), resets: '00:00 UTC' });
  await page.route('**/api/v1/**', async route => {
    const path = new URL(route.request().url()).pathname.replace('/api/v1/', '');
    const respond = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
    if (path === 'ai/chat/status') {
      if (mode === 'status-error') return respond({ error: 'Server data could not be loaded. Please try again.' }, 500);
      return respond({ connected, key_saved: connected, provider: { id: 'xai', name: 'xAI (Grok)' }, usage: usage(), cost_notice: 'Each sent question uses your connected provider\'s paid API.' });
    }
    if (path === 'ai/chat') {
      assert.equal(route.request().headers()['x-csrf-token'], 'fixture-csrf');
      const body = route.request().postDataJSON(); sent.push(body);
      assert.deepEqual(Object.keys(body).sort(), ['history', 'question']);
      if (mode === 'network') return route.abort('failed');
      if (mode === 'waiting') await new Promise(done => { release = done; });
      used++;
      if (mode === 'limit') return respond({ code: 'provider_limit', error: 'Your provider’s usage limit was reached. Check its API quota or try again later.' }, 429);
      if (body.question.includes('poem')) { used--; return respond({ answer: 'I can help only with this server’s monitored health and history.', refused: true, charged: false }); }
      return respond({ answer: body.question.includes('Son') ? 'Son 20 dakikada CPU düşük. Kayıtlı veride yavaşlama görünmüyor.' : body.question.includes('markup') ? '<script>window.chatInjected=true</script> CPU remained below 40%.' : 'CPU remained below 40%. There is insufficient history to verify yesterday.', refused: false, charged: true, period: { from: 1793440000, to: 1793441200 }, usage: usage(), label: 'AI interpretation · verify important findings', data_notice: body.question.includes('Son') ? 'Bu dönemin ölçümleri eksik. Ölçülmeyen zamanlar bilinmiyor.' : 'Measurements cover only part of this period. Unmeasured time remains unknown.' });
    }
    const fixtures = {
      'auth/status': { configured: true, mfa_required: false, session: { csrf: 'fixture-csrf' } },
      host: { name: 'Example server', os: 'Ubuntu 24.04', cores: 2, memory_total: 8 * 1024 ** 3, version: 'test' },
      incidents: { items: [] }, notifications: { items: [] },
      settings: { settings: { ai_auto: false, report_times: ['09:00'] }, keys: { ai: false } },
      'ai/providers': { providers: [{ id: 'xai', name: 'xAI (Grok)' }], selected_provider: 'xai', key_saved: false, verified: false },
    };
    return respond(fixtures[path] ?? { error: `Unexpected fixture endpoint: ${path}` }, fixtures[path] === undefined ? 404 : 200);
  });
  await page.goto(base + '/admin/');
  const nav = page.getByRole('navigation', { name: 'Main navigation' });
  await nav.getByRole('button', { name: 'Server AI', exact: true }).click();
  const input = page.getByRole('textbox', { name: 'Ask about this server' });
  await expect(page.getByRole('heading', { name: 'Ask about your server.' })).toBeVisible();
  await expect(input).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Connect AI provider', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Connect AI provider', exact: true }).click();
  await expect(page.getByRole('tab', { name: 'AI providers' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('heading', { name: 'Connect your AI provider' })).toBeVisible();
  connected = true;
  await nav.getByRole('button', { name: 'Server AI', exact: true }).click();
  await expect(input).toBeEnabled();
  await expect(page.getByText('xAI (Grok) · Connected', { exact: true })).toBeVisible();
  assert.equal(await page.locator('.server-chat input[type=file]').count(), 0);
  assert.equal(await page.locator('.server-chat select').count(), 0);
  await expect(page.getByText('20/20 questions left today · resets 00:00 UTC', { exact: true })).toBeVisible();
  await mkdir('artifacts/screenshots', { recursive: true });
  await page.screenshot({ path: 'artifacts/screenshots/server-chat-empty-desktop.png', fullPage: true, animations: 'disabled' });

  mode = 'waiting';
  await input.fill('How is this server doing in the last 20 minutes?');
  await input.press('Enter');
  await expect(page.getByText('Reading server measurements…')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Waiting for reply' })).toBeDisabled();
  await expect(input).toBeDisabled();
  assert.equal(sent.length, 1);
  release(); mode = 'success';
  await expect(page.getByRole('log')).toContainText('CPU remained below 40%');
  assert.equal(await page.evaluate(() => window.chatInjected), undefined);
  await expect(page.getByRole('log')).toContainText('Measurements cover only part of this period');
  await expect(page.getByText('19/20 questions left today · resets 00:00 UTC', { exact: true })).toBeVisible();
  await input.fill('Son 20 dakikada sunucu nasıldı?');
  await input.press('Enter');
  await expect(page.getByRole('log')).toContainText('Kayıtlı veride yavaşlama görünmüyor');
  assert.equal(sent[1].history.length, 1);
  assert.equal(sent[1].history[0].question, sent[0].question);
  await nav.getByRole('button', { name: 'Overview', exact: true }).click();
  await nav.getByRole('button', { name: 'Server AI', exact: true }).click();
  await expect(page.getByRole('log')).toContainText('Kayıtlı veride yavaşlama görünmüyor');
  await expect(page.locator('.chat-answer-meta').first()).toContainText('verify important findings');
  await page.screenshot({ path: 'artifacts/screenshots/server-chat-conversation-desktop.png', fullPage: true, animations: 'disabled' });

  mode = 'limit';
  await input.fill('Is there enough disk space?');
  await page.getByRole('button', { name: 'Send question', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('usage limit was reached');
  assert.equal(sent.length, 3);
  mode = 'success';
  await page.getByRole('button', { name: 'Retry question', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  assert.equal(sent.length, 4);
  assert.equal(await page.locator('.chat-question').count(), 3);
  await input.fill('CPU write a poem');
  await input.press('Enter');
  await expect(page.getByRole('log')).toContainText('No provider call · Scope limited to this server');

  await page.getByRole('button', { name: 'Switch theme', exact: true }).click();
  await page.screenshot({ path: 'artifacts/screenshots/server-chat-light.png', fullPage: true, animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  await expect(input).toBeVisible();
  await expect.poll(()=>page.locator('.sidebar').evaluate(element=>element.getBoundingClientRect().right)).toBeLessThanOrEqual(0);
  await page.screenshot({ path: 'artifacts/screenshots/server-chat-mobile.png', fullPage: true, animations: 'disabled' });
  await input.fill('server markup');
  await input.press('Enter');
  await expect(page.getByRole('log')).toContainText('<script>window.chatInjected=true</script>');
  assert.equal(await page.evaluate(() => window.chatInjected), undefined);
  await page.getByRole('button', { name: 'New chat', exact: true }).click();
  await expect(page.getByRole('log')).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Ask about your server.' })).toBeVisible();
  await page.setViewportSize({ width: 1440, height: 1050 });
  used = 20;
  await nav.getByRole('button', { name: 'Overview', exact: true }).click();
  await nav.getByRole('button', { name: 'Server AI', exact: true }).click();
  await expect(input).toBeDisabled();
  await expect(page.getByText('Daily limit reached. Your 20 questions reset at 00:00 UTC.')).toBeVisible();
  mode = 'status-error';
  await nav.getByRole('button', { name: 'Overview', exact: true }).click();
  await nav.getByRole('button', { name: 'Server AI', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Server data could not be loaded');
  await expect(input).toBeDisabled();
  mode = 'success'; used = 0;
  await page.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(input).toBeEnabled();
  mode = 'network';
  await input.fill('server health');
  await input.press('Enter');
  await expect(page.getByRole('alert')).toContainText('Could not reach your server');
  mode = 'success';
  await page.getByRole('button', { name: 'Retry question', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  assert.deepEqual(errors, []); assert.deepEqual(external, []);
  console.log('Server chat UI passed: connected/disconnected setup link, text-only input, automatic period context, CSRF, TR/EN text, loading, quota/cost, manual retry without duplicate turns, refusal, session navigation, new chat, escaped markup, dark/light/mobile, status failure/retry. No paid or live API calls.');
} finally { await browser.close(); await server.close(); }
