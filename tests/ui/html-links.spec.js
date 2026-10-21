// `.html` のプレビュー（同じ origin の iframe）の中の外部リンクは、押すと内線の `open-url:` で
// 既定のブラウザへ渡り、iframe は移らないこと（common.js の bindFrame）。
//
// 窓では、iframe の中の移動はシェルの CSP（frame-src 'self'）が移動の判定より手前で止めるので、
// ここで拾わないと普通のリンクは押しても何も起きない。開発用サーバの内線はスタブで、送った本文を
// `window.__mdIpc` に積むだけなので、そこを数える。
// 外部の住所は `ext.invalid` に寄せ、そこへの要求を止める（来ても読み込ませない）。
const { test, expect, openFolder, treeItem } = require('./helpers');

const EXT = 'ext.invalid';

/// フィクスチャの page.html に、外部へのリンクを足して開く。ヘッダの CSP は本物の応答のまま。
async function openPage(page) {
  await page.route((url) => url.hostname.endsWith(EXT), (route) => route.abort());
  await page.route((url) => url.pathname === '/page.html', async (route) => {
    const resp = await route.fetch();
    const body = (await resp.text()).replace('<body>', '<body>' + [
      `<p><a id="plain" href="https://${EXT}/plain?q=1#h">普通のリンク</a></p>`,
      `<p><a id="blank" target="_blank" href="https://${EXT}/blank">新しい窓のリンク</a></p>`,
      `<p><a id="top" target="_top" href="http://${EXT}/top">_top のリンク</a></p>`,
      `<p><a id="nested" href="https://${EXT}/nested"><span>中の文字</span></a></p>`,
      `<svg width="200" height="30"><a id="svg" xlink:href="https://${EXT}/svg"><text x="2" y="20">SVG のリンク</text></a></svg>`,
      `<map name="m"><area id="area" shape="rect" coords="0,0,40,20" href="https://${EXT}/area"></map>`,
      '<img usemap="#m" width="40" height="20" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=">',
      // 外部の画像は extmedia.js が「開く」リンクに置き換える。
      `<p><img id="ext-img" src="https://${EXT}/fig.png" alt="図"></p>`,
      '<p><a id="local" href="#here">ページ内の錨</a></p>',
    ].join('\n'));
    await route.fulfill({ response: resp, body });
  });
  await openFolder(page);
  await treeItem(page, 'page.html').click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await expect(frame.locator('a[data-md-ext-media]')).toHaveCount(1);
  return frame;
}

const sent = (page) => page.evaluate(() =>
  window.__mdIpc.filter((m) => m.startsWith('open-url:')).map((m) => m.slice('open-url:'.length)));

const frameUrl = (page) => page.frames().map((f) => f.url()).find((u) => u.includes('page.html'));

test.describe('.html の中の外部リンク', () => {
  // 外部の画像は、置き換える前に一度は読みにいって CSP に止められる。合成のクリックは移動を
  // 起こし、それも CSP に止められる。どちらも外部の住所のものだけを数えない。
  test.use({ cspAllowed: [(v) => (v.blocked || '').includes(EXT)] });

  test('押した外部リンクは open-url: を 1 回だけ送り、iframe は移らず、窓も開かない', async ({ page }) => {
    const frame = await openPage(page);
    const before = frameUrl(page);
    let popups = 0;
    page.on('popup', () => { popups++; });

    const cases = [
      ['#plain', `https://${EXT}/plain?q=1#h`],
      ['#blank', `https://${EXT}/blank`],
      ['#top', `http://${EXT}/top`],
      ['#nested span', `https://${EXT}/nested`],
      ['#svg text', `https://${EXT}/svg`],
      ['a[data-md-ext-media]', `https://${EXT}/fig.png`],
    ];
    for (const [sel, url] of cases) {
      await page.evaluate(() => { window.__mdIpc.length = 0; });
      await frame.locator(sel).click();
      await expect.poll(() => sent(page), sel).toEqual([url]);
    }
    // area は大きさの無い要素なので、画像の上を押す。
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await frame.locator('img[usemap]').click({ position: { x: 5, y: 5 } });
    await expect.poll(() => sent(page)).toEqual([`https://${EXT}/area`]);

    await page.waitForTimeout(300);
    expect(await sent(page)).toEqual([`https://${EXT}/area`]);
    expect(frameUrl(page)).toBe(before);
    expect(popups).toBe(0);
    await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  });

  test('外部でないリンクは送らない', async ({ page }) => {
    const frame = await openPage(page);
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await frame.locator('#local').click();
    await page.waitForTimeout(200);
    expect(await sent(page)).toEqual([]);
  });

  test('script の合成したクリックでは送らない', async ({ page }) => {
    const frame = await openPage(page);
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    // 文書の中からの click() と、親からの dispatchEvent（どちらも isTrusted が false）。
    await frame.locator('#plain').evaluate((a) => a.click());
    await frame.locator('#blank').evaluate((a) =>
      a.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 })));
    await page.waitForTimeout(300);
    expect(await sent(page)).toEqual([]);
  });
});
