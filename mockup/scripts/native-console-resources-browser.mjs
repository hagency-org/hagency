/* Actual retained resource controls over native Salvo; no operator credential. */
import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const config = JSON.parse((await lines.next()).value);
async function fixture(command) { console.log(command); return JSON.parse((await lines.next()).value); }
const browser = await chromium.launch({ executablePath: process.env.HAGENCY_BROWSER_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--disable-background-networking', '--disable-component-update', '--no-default-browser-check'] });
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
  const ready = () => page.locator('[data-native-resource-state="ready"][aria-busy="false"]').waitFor();
  const row = (id = config.resource) => page.locator(`[data-resource-row="${id}"]`);
  const publication = (id = config.resource) => `${config.base}/console/api/resources/${id}/publication`;
  async function toggle(id, target) {
    await row(id).getByRole('button').click();
    await page.locator('[data-resource-action="saved"]').waitFor();
    await row(id).locator(`[data-publication="${target}"]`).waitFor();
    await ready();
  }
  await page.goto(config.url);
  await ready();
  assert.equal(new URL(page.url()).hash, '');
  await page.locator('#native-resource').selectOption(config.resource);
  await page.locator(`[data-resource-id="${config.resource}"]`).waitFor();
  assert.match(await page.locator('main').innerText(), /Delivery to Palpo has not been verified/);
  assert.equal(await row().getByRole('button').isEnabled(), true);
  await toggle(config.resource, false); await toggle(config.resource, true);
  if (process.env.HAGENCY_CONSOLE_SCREENSHOTS && !config.executable) {
    await mkdir(process.env.HAGENCY_CONSOLE_SCREENSHOTS, { recursive: true });
    await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, 'console-resources-en.png'), fullPage: true });
  }
  const cookie = (await context.cookies()).find((c) => c.name === 'hagency_console');
  assert(cookie?.httpOnly && cookie.sameSite === 'Strict' && cookie.path === '/console');
  assert.equal(await page.evaluate(() => document.cookie), '');
  assert.equal(await page.evaluate(async () => (await fetch('/api/native/v1/resources')).status), 403);
  await page.getByRole('button', { name: '中文', exact: true }).click();
  await page.getByRole('button', { name: '深色', exact: true }).click();
  await page.reload(); await ready();
  assert.equal(await page.locator('html').getAttribute('lang'), 'zh-CN');
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  assert.equal(await row().getByRole('button').textContent(), '从本地资源目录撤下');
  await toggle(config.resource, false); await toggle(config.resource, true);
  if (process.env.HAGENCY_CONSOLE_SCREENSHOTS && !config.executable) await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, 'console-resources-zh.png'), fullPage: true });
  if (!config.executable) {
    const created = (await fixture('CREATE_RESOURCE')).resource;
    await page.getByRole('button', { name: '刷新', exact: true }).click();
    await page.locator(`option[value="${created}"]`).waitFor({ state: 'attached' });
    await page.locator('#native-resource').selectOption(created);
    await page.locator(`[data-resource-id="${created}"]`).waitFor();
    assert.equal(new URL(page.url()).searchParams.get('resource_id'), created);
    await page.reload(); await ready();
    assert.equal(await page.locator('#native-resource').inputValue(), created);
    await page.getByRole('button', { name: 'English', exact: true }).click();
    await fixture('EDIT_RESOURCE');
    await row(created).getByRole('button').click();
    await page.locator('[data-resource-action="conflict"]').waitFor();
    assert.equal(await row(created).locator('[data-publication="true"]').count(), 1);
    await page.getByRole('button', { name: 'Read current state', exact: true }).click(); await ready();
    assert.equal(await page.locator('[data-resource-action="conflict"]').count(), 1);
    // The actual server commits; the browser loses only its response.
    // Playwright intercepts before Chromium adds Fetch Metadata. The test relay
    // supplies same-origin metadata after checking the actual page origin and
    // forwards the actual HttpOnly cookie; native authority remains unchanged.
    await context.route(publication(created), async (route) => { const headers = await route.request().allHeaders(); assert(headers.cookie, "actual browser request retains its session cookie"); assert.equal(new URL(page.url()).origin, config.base); headers["sec-fetch-site"] = "same-origin"; headers.origin = config.base; const actual = await route.fetch({ headers }); assert.equal(actual.status(), 200, await actual.text()); await route.abort('failed'); });
    await row(created).getByRole('button').click();
    await page.locator('[data-resource-action="unknown"]').waitFor();
    await context.unroute(publication(created));
    await page.getByRole('button', { name: 'Read current state', exact: true }).click(); await ready();
    await row(created).locator('[data-publication="false"]').waitFor();
    assert.equal(await page.locator('[data-resource-action="unknown"]').count(), 1);
    await toggle(created, true);
    // A real held SQLite transaction leaves the original command pending. Logout
    // must refuse Busy without claiming revocation, and offer a visible retry.
    await fixture('HOLD_STORE');
    const finished = page.waitForResponse(publication(created));
    await row(created).getByRole('button').click();
    await page.locator('[data-resource-action="pending"]').waitFor();
    await page.getByRole('button', { name: 'End access', exact: true }).click();
    await page.locator('[data-logout-state="busy"]').waitFor();
    assert.match(await page.locator('main').innerText(), /Access has not been ended/);
    await fixture('RELEASE_STORE'); await finished;
    await page.getByRole('button', { name: 'Retry ending access', exact: true }).click();
    await page.locator('[data-logout-state="ended"]').waitFor();
    assert.equal(await page.locator('[data-native-resource-state]').count(), 0);
  } else {
    await page.getByRole('button', { name: '结束访问', exact: true }).click();
    await page.locator('[data-logout-state="ended"]').waitFor();
  }
  const storage = await page.evaluate(() => ({ local: Object.fromEntries(Object.entries(localStorage)), session: Object.fromEntries(Object.entries(sessionStorage)) }));
  assert.deepEqual(Object.keys(storage.local).sort(), ['hagency.locale', 'hagency.theme']); assert.deepEqual(storage.session, {});
  assert(!JSON.stringify(storage).includes(cookie.value));
  assert(urls.every((url) => !url.includes('access=') && !url.includes(cookie.value)));
  assert.deepEqual(failures, []);
  console.log(config.executable ? 'PASS resource publication through native executable with empty runtime PATH' : 'PASS bilingual retained resource selection, native publication, conflict, lost reply and Busy logout');
} finally { await browser.close(); }
process.exit(0);
