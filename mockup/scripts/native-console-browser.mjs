/* Real Chromium over a fresh native fixture. It receives no operator token. */
import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright-core';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const config = JSON.parse((await lines.next()).value);

/* The agent roster walk (ADR-126), shared by the full console walk and the
 * roster-only lane: the page renders the seven-column projection, the
 * SERVER-OWNED unavailable list verbatim, the null-not-zero activity arms,
 * and no lifecycle control — and no private value reaches the screen. */
async function rosterWalk(page) {
  await page.goto(`${config.base}/console/agents/`);
  await page.locator('[data-native-state="ready"]').waitFor();
  assert((await page.locator('tbody tr').count()) >= 3, 'one roster row per seeded engagement');
  const text = await page.locator('main').innerText();
  assert.match(text, /UsageWorker/);
  assert.match(text, /read-only — derived from the engagement projections|只读 —— 由接洽投影派生/);
  // The server's own gap list, rendered verbatim: the page never decides
  // which columns are unknown.
  assert.match(text, /tmux/);
  assert.match(text, /workspace_path/);
  // The null-not-zero arms: the active engagement carries its dispatch
  // clock; a pending one renders the unknown word — never a zero.
  const cells = await page.locator('tbody tr td:last-child').allInnerTexts();
  assert(cells.some((c) => /^\d{4}-\d{2}-\d{2}T/.test(c)), 'the active engagement carries its dispatch clock');
  assert(cells.some((c) => c === 'Unknown' || c === '未知'), 'an engagement with no attempt row renders unknown');
  assert(cells.every((c) => c !== '0'), 'unknown is never rendered as zero');
  assert(!/private_|\/Users\/|tmux attach/.test(text), 'no private path, home or target renders');
  assert((await page.locator('main button').count()) === 1, 'Refresh is the only control — no lifecycle, no mutation');
}

/* The project-sides walk (ADR-132): the page renders the six-key side
 * cards with the SERVER-OWNED unavailable list verbatim, and no
 * credential value can appear on screen — the validator refuses any key
 * set other than the declared one and no declared key is a credential. */
async function projectSidesWalk(page) {
  await page.goto(`${config.base}/console/project-sides/`);
  await page.locator('[data-native-state="ready"]').waitFor();
  const text = await page.locator('main').innerText();
  assert.match(text, /example\.test/, 'the side card renders, keyed by server name');
  assert.match(text, /read-only — the fleet registrations|只读 —— 车队注册及其项目/);
  assert.match(text, /!reception:example\.test/, 'the reception room id renders as ordinary data');
  assert.match(text, /project_one/, 'the joined project renders');
  assert.match(text, /!project:example\.test/, 'the project room id renders');
  // The server's own gap list, verbatim: credential_kind and owner are
  // NAMED as unknown rather than invented.
  assert.match(text, /credential_kind/);
  assert.match(text, /owner/);
  assert(!/as_token|hs_token|asToken|hsToken/.test(text), 'no credential word on screen');
  assert(!/@owner:example\.test/.test(text), 'the owner mxid stays withheld');
  assert(!/!private:example\.test/.test(text), 'the owner DM room stays withheld');
  assert((await page.locator('main button').count()) === 1, 'Refresh is the only control');
}

const browser = await chromium.launch({ executablePath: process.env.HAGENCY_BROWSER_CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true,
  args: ['--disable-background-networking', '--disable-component-update', '--no-default-browser-check'] });
if (config.roster) {
  // The roster-only lane (ADR-126 browser scenario): same read-only ticket
  // the usage walk exchanges, one page, no operator token in the browser.
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
    await rosterWalk(page);
    assert(urls.every((url) => !url.includes('access=')), 'no ticket value in a request URL');
    assert(!/private_|operator\.token/.test(await page.locator('main').innerText()), 'no credential value on screen');
    assert.deepEqual(failures, []);
    console.log('PASS native agent roster browser');
  } finally { await browser.close(); }
  process.exit(0);
}
if (config.sides) {
  // The project-sides-only lane (ADR-132 browser scenario): the same
  // read-only ticket the usage walk exchanges, one page, no operator
  // token in the browser, and no credential value on screen.
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
    await projectSidesWalk(page);
    assert(urls.every((url) => !url.includes('access=')), 'no ticket value in a request URL');
    assert(!/private_|operator\.token/.test(await page.locator('main').innerText()), 'no credential value on screen');
    assert.deepEqual(failures, []);
    console.log('PASS native project-sides browser');
  } finally { await browser.close(); }
  process.exit(0);
}
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
  // The alerts page — the operator close path (ADR-124 amendment). Brief 28
  // adds the read-only arm first: this lane's original link is a READ-ONLY
  // session, so the triage buttons must be ABSENT and the notice naming the
  // configuration management link rendered — the same hide rule the
  // resources page applies without `can_configure`. The scoped link then
  // walks the REAL buttons, which render only from the served `next` array.
  // Both lanes; the seed is shared. Returns to the usage page afterwards so
  // the logout assertions below run against the page they were written for.
  await page.goto(`${config.base}/console/alerts/`);
  await page.locator('[data-native-state="ready"]').waitFor();
  assert.match(await page.locator('main').innerText(), /has drawn 100 against a ceiling of 50/);
  assert.match(await page.locator('main').innerText(), /raise the ceiling on preset private_alert_pool/);
  assert((await page.locator('tbody tr[aria-selected]').count()) >= 1, 'the seeded alert row renders and is selectable');
  assert((await page.locator('[data-transition]').count()) === 0, 'a read-only session is offered no triage controls');
  assert.match(await page.locator('main').innerText(), /This session can read alerts|此会话可以查看告警/);
  // The scoped session: exchange its ticket, then the walk is unchanged.
  // Present in the in-process lane; the executable lane reuses the read-only
  // walk because its link is minted by the real subcommand.
  // One access ticket is outstanding at a time (`authority.rs` `issue_scope`
  // replaces it), so the scoped link is minted only now, after the read-only
  // ticket was exchanged above: the in-process lane asks the harness for it
  // and walks the buttons; the executable lane has no scoped walk because its
  // link is minted by the real subcommand once.
  if (!config.executable) {
    console.log('SCOPED_LINK');
    const scopedUrl = JSON.parse((await lines.next()).value).scopedUrl;
    // The exchange lands on the resources page, which carries no
    // `data-native-state` marker; the session cookie is set once its main
    // content renders.
    await page.goto(scopedUrl);
    await page.locator('main').waitFor();
    await page.goto(`${config.base}/console/alerts/`);
    await page.locator('[data-native-state="ready"]').waitFor();
    // The open row offers exactly the served map: acknowledge, resolve, suppress.
    const buttons = page.locator('[data-transition]');
    assert(await buttons.count() === 3, 'the open row serves exactly three transitions');
    assert((await page.locator('[data-transition="acknowledged"]').count()) === 1);
    assert((await page.locator('[data-transition="resolved"]').count()) === 1);
    assert((await page.locator('[data-transition="suppressed"]').count()) === 1);
    // A REAL press: acknowledge, then the served map narrows to resolve/suppress.
    await page.locator('[data-transition="acknowledged"]').click();
    // The page is already in its ready state while the transition request is in
    // flight, so the wait is for the served map to change: the pressed control
    // leaves the DOM when the reply renders (bounded, so a page that never
    // re-renders still fails here rather than passing on the stale set).
    await page.locator('[data-transition="acknowledged"]').waitFor({ state: 'detached', timeout: 10_000 });
    assert((await page.locator('[data-transition="acknowledged"]').count()) === 0, 'acknowledged is no longer offered');
    assert((await buttons.count()) === 2, 'the acknowledged row serves resolve and suppress');
    // To terminal: resolve, and the terminal row serves nothing.
    await page.locator('[data-transition="resolved"]').click();
    await page.locator('[data-transition="resolved"]').waitFor({ state: 'detached', timeout: 10_000 });
    assert((await buttons.count()) === 0, 'resolved is terminal');
    // The console read serves open alerts only, so the resolved row leaves the
    // list and the page shows its no-open-alerts state rather than a terminal
    // notice for a row it no longer lists.
    assert.match(await page.locator('main').innerText(), /No open alerts\. The sweep resolves them|没有未解决的告警/);
  }
  // The engagements page (the console consumer slice, read-only): the list
  // read the usage flow already carries, rendered as triage. Ready state,
  // the seeded engagement's row, the read-only note, no mutating buttons.
  await page.goto(`${config.base}/console/engagements/`);
  await page.locator('[data-native-state="ready"]').waitFor();
  assert((await page.locator('tbody tr').count()) >= 1, 'the seeded engagement renders');
  assert.match(await page.locator('main').innerText(), /UsageWorker|NewUsageWorker/);
  assert.match(await page.locator('main').innerText(), /read-only — creating, verdicts and revocation|只读 —— 创建、裁定与撤销/);
  assert(await page.locator('main button.danger').count() === 0, 'no mutating buttons on the engagements page');
  // Page IN-PAGE through the seeded rows: every page reaching the ready state
  // passed validateEngagements, and the Next button disables on the null
  // cursor. The in-page pager uses the client's own page size, so the three
  // seeded rows fit one page here; the multi-page walk at ?limit=1 is pinned
  // by the Rust console test, not by this driver.
  // The executable lane runs in Chinese, like its logout step below.
  const nextButton = page.locator('button', { hasText: config.executable ? '下一页' : 'Next page' });
  let pages = 1;
  for (let i = 0; i < 6 && (await nextButton.isEnabled()); i += 1) {
    await nextButton.click();
    await page.locator('[data-native-state="ready"]').waitFor();
    pages += 1;
  }
  assert(await nextButton.isDisabled(), 'the cursor exhausts to null and disables Next');
  assert(pages >= 1 && pages <= 7, `walked ${pages} pages`);
  await page.locator('button', { hasText: config.executable ? '第一页' : 'First page' }).click();
  await page.locator('[data-native-state="ready"]').waitFor();
  // The roster page in the full walk too: the same seven-column projection
  // under the same session, bilingually asserted by rosterWalk.
  await rosterWalk(page);
  // The project-sides page likewise: the six-key side cards, the server's
  // gap list, and the credential negatives — bilingual via the walk.
  await projectSidesWalk(page);
  await page.goto(`${config.base}/console/usage/?engagement_id=${config.engagement}`);
  await page.locator('[data-native-state="ready"]').waitFor();
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
