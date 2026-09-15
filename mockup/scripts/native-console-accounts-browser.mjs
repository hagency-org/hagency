/* The native accounts page over a real Chromium; no operator credential. */
import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright-core';
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const config = JSON.parse((await lines.next()).value);
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
  await page.goto(config.url);
  await page.locator('[data-native-state="ready"][aria-busy="false"]').waitFor();
  assert.equal(new URL(page.url()).hash, '');
  // The seeded row is present and readable.
  const row = page.locator(`[data-account-row="${config.account}"]`);
  await row.waitFor();
  assert.equal(await row.locator('[data-account="state"]').textContent(), 'active');
  // No credential namespace tuple, seat or volume value appears in the DOM
  // or in local or session storage.
  const dom = await page.content();
  for (const secret of config.secrets) assert.ok(!dom.includes(secret), `identity value reached the DOM: ${secret}`);
  for (const store of [await page.evaluate(() => JSON.stringify(window.localStorage)), await page.evaluate(() => JSON.stringify(window.sessionStorage))]) {
    for (const secret of config.secrets) assert.ok(!store.includes(secret), `identity value reached storage: ${secret}`);
  }
  assert.deepEqual(failures, []);
  await context.close();
} finally {
  await browser.close();
}
