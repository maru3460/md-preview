// 外部の読み込みを止めた既定（設定「外部の読み込みを許す」が OFF）で、外部の画像・iframe・動画・
// 音声が「開く」リンクに置き換わること（md は extmedia.rs、.html は extmedia.js）。
//
// 外部の住所は `ext.invalid` に寄せ、そこへの要求を全部数える。CSP が止めたものは要求に
// ならないので、1 件でも来たら守りが抜けている。
// md は本物の描画を通すため、一時ディレクトリの md を転送の口（MdOpenFiles）で開く
// （フィクスチャを足すとツリーの並びを見ている spec に響く）。`.html` は本物の応答を取ってから
// 中身だけを替える（ヘッダの CSP をそのまま残す。csp.spec.js と同じやり方）。
const fs = require('fs');
const os = require('os');
const path = require('path');
const { test, expect, openFolder, treeItem } = require('./helpers');

const EXT = 'ext.invalid';
// 1x1 の PNG。
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');

/// 外部への要求を数える。来たら落とすために、中身は返さず止める。
async function countExternal(page) {
  const hits = [];
  await page.route((url) => url.hostname.endsWith(EXT), (route) => {
    hits.push(route.request().url());
    return route.abort();
  });
  return hits;
}

test.describe('md の本文', () => {
  // 置き換えない `<object>` だけは CSP が止める（違反が出る）。
  test.use({ cspAllowed: [(v) => v.directive === 'object-src' && v.blocked.includes(EXT)] });

  test('外部の画像・iframe・動画は「開く」リンクになり、読み込まれない', async ({ page }) => {
    const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'md-extmedia-'));
    fs.writeFileSync(path.join(dir, 'local.png'), PNG);
    fs.writeFileSync(path.join(dir, 'doc.md'), [
      '# 外部の読み込み',
      '',
      `![構成図](https://${EXT}/arch.png)`,
      '',
      `[![](https://${EXT}/badge/build-passing.svg)](https://example.com/ci)`,
      '',
      `<img id="html-alt" src="https://${EXT}/x.png" alt="<b>太字</b>">`,
      '',
      '<img id="js-img" src="javascript:alert(1)" alt="js">',
      '',
      `<iframe src="https://${EXT}/embed" title="埋め込み"></iframe>`,
      '',
      // 閉じていない `<a name>` の中はリンクの中ではない（チップにせず、開くリンクにする）。
      `<p><a name="top">錨<img src="https://${EXT}/anchor.png" alt="錨の図"></p>`,
      '',
      // `//host/…` も外部。リンクの URL は読み直した形（スキームとホストが小文字）。
      `![相対](//${EXT}/pr.png) ![大文字](HTTPS://EXT.INVALID/Up.png)`,
      '',
      `<video src="https://${EXT}/v.mp4" controls></video>`,
      '',
      `<object data="https://${EXT}/o.pdf"></object>`,
      '',
      '![手元](local.png)',
      '',
    ].join('\n'));
    try {
      const hits = await countExternal(page);
      await openFolder(page);
      await page.evaluate((id) => window.MdOpenFiles([id]), path.join(dir, 'doc.md'));
      const body = page.locator('#preview-pane .markdown-body');
      await expect(body.locator('h1')).toHaveText('外部の読み込み');

      const links = body.locator('a.md-ext-media');
      await expect(links).toHaveText([
        `外部の画像: 構成図（${EXT}）`,
        `外部の画像: <b>太字</b>（${EXT}）`,
        `外部の埋め込み: 埋め込み（${EXT}）`,
        `外部の画像: 錨の図（${EXT}）`,
        `外部の画像: 相対（${EXT}）`,
        `外部の画像: 大文字（${EXT}）`,
        `外部の動画: ${EXT}`,
      ]);
      await expect(links.first()).toHaveAttribute('href', `https://${EXT}/arch.png`);
      await expect(links.nth(4)).toHaveAttribute('href', `https://${EXT}/pr.png`);
      await expect(links.nth(5)).toHaveAttribute('href', `https://${EXT}/Up.png`);
      expect(await body.locator('a[name="top"] .md-ext-chip').count()).toBe(0);
      // alt に書いた HTML は文字のまま。
      expect(await body.locator('a.md-ext-media b').count()).toBe(0);
      // バッジは外側のリンクの中のチップ。リンクを入れ子にしない。
      const outer = body.locator('a[href="https://example.com/ci"]');
      await expect(outer.locator('.md-ext-chip')).toHaveText('build-passing');
      await expect(outer.locator('.md-ext-chip')).toHaveAttribute('title', EXT);
      expect(await outer.locator('a').count()).toBe(0);
      // http(s) 以外は開く口を作らずに消す。
      expect(await body.locator('#js-img').count()).toBe(0);
      expect(await body.locator('a[href^="javascript:"]').count()).toBe(0);
      expect(await body.locator(`img[src*="${EXT}"], iframe, video`).count()).toBe(0);

      // 手元の画像は読める。
      const local = body.locator('img[alt="手元"]');
      await expect.poll(() => local.evaluate((img) => img.complete && img.naturalWidth)).toBe(1);
      await page.waitForTimeout(300);
      expect(hits).toEqual([]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

test.describe('.html の中', () => {
  // `.html` は描くのが WebView なので、置き換える前に一度は読みにいって CSP に止められる。
  // 止めた先が外部の住所か、開けないスキームのものだけを数えない。
  test.use({ cspAllowed: [(v) => (v.blocked || '').includes(EXT) || /^vscode/.test(v.blocked || '')] });

  test('外部の画像・picture・iframe・動画は「開く」リンクになり、手元のものは残る', async ({ page }) => {
    const hits = await countExternal(page);
    await page.route((url) => url.pathname === '/page.html', async (route) => {
      const resp = await route.fetch();
      const body = (await resp.text()).replace('<body>', '<body>' + [
        // 文書の要素で document のメソッドの名前をすり替えても、置き換えは動く。
        // （querySelectorAll はすり替えない。Playwright 自身の locator が壊れる）
        '<img name="createElement"><img name="createElementNS"><form name="URL"></form>',
        `<link rel="stylesheet" href="https://${EXT}/x.css">`,
        `<style>@font-face { font-family: Ext; src: url(https://${EXT}/f.woff); } h1 { font-family: Ext; }</style>`,
        `<p><img id="plain" src="https://${EXT}/fig.png" alt="図"></p>`,
        `<p><a id="badge" href="https://example.com/ci"><img src="https://${EXT}/badge/build.svg" alt=""></a></p>`,
        `<picture><source media="(prefers-color-scheme: dark)" srcset="https://${EXT}/d.png"><source srcset="https://${EXT}/l.png 1x, https://${EXT}/l2.png 2x"><img src="https://${EXT}/l.png" alt="ロゴ"></picture>`,
        `<iframe src="https://${EXT}/embed" title="動画プレイヤー"></iframe>`,
        `<p><a name="top">錨<img src="https://${EXT}/anchor.png" alt="錨の図"></p>`,
        `<p><img src="//${EXT}/pr.png" alt="相対"></p>`,
        `<video controls><source src="https://${EXT}/a.mp4"></video>`,
        '<img id="bad" src="vscode://file/etc/hosts" alt="x">',
        `<img id="local" src="sub/fig.svg" srcset="sub/fig.svg 1x, https://${EXT}/fig@2x.png 2x" alt="手元">`,
        `<svg id="sv" width="120" height="40"><image href="https://${EXT}/s.png" x="2" y="4" width="100" height="30"/></svg>`,
      ].join('\n'));
      await route.fulfill({ response: resp, body });
    });
    await openFolder(page);
    await treeItem(page, 'page.html').click();
    const frame = page.frameLocator('iframe.html-frame');
    await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');

    const links = frame.locator('a[data-md-ext-media]');
    await expect(links).toHaveText([
      `外部の画像: 図（${EXT}）`,
      `外部の画像: ロゴ（${EXT}）`,
      `外部の埋め込み: 動画プレイヤー（${EXT}）`,
      `外部の画像: 錨の図（${EXT}）`,
      `外部の画像: 相対（${EXT}）`,
      `外部の動画: ${EXT}`,
    ]);
    await expect(links.first()).toHaveAttribute('href', `https://${EXT}/fig.png`);
    // 開発用サーバの文書は http なので、`//host` は http の外部。
    await expect(links.nth(4)).toHaveAttribute('href', `http://${EXT}/pr.png`);
    // `<a name>` の中はリンクの中ではない。
    expect(await frame.locator('a[name="top"] [data-md-ext-chip]').count()).toBe(0);
    // 札は周りの文字の色で、薄くしない。
    const style = await links.first().evaluate((a) => {
      const cs = getComputedStyle(a), p = getComputedStyle(a.parentElement);
      return { same: cs.color === p.color, opacity: cs.opacity, line: cs.textDecorationLine };
    });
    expect(style).toEqual({ same: true, opacity: '1', line: 'underline' });
    // picture は中の source がいくつあっても 1 個。
    expect(await frame.locator('picture, source, iframe, video').count()).toBe(0);
    // バッジは外側のリンクの中のチップ。
    const chip = frame.locator('#badge [data-md-ext-chip]');
    await expect(chip).toHaveText('build');
    await expect(chip).toHaveAttribute('title', EXT);
    expect(await frame.locator('#badge a').count()).toBe(0);
    // http(s) 以外は消す。
    expect(await frame.locator('#bad').count()).toBe(0);
    // 手元の画像は残し、srcset の外の候補だけ外す。
    await expect(frame.locator('#local')).toHaveAttribute('srcset', 'sub/fig.svg 1x');
    await expect.poll(() => frame.locator('#local').evaluate((img) => img.complete && img.naturalWidth)).toBeGreaterThan(0);
    // SVG の中は SVG の文字で。
    await expect(frame.locator('#sv a text')).toHaveText(`外部の画像: ${EXT}`);
    expect(await frame.locator(`img[src*="${EXT}"], #sv image`).count()).toBe(0);

    await page.waitForTimeout(300);
    expect(hits).toEqual([]);
  });
});

test.describe('.html の URL の分け方', () => {
  // 本番の文書は mdpreview://localhost で、開発用サーバの上では作れないので、分け方を直に見る。
  test('mdpreview: は文書と同じホストだけが手元で、`//host` は https の外部', async ({ page }) => {
    await openFolder(page);
    const got = await page.evaluate(() => {
      const doc = 'mdpreview://localhost/docs/a.html';
      const c = (raw) => window.MdExtMedia.classifyUrl(raw, doc);
      return {
        same: c('mdpreview://localhost/x.png').kind,
        relative: c('sub/x.png').kind,
        other: c('mdpreview://other-host/x.png').kind,
        pr: c('//cdn.example/x.png'),
        backslash: c('\\\\cdn.example/x.png').url,
        httpSameHost: window.MdExtMedia.classifyUrl('//127.0.0.1:1/x.png', 'http://127.0.0.1:7878/a.html').kind,
      };
    });
    expect(got).toEqual({
      same: 'local',
      relative: 'local',
      other: 'blocked',
      pr: { kind: 'external', url: 'https://cdn.example/x.png', host: 'cdn.example' },
      backslash: 'https://cdn.example/x.png',
      httpSameHost: 'external',
    });
  });
});
