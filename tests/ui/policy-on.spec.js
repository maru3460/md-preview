// 設定「文書を信頼する」（csp.rs の Policy）を ON にしたとき。設定は起動時に決まるので、
// ON にしたサーバ（playwright.config.js の 5 つ目）で開く。
//
// - md と `.html` の文書の JS が動き、`.html` に style-gate が入る。それでも本文の断片などの
//   応答（`Other`）は `script-src 'none'` のまま
// - 外部の画像・iframe が読まれ、「開く」リンクに置き換えない。それでも `data:` の iframe は
//   止まる（指定を消すのではなく `https: http:` を足すため）
// - 文書の JS が外部の画像を読める（drawio の拡張図形が要る形）
//
// 本文の作り方は csp.spec.js / extmedia.spec.js と同じ（応答の差し替えと、一時ディレクトリの md）。
const fs = require('fs');
const os = require('os');
const path = require('path');
const { test, expect, openFolder, treeItem, TRUSTED_URL } = require('./helpers');

const EXT = 'ext.invalid';
// 1x1 の PNG。
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');

const mark = (target, name) => `${target}.dataset.ran = (${target}.dataset.ran || '') + '${name},'`;
const PROBES = (t) => `
<img id="p-onerror" src="/nope-for-csp.png" onerror="${mark(t, 'onerror')}">
<a id="p-js" href="javascript:${mark(t, 'href')}; void 0">js link</a>
<button id="p-click" onclick="${mark(t, 'onclick')}">click</button>
`;

const OPEN_SCRIPT_SRC = "script-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob: https: http:";

const ran = (page, frameSel) => page.evaluate((sel) => {
  const doc = sel ? document.querySelector(sel).contentDocument : document;
  return (doc.documentElement.dataset.ran || '').split(',').filter(Boolean).sort();
}, frameSel || null);

/// 外部への要求を数え、中身を返す（ON なので読まれる）。
async function serveExternal(page) {
  const hits = [];
  await page.route((url) => url.hostname.endsWith(EXT), (route) => {
    const url = route.request().url();
    hits.push(url);
    if (/\.png$/.test(url)) return route.fulfill({ contentType: 'image/png', body: PNG });
    return route.fulfill({ contentType: 'text/html; charset=utf-8', body: '<p id="ext-inner">EXT</p>' });
  });
  return hits;
}

test.describe('文書を信頼する（ON）', () => {
  // `data:` の iframe は ON でも止まる。止まったことを確かめるので、その違反だけは数えない。
  test.use({ cspAllowed: [(v) => v.directive === 'frame-src' && /^data/.test(v.blocked || '')] });

  test('md の本文のイベント属性・javascript:・srcdoc の script が動く（本文の <script> 要素は動かない）', async ({ page }) => {
    const t = 'top.document.documentElement';
    await page.route((url) => /[?&]file=[^&]*a\.md$/.test(url.search), (route) => route.fulfill({
      contentType: 'text/html; charset=utf-8',
      body: '<div class="markdown-body"><h1>見出し A</h1>' + PROBES('document.documentElement') +
        `<script>${mark('document.documentElement', 'script')}</script>` +
        `<iframe id="p-srcdoc" srcdoc="<script>${mark(t, 'srcdoc')}</script>"></iframe>` +
        '<p id="tail">TAIL</p></div>',
    }));
    await openFolder(page, TRUSTED_URL);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();

    await page.locator('#preview-pane #p-click').click();
    await page.locator('#preview-pane #p-js').click();
    await expect.poll(() => ran(page)).toEqual(['href', 'onclick', 'onerror', 'srcdoc']);

    // meta の script-src はハッシュではなく、文書の JS を許す形になっている。外部も通る。
    const meta = await page.evaluate(() => document.querySelector('meta[http-equiv="Content-Security-Policy"]').content);
    expect(meta).toContain(OPEN_SCRIPT_SRC);
    expect(meta).not.toContain("'sha256-");
    expect(meta).toContain("img-src 'self' data: blob: https: http:");
    expect(meta).toContain("frame-src 'self' https: http:");
    expect(await page.evaluate(() => window.MD_TRUSTED)).toBe(true);
  });

  test('.html の中の script とイベント属性が動き、style-gate が入る', async ({ page }) => {
    let header = null;
    let served = null;
    await page.route((url) => url.pathname === '/page.html', async (route) => {
      const resp = await route.fetch();
      header = resp.headers()['content-security-policy'];
      served = await resp.text();
      const body = served.replace('<body>',
        `<body><script>${mark('document.documentElement', 'script')}</script>` + PROBES('document.documentElement'));
      await route.fulfill({ response: resp, body });
    });
    await openFolder(page, TRUSTED_URL);
    await treeItem(page, 'page.html').click();
    const frame = page.frameLocator('iframe.html-frame');
    await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');

    expect(header).toContain(OPEN_SCRIPT_SRC);
    expect(header).toContain("img-src 'self' data: blob: https: http:");
    // 文書の JS より先に走る gate（request.rs の inject_style_gate）。
    expect(served).toContain('/*md:style-gate*/');

    await frame.locator('#p-click').click();
    await frame.locator('#p-js').click();
    await expect.poll(() => ran(page, 'iframe.html-frame')).toEqual(['href', 'onclick', 'onerror', 'script']);

    // 親が外から動かしている機能は、文書の JS が動いていても効く。
    await page.evaluate(() => document.activeElement && document.activeElement.blur());
    await page.keyboard.press('Meta+f');
    await page.locator('.md-search-input').fill('フィクスチャ');
    await expect(page.locator('.md-search-counter')).not.toHaveText('0/0');
  });

  test('本文の断片・JSON・404 の応答は script-src が none のまま', async ({ page }) => {
    await openFolder(page, TRUSTED_URL);
    const got = await page.evaluate(async (root) => {
      const csp = async (u) => (await fetch(u)).headers.get('content-security-policy');
      return {
        fragment: await csp('/?file=' + encodeURIComponent(root + '/a.md')),
        notFound: await csp('/no-such-file.png'),
        lib: await csp('/__lib/mermaid.min.js'),
      };
    }, page.mdRoot);
    for (const [name, value] of Object.entries(got)) {
      expect(value, name).toContain("script-src 'none'");
    }
  });

  test('md の外部の画像と iframe は読まれ、「開く」リンクにならない。data: の iframe は止まる', async ({ page, cspViolations }) => {
    const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'md-ext-on-'));
    fs.writeFileSync(path.join(dir, 'doc.md'), [
      '# 外部を許す',
      '',
      `![構成図](https://${EXT}/arch.png)`,
      '',
      `<iframe id="ext-frame" src="https://${EXT}/embed"></iframe>`,
      '',
      '<iframe id="data-frame" src="data:text/html,<p>DATA</p>"></iframe>',
      '',
    ].join('\n'));
    try {
      const hits = await serveExternal(page);
      await openFolder(page, TRUSTED_URL);
      await page.evaluate((id) => window.MdOpenFiles([id]), path.join(dir, 'doc.md'));
      const body = page.locator('#preview-pane .markdown-body');
      await expect(body.locator('h1')).toHaveText('外部を許す');

      expect(await body.locator('.md-ext-media, .md-ext-chip').count()).toBe(0);
      const img = body.locator('img[alt="構成図"]');
      await expect.poll(() => img.evaluate((el) => el.complete && el.naturalWidth)).toBe(1);
      await expect(page.frameLocator('#ext-frame').locator('#ext-inner')).toHaveText('EXT');
      expect(hits).toEqual(expect.arrayContaining([`https://${EXT}/arch.png`, `https://${EXT}/embed`]));

      await expect.poll(() => cspViolations.filter((v) => v.directive === 'frame-src').length).toBeGreaterThanOrEqual(1);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  test('.html の外部の画像は読まれて置き換えず、script が外部の画像を足せる。data: の iframe は止まる', async ({ page, cspViolations }) => {
    const hits = await serveExternal(page);
    let header = null;
    await page.route((url) => url.pathname === '/page.html', async (route) => {
      const resp = await route.fetch();
      header = resp.headers()['content-security-policy'];
      const body = (await resp.text()).replace('<body>', '<body>' +
        `<p><img id="ext-img" src="https://${EXT}/fig.png" alt="図"></p>` +
        '<iframe id="data-frame" src="data:text/html,<p>DATA</p>"></iframe>' +
        // 文書の JS が外部の画像を足す（drawio の拡張図形と同じ形）。
        '<script>' +
        `var i = new Image(); i.onload = function() { ${mark('document.documentElement', 'loaded')} };` +
        `i.src = 'https://${EXT}/by-script.png';</script>`);
      await route.fulfill({ response: resp, body });
    });
    await openFolder(page, TRUSTED_URL);
    await treeItem(page, 'page.html').click();
    const frame = page.frameLocator('iframe.html-frame');
    await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');

    expect(header).toContain("img-src 'self' data: blob: https: http:");
    expect(header).toContain(OPEN_SCRIPT_SRC);
    const img = frame.locator('#ext-img');
    await expect.poll(() => img.evaluate((el) => el.complete && el.naturalWidth)).toBe(1);
    expect(await frame.locator('[data-md-ext-media], [data-md-ext-chip]').count()).toBe(0);
    await expect.poll(() => ran(page, 'iframe.html-frame')).toEqual(['loaded']);
    expect(hits).toEqual(expect.arrayContaining([`https://${EXT}/fig.png`, `https://${EXT}/by-script.png`]));
    await expect.poll(() => cspViolations.filter((v) => v.directive === 'frame-src').length).toBeGreaterThanOrEqual(1);
  });
});
