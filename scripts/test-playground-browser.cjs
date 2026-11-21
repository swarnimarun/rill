#!/usr/bin/env node
// Run against a local "rill playground" process, using Playwright's Chromium API.
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');

function chromiumDriver() {
  for (const name of ['playwright', 'playwright-core']) {
    let resolved;
    try { resolved = require.resolve(name); } catch (_) { continue; }
    return require(resolved).chromium;
  }
  throw new Error('Install playwright or playwright-core, or set NODE_PATH to its node_modules directory.');
}

async function main() {
  const server = new URL(process.argv[2] || 'http://127.0.0.1:7878/');
  if (server.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(server.hostname)) {
    throw new Error('Supply the loopback HTTP URL printed by rill playground.');
  }
  const browser = await chromiumDriver().launch({
    headless: true,
    ...(process.env.CHROMIUM_EXECUTABLE ? { executablePath: process.env.CHROMIUM_EXECUTABLE } : {}),
  });
  const failures = [];
  const captures = process.env.RILL_PLAYGROUND_SCREENSHOTS;
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      if (url.origin === server.origin) return route.continue();
      failures.push('Unexpected external request ' + url.href);
      return route.abort();
    });
    const page = await context.newPage();
    page.on('pageerror', (error) => failures.push(error.message));
    page.setDefaultTimeout(8000);
    async function result(state, needle) {
      try {
        await page.waitForFunction(([state, needle]) =>
          document.getElementById('output-status').dataset.state === state &&
          document.getElementById('output').textContent.includes(needle), [state, needle]);
      } catch (error) {
        const observed = await page.evaluate(() => ({
          title: document.title,
          description: document.getElementById('description')?.textContent,
          status: document.getElementById('output-status')?.textContent,
          output: document.getElementById('output')?.textContent.slice(0, 512),
        }));
        throw new Error('Expected ' + state + ' with ' + JSON.stringify(needle) +
          ', observed ' + JSON.stringify(observed) + '. Page errors: ' + failures.join(', '), { cause: error });
      }
    }
    async function capture(name) {
      if (!captures) return;
      await fs.mkdir(captures, { recursive: true });
      await page.screenshot({ path: path.join(captures, name + '.png') });
    }
    async function selectLesson(id) {
      await page.locator('#lesson-list a[data-lesson="' + id + '"]').click();
    }

    server.searchParams.set('lesson', 'signal-gain');
    await page.goto(server.href);
    await result('done', '42');
    assert.equal(await page.locator('#source-file').textContent(), 'signal-gain.rill');
    assert.ok((await page.locator('#view-help').textContent()).trim());
    const original = await page.locator('#source').inputValue();
    await capture('desktop');
    assert.equal(await page.locator('#reset').isDisabled(), true);

    await page.locator('#source').fill('fn main() { 73 }');
    assert.equal(await page.locator('#output-status').getAttribute('data-state'), 'stale');
    await page.locator('#source').press('Control+Enter');
    await result('done', '73');
    await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: server.origin });
    await page.locator('#copy').click();
    await page.waitForFunction(() => document.getElementById('copy').textContent === 'Copied');
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), await page.locator('#output').textContent());
    for (const [stage, needle] of [['tokens', 'main'], ['ast', 'main'], ['types', 'main'], ['format', '73'], ['c', 'int main(void)']]) {
      await page.locator('#view').selectOption(stage);
      await result('done', needle);
    }
    await page.locator('#view').selectOption('typed-ir');
    await result('done', '73');
    await page.locator('#view').selectOption('check');
    await result('done', '');
    await page.reload();
    await result('done', '');
    assert.equal(await page.locator('#source').inputValue(), 'fn main() { 73 }');
    assert.equal(await page.locator('#view').inputValue(), 'check');

    await selectLesson('operand-error');
    await result('error', "right operand of '+'");
    await page.locator('#source').fill('fn main() { 3 }');
    await page.locator('#run').click();
    await result('done', '');
    await selectLesson('signal-gain');
    await result('done', '');
    assert.equal(await page.locator('#source').inputValue(), 'fn main() { 73 }');
    page.once('dialog', (dialog) => dialog.accept());
    await page.locator('#reset').click();
    await result('done', '42');
    assert.equal(await page.locator('#source').inputValue(), original);

    // Hold one request until after an edit. Its response must never describe the newer buffer.
    let release;
    let started;
    const held = new Promise((resolve) => { release = resolve; });
    const requested = new Promise((resolve) => { started = resolve; });
    await page.route('**/api/inspect', async (route) => {
      started();
      await held;
      await route.fulfill({ json: { ok: true, output: 'obsolete response', diagnostics: '' } });
    }, { times: 1 });
    await page.locator('#run').click();
    await requested;
    await page.locator('#source').fill('fn main() { 123 }');
    release();
    await page.unrouteAll({ behavior: 'wait' });
    assert.equal(await page.locator('#output-status').getAttribute('data-state'), 'stale');
    assert.doesNotMatch(await page.locator('#output').textContent(), /obsolete response/);
    await page.locator('#run').click();
    await result('done', '123');

    await selectLesson('signed-bands');
    await result('done', '0\n1\n1\n2');
    await page.locator('#inputs').fill('2147483648');
    await page.locator('#run').click();
    await result('error', 'i32');
    for (const [stage, needle] of [['rir', 'slt.i32'], ['nvptx', 'nvptx64-nvidia-cuda'], ['vulkan', 'vulkan1.2-compute']]) {
      await page.locator('#view').selectOption(stage);
      await result('done', needle);
    }
    await page.locator('#entry').fill('missing_entry');
    await page.locator('#run').click();
    await result('error', 'missing_entry');
    await page.locator('#filter').fill('missing lesson text');
    assert.equal(await page.locator('#filter-empty').isVisible(), true);
    await page.locator('#filter').fill('');

    await page.setViewportSize({ width: 390, height: 844 });
    await page.locator('#toggle-lessons').click();
    await selectLesson('optional-reading');
    await result('done', '18');
    assert.equal(await page.locator('#toggle-lessons').getAttribute('aria-expanded'), 'false');
    const overflowing = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
    assert.equal(overflowing, false, 'mobile page should fit the viewport');
    await page.evaluate(() => scrollTo(0, 0));
    await capture('mobile');
    await page.locator('.book-link').click();
    assert.equal(new URL(page.url()).pathname, '/book/');
    assert.ok((await page.locator('h1').textContent()).trim());
    await page.locator('.mobile-navigation summary').click();
    await page.locator('.mobile-navigation').getByRole('link', { name: 'Compiler explorer', exact: true }).click();
    await page.waitForSelector('#lesson-title');
    assert.equal(new URL(page.url()).pathname, '/');
    assert.deepEqual(failures, []);
    console.log('Playground browser checks passed for desktop and mobile.');
  } finally {
    await browser.close();
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
