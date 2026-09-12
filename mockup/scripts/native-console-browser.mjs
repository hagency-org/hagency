/* Real Chromium over a fresh native fixture. It receives no operator token. */
import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const config = JSON.parse((await lines.next()).value);
const browser = await chromium.launch({ executablePath: process.env.HAGENCY_BROWSER_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true,
  args: ['--disable-background-networking', '--disable-component-update', '--no-default-browser-check'] });
try {
  const context = await browser.newContext({ serviceWorkers: 'block' });
  const page = await context.newPage();
  const failures = []; const urls = [];
  page.on('pageerror', (error) => failures.push(error.message));
  await context.route('**/*', async (route) => {
    const url = new URL(route.request().url()); urls.push(url.toString());
    if (url.origin !== config.base) { failures.push('unexpected external request'); await route.abort(); }
    else await route.continue();
  });
  await page.goto(config.url);
  await page.locator('[data-native-state="ready"]').waitFor();
  assert.equal(new URL(page.url()).hash, '');
  assert.equal(await page.locator('[data-engagement-id]').getAttribute('data-engagement-id'), config.engagement);
  assert.equal(await page.locator('[data-kind="input"]').nth(0).textContent(), '4');
  assert.equal(await page.locator('[data-kind="input"]').nth(1).textContent(), '7');
  assert.match(await page.locator('main').innerText(), /Historical high-water lower bounds/);
  assert.match(await page.locator('main').innerText(), /untrusted usage evidence/);
  if (!config.executable) {
    // Delay the actual same-selection read: refresh must retain the current view.
    const path = `${config.base}/console/api/engagements/${config.engagement}/usage`;
    let release; let observed;
    const held = new Promise((resolve) => { release = resolve; });
    const started = new Promise((resolve) => { observed = resolve; });
    await context.route(path, async (route) => { observed(); await held; await route.continue(); });
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await started;
    assert.equal(await page.locator('[data-native-state="ready"]').getAttribute('aria-busy'), 'true');
    assert.equal(await page.locator('#native-engagement').count(), 1);
    assert.equal(await page.locator('[data-kind="input"]').first().textContent(), '4');
    assert.match(await page.locator('main').innerText(), /Refreshing usage/);
    const finished = page.waitForResponse(path); release(); await finished;
    await page.locator('[data-native-state="ready"][aria-busy="false"]').waitFor();
    await context.unroute(path);
    // A real transport failure marks the retained observation stale explicitly.
    await context.route(path, (route) => route.abort('failed'));
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await page.locator('[data-native-state="stale"]').waitFor();
    assert.match(await page.locator('main [role="alert"]').innerText(), /earlier observations may be stale/);
    assert.equal(await page.locator('[data-kind="input"]').first().textContent(), '4');
    await context.unroute(path);
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    await page.locator('[data-native-state="ready"][aria-busy="false"]').waitFor();
  }
  if (process.env.HAGENCY_CONSOLE_SCREENSHOTS && !config.executable) {
    await mkdir(process.env.HAGENCY_CONSOLE_SCREENSHOTS, { recursive: true });
    await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, 'console-usage-en.png'), fullPage: true });
  }
  assert(!await page.locator('main').innerText().then((v) => /NaN|undefined/.test(v)));
  const cookies = await context.cookies();
  const cookie = cookies.find((c) => c.name === 'hagency_console');
  assert(cookie?.httpOnly && cookie.sameSite === 'Strict' && cookie.path === '/console');
  assert(cookie.expires * 1000 > Date.now() && cookie.expires * 1000 <= Date.now() + 901000);
  assert.equal(await page.evaluate(() => document.cookie), '');
  assert(urls.every((url) => !url.includes('access=') && !url.includes(cookie.value)));
  const rawStatus = await page.evaluate(async () => (await fetch('/api/native/v1/engagements')).status);
  assert.equal(rawStatus, 403);
  await page.getByRole('button', { name: '中文', exact: true }).click();
  await page.getByRole('button', { name: '深色', exact: true }).click();
  await page.reload();
  await page.locator('[data-native-state="ready"]').waitFor();
  assert.equal(await page.locator('html').getAttribute('lang'), 'zh-CN');
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  assert.match(await page.locator('main').innerText(), /历史高水位下界/);
  assert.match(await page.locator('main').innerText(), /未经验证的用量证据/);
  if (process.env.HAGENCY_CONSOLE_SCREENSHOTS && !config.executable) await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, 'console-usage-zh.png'), fullPage: true });
  if (!config.executable) {
    console.log('CREATE_ENGAGEMENT');
    const created = JSON.parse((await lines.next()).value).engagement;
    await page.getByRole('button', { name: '刷新', exact: true }).click();
    await page.locator(`option[value="${created}"]`).waitFor({ state: 'attached' });
    await page.locator('#native-engagement').selectOption(created);
    await page.locator(`[data-engagement-id="${created}"]`).waitFor();
    assert.equal(new URL(page.url()).searchParams.get('engagement_id'), created);
    assert.match(await page.locator('main').innerText(), /此期间没有观测。用量未知/);
    assert.equal(await page.locator('[data-kind="input"]').first().textContent(), '未知');
    await page.reload();
    await page.locator(`[data-engagement-id="${created}"]`).waitFor();
    // Hold the real B read, return to A, then release B. No response is mocked.
    await page.locator('#native-engagement').selectOption(config.engagement);
    await page.locator(`[data-engagement-id="${config.engagement}"]`).waitFor();
    let releaseRead; let observedRead;
    const heldRead = new Promise((resolve) => { releaseRead = resolve; });
    const readStarted = new Promise((resolve) => { observedRead = resolve; });
    const delayedPath = `${config.base}/console/api/engagements/${created}/usage`;
    await context.route(delayedPath, async (route) => { observedRead(); await heldRead; await route.continue(); });
    await page.locator('#native-engagement').selectOption(created);
    await readStarted;
    await page.goBack();
    await page.locator(`[data-engagement-id="${config.engagement}"]`).waitFor();
    const finishedRead = page.waitForResponse(delayedPath);
    releaseRead(); await finishedRead;
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(new URL(page.url()).searchParams.get('engagement_id'), config.engagement);
    assert.equal(await page.locator('[data-engagement-id]').getAttribute('data-engagement-id'), config.engagement);
    await context.unroute(delayedPath);
    await page.locator('#native-engagement').selectOption(created);
    await page.locator(`[data-engagement-id="${created}"]`).waitFor();
    await page.getByRole('button', { name: 'English', exact: true }).click();
    assert.equal(await page.locator('[data-kind="input"]').first().textContent(), 'Unknown');
    await page.goto(`${config.base}/console/usage/?engagement_id=does_not_exist`);
    await page.locator('main [role="alert"]').waitFor();
    assert.match(await page.locator('main').innerText(), /not present in the native service/);
    assert.equal(await page.locator('[data-kind]').count(), 0);
    await page.goto(`${config.base}/console/usage/?engagement_id=${config.engagement}`);
    await page.locator('[data-native-state="ready"]').waitFor();
  }
  const storage = await page.evaluate(() => ({ local: Object.fromEntries(Object.entries(localStorage)), session: Object.fromEntries(Object.entries(sessionStorage)) }));
  assert.deepEqual(Object.keys(storage.local).sort(), ['hagency.locale', 'hagency.theme']);
  assert.deepEqual(storage.session, {});
  assert(!JSON.stringify(storage).includes(cookie.value));
  let releaseLogout; let observedLogout; let readsDuringLogout = 0;
  const heldLogout = new Promise((resolve) => { releaseLogout = resolve; });
  const logoutStarted = new Promise((resolve) => { observedLogout = resolve; });
  await context.route(`${config.base}/console/session`, async (route) => { observedLogout(); await heldLogout; await route.continue(); });
  page.on('request', (request) => { if (request.url().includes('/console/api/')) readsDuringLogout += 1; });
  await page.getByRole('button', { name: config.executable ? '结束访问' : 'End access', exact: true }).click();
  await page.locator('[data-native-state="access"]').waitFor();
  await logoutStarted;
  await page.evaluate(() => { window.dispatchEvent(new Event('focus')); document.dispatchEvent(new Event('visibilitychange')); window.dispatchEvent(new PopStateEvent('popstate')); });
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(readsDuringLogout, 0);
  const ended = page.waitForResponse((response) => response.url().endsWith('/console/session') && response.request().method() === 'DELETE');
  releaseLogout(); await ended;
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await page.locator('[data-native-state="access"]').waitFor();
  assert.equal(readsDuringLogout, 0);
  assert.equal(await page.locator('[data-kind]').count(), 0);
  assert.deepEqual(failures, []);
  console.log(config.executable ? 'PASS native executable browser without Node runtime PATH' : 'PASS bilingual retained browser, dynamic engagement, null evidence, privacy and logout');
} finally { await browser.close(); }
process.exit(0);
