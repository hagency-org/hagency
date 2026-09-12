/* Real retained wizard over native HTTP. Tooling never gives the browser an operator token. */
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
  const context = await browser.newContext({ serviceWorkers: 'block' }); const page = await context.newPage();
  const failures = []; const urls = []; page.on('pageerror', (e) => failures.push(e.message));
  await context.route('**/*', async (route) => { const url = new URL(route.request().url()); urls.push(url.toString()); if (url.origin !== config.base) { failures.push('external request'); await route.abort(); } else await route.continue(); });
  const ready = () => page.locator('[data-native-resource-state="ready"][aria-busy="false"]').waitFor();
  const editor = (id) => page.locator(`[data-native-configuration-id="${id}"][aria-busy="false"]`).waitFor();
  const configuration = (id) => `${config.base}/console/api/resources/${id}/configuration`;
  const editUrl = (id) => `${config.base}/console/resources/new/?resource_id=${id}`;
  const createUrl = (id) => `${config.base}/console/resources/new/?source_resource_id=${id}`;
  const next = () => page.getByRole('button', { name: /^(Next|下一步)$/ }).click();
  // The logout attribute is a single value the product only renders for ONE store
  // outcome (`console/authority.rs:200-205` maps only `Error::Busy` to the 429 the
  // client turns into `busy`; every other outcome renders `unknown`). A bare
  // `waitFor` on the expected value cannot tell "never rendered" from "rendered
  // with another value", so report what was actually there.
  async function expectLogoutState(state) {
    const expected = page.locator(`[data-logout-state="${state}"]`);
    try {
      await expected.waitFor();
    } catch (error) {
      const sections = await page.locator('[data-logout-state]').count();
      const observed = sections
        ? await page.locator('[data-logout-state]').first().getAttribute('data-logout-state')
        : null;
      const body = await page.locator('main').innerText().catch(() => '');
      if (process.env.HAGENCY_CONSOLE_SCREENSHOTS) {
        await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, `logout-${state}-failure.png`), fullPage: true }).catch(() => {});
      }
      throw new Error(
        `expected data-logout-state="${state}", observed ${JSON.stringify(observed)} ` +
        `(sections=${sections}); main=${JSON.stringify(body.slice(0, 400))}; ${error.message}`
      );
    }
  }
  async function budget(id, edit = false) { await page.goto(edit ? editUrl(id) : createUrl(id)); await editor(id); await next(); await next(); await next(); }
  async function ceiling(tokens) { await page.locator('#configuration-ceiling').selectOption('monthly'); await page.locator('#wz-tokens').fill(String(tokens)); }
  async function save(edit = false) { await page.getByRole('button', { name: edit ? /^(Save configuration|保存配置)$/ : /^(Create another configuration|创建另一项配置)$/ }).click(); await page.locator('[data-configuration-action="saved"]').waitFor(); }
  await page.goto(config.url); await ready();
  await page.getByRole('button', { name: 'Light', exact: true }).click();
  const cookie = (await context.cookies()).find((c) => c.name === 'hagency_console'); assert(cookie?.httpOnly && cookie.sameSite === 'Strict'); assert.equal(await page.evaluate(() => document.cookie), ''); assert.equal(new URL(page.url()).hash, '');
  assert.equal(await page.evaluate(async () => (await fetch('/api/native/v1/resources')).status), 403);
  if (config.verifyOnly) {
    await page.goto(`${config.base}/console/resources/?resource_id=${config.resource}`); await ready();
    assert.equal(await page.locator('[data-budget="ceiling"]').innerText(), '22,222');
    console.log(`VERIFIED ${config.resource}`);
  } else {
    await budget(config.resource); await ceiling(12345);
    await page.locator('#wz-tokens').fill('9007199254740992'); assert.equal(await page.getByRole('button', { name: 'Create another configuration', exact: true }).isEnabled(), false);
    await page.locator('#wz-tokens').fill('1e3'); assert.equal(await page.getByRole('button', { name: 'Create another configuration', exact: true }).isEnabled(), false);
    await page.locator('#wz-tokens').fill('12345');
    for (const selector of ['#wz-name', '#wz-rate', '[name="apiKey"]']) assert.equal(await page.locator(selector).count(), 0);
    await save();
    const saved = await page.locator('[data-configuration-action="saved"] a').getAttribute('href'); const created = new URL(saved, config.base).searchParams.get('resource_id');
    assert.match(created, /^resource_[a-f0-9]{24}$/); assert.notEqual(created, config.resource); console.log(`CREATED ${created}`);
    if (process.env.HAGENCY_CONSOLE_SCREENSHOTS && !config.executable) { await mkdir(process.env.HAGENCY_CONSOLE_SCREENSHOTS, { recursive: true }); await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, 'console-configuration-en.png'), fullPage: true }); }
    await page.locator('[data-configuration-action="saved"] a').click(); await ready(); assert.equal(await page.locator('#native-resource').inputValue(), created);
    await page.getByRole('button', { name: '中文', exact: true }).click(); await page.getByRole('button', { name: '深色', exact: true }).click();
    await budget(created, true); await ceiling(22222); await save(true);
    assert.equal(await page.locator('html').getAttribute('lang'), 'zh-CN'); assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
    const colors = await page.evaluate(() => [getComputedStyle(document.querySelector('#wz-tokens')).backgroundColor, getComputedStyle(document.querySelector('#configuration-ceiling')).backgroundColor]);
    assert.equal(colors[0], colors[1]); assert.notEqual(colors[0], 'rgb(255, 255, 255)');
    console.log(`EDITED ${created}`);
    if (process.env.HAGENCY_CONSOLE_SCREENSHOTS && !config.executable) await page.screenshot({ path: join(process.env.HAGENCY_CONSOLE_SCREENSHOTS, 'console-configuration-zh.png'), fullPage: true });
    await page.getByRole('button', { name: 'English', exact: true }).click();
    if (!config.executable) {
      await budget(created, true); await ceiling(33333);
      // A same-query automatic refresh keeps both the active form and unsaved input.
      const refreshed = page.waitForResponse((r) => r.url() === configuration(created) && r.request().method() === 'GET');
      await page.evaluate(() => window.dispatchEvent(new Event('focus'))); await refreshed; await editor(created);
      assert.equal(await page.locator('#wz-tokens').inputValue(), '33333');
      await fixture(`EDIT_RESOURCE ${created}`); await page.getByRole('button', { name: 'Save configuration', exact: true }).click(); await page.locator('[data-configuration-action="conflict"]').waitFor();
      await page.getByRole('button', { name: 'Reload configuration and discard draft', exact: true }).click(); await editor(created);
      await page.locator('#configuration-ceiling').selectOption('monthly'); assert.equal(await page.locator('#wz-tokens').inputValue(), '6000');
      // Real delayed B navigation cannot replace A after Back supersedes it.
      let release; let entered; const held = new Promise((r) => { release = r; }); const began = new Promise((r) => { entered = r; });
      await context.route(configuration(config.resource), async (route) => { entered(); await held; await route.continue(); });
      await page.evaluate((id) => { history.pushState(history.state, '', `/console/resources/new/?resource_id=${id}`); window.dispatchEvent(new PopStateEvent('popstate')); }, config.resource);
      await began; await page.goBack(); await editor(created); release(); await context.unroute(configuration(config.resource)); await page.waitForTimeout(100);
      assert.equal(await page.locator('[data-native-configuration-id]').getAttribute('data-native-configuration-id'), created);
      // Commit a real additional configuration and lose only its original reply.
      await budget(config.resource); await ceiling(44444); let posts = 0; let unknownId;
      await context.route(`${config.base}/console/api/resources`, async (route) => {
        if (route.request().method() !== 'POST') { await route.continue(); return; }
        posts += 1; const headers = await route.request().allHeaders(); assert(headers.cookie); assert.equal(new URL(page.url()).origin, config.base);
        headers['sec-fetch-site'] = 'same-origin'; headers.origin = config.base;
        const actual = await route.fetch({ headers }); assert.equal(actual.status(), 200, await actual.text()); unknownId = (await actual.json()).resourceId; await route.abort('failed');
      });
      await page.getByRole('button', { name: 'Create another configuration', exact: true }).click(); await page.locator('[data-configuration-action="unknown"]').waitFor();
      assert.equal(await page.getByRole('button', { name: 'Create another configuration', exact: true }).isEnabled(), false);
      await page.getByRole('button', { name: 'Refresh', exact: true }).click(); await editor(config.resource); assert.equal(await page.locator('[data-configuration-action="unknown"]').count(), 1); assert.equal(posts, 1);
      await context.unroute(`${config.base}/console/api/resources`); console.log(`UNKNOWN_CREATED ${unknownId}`);
      // Real held SQLite command makes server revocation explicitly Busy.
      await budget(created, true); await ceiling(77777); await fixture('HOLD_STORE');
      const finished = page.waitForResponse((r) => r.url() === configuration(created) && r.request().method() === 'PATCH');
      await page.getByRole('button', { name: 'Save configuration', exact: true }).click(); await page.locator('[data-configuration-action="pending"]').waitFor();
      await page.getByRole('button', { name: 'End access', exact: true }).click(); await expectLogoutState('busy');
      await fixture('RELEASE_STORE'); await finished;
      await page.getByRole('button', { name: 'Retry ending access', exact: true }).click(); await expectLogoutState('ended');
      assert.equal(await page.locator('[data-native-configuration-id]').count(), 0);
    }
  }
  const storage = await page.evaluate(() => ({ local: Object.fromEntries(Object.entries(localStorage)), session: Object.fromEntries(Object.entries(sessionStorage)) }));
  assert(Object.keys(storage.local).every((k) => ['hagency.locale', 'hagency.theme'].includes(k))); assert.deepEqual(storage.session, {});
  assert(!JSON.stringify(storage).includes(cookie.value)); assert(urls.every((url) => !url.includes('access=') && !url.includes(cookie.value))); assert.deepEqual(failures, []);
  console.log('PASS retained native resource configuration');
} finally { await browser.close(); }
process.exit(0);
