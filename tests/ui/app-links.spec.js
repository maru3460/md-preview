// 本文と `.html` のプレビューの中のリンクのうち、窓の中のファイルを指すものは、`target` や
// 要素の種類（`<area>`・SVG のリンク）に関係なくタブで開くこと（folder.js のクリック処理）。
// 窓のメインフレームは md の画面から動かない（webview_delegate が取り消す）ので、拾い損ねると
// 窓では押しても何も起きない。Playwright は webview_delegate を通らないので、ここでは
// 「アプリがクリックを横取りしたか（defaultPrevented）」と「どのタブが開いたか」を見る。
//
// あわせて、`.html` の中の入れ子の iframe の外部リンクも内線の `open-url:` へ回ること（common.js）。
const { test, expect, openFolder, treeItem, tab } = require('./helpers');

const EXT = 'ext.invalid';
const GIF = 'data:image/gif;base64,R0lGODlhAQABAAAAACw=';

/// b.md の本文に、窓の中を指すリンクを足す。本文の href はサーバが URL へ畳んだ形で来るので、
/// ここでも畳んだ形（`/` から始まる URL）で書く。
async function routeBodyLinks(page) {
  await page.route((url) => url.pathname === '/' && (url.searchParams.get('file') || '').endsWith('/b.md'), async (route) => {
    const resp = await route.fetch();
    const body = (await resp.text()).replace('</h1>', '</h1>' + [
      '<p><a id="img" href="/sub/fig.svg">図へ</a></p>',
      '<p><a id="txt" href="/notes.txt">メモへ</a></p>',
      '<p><a id="blank" target="_blank" href="/a.md">a を新しい窓で</a></p>',
      '<svg width="200" height="30"><a id="svg" xlink:href="/a.md"><text x="2" y="20">SVG のリンク</text></a></svg>',
      `<p><map name="m"><area shape="rect" coords="0,0,40,20" href="/notes.txt"></map><img id="map" usemap="#m" width="40" height="20" src="${GIF}"></p>`,
      '<p><a id="vscode" href="vscode://file/a.md">vscode で開く</a></p>',
      '<p><a id="query" href="/?file=/etc/hosts">クエリで引く口</a></p>',
      '<p><a id="dir" href="/sub/">フォルダへ</a></p>',
    ].join('\n'));
    await route.fulfill({ response: resp, body });
  });
}

/// アプリのクリック処理より後（window の泡立ちの段）で、横取りされたかを記録し、横取り
/// されなかった移動はここで止める（Playwright の窓には移動の判定が無いので、素通しすると
/// 窓ごと移ってしまう）。
async function recordClicks(page) {
  await page.evaluate(() => {
    window.__clicks = [];
    window.addEventListener('click', (e) => {
      window.__clicks.push(e.defaultPrevented);
      e.preventDefault();
    });
  });
}

async function openB(page) {
  await treeItem(page, 'b.md').click();
  await expect(tab(page, 'b.md')).toHaveClass(/active/);
  await expect(page.locator('#preview-pane #img')).toBeVisible();
}

test.describe('窓の中を指すリンク', () => {
  test('本文のリンクは、画像・テキスト・_blank・SVG・area でもタブで開く', async ({ page }) => {
    await routeBodyLinks(page);
    await openFolder(page);
    await recordClicks(page);
    let popups = 0;
    page.on('popup', () => { popups++; });

    const cases = [
      ['#img', 'sub/fig.svg'],
      ['#txt', 'notes.txt'],
      ['#blank', 'a.md'],
      ['#svg text', 'a.md'],
    ];
    for (const [sel, rel] of cases) {
      await openB(page);
      await page.locator('#preview-pane ' + sel).click();
      await expect(tab(page, rel), sel).toHaveClass(/active/);
    }
    // area は大きさの無い要素なので、画像の上を押す。
    await openB(page);
    await page.locator('#preview-pane #map').click({ position: { x: 5, y: 5 } });
    await expect(tab(page, 'notes.txt')).toHaveClass(/active/);

    expect(await page.evaluate(() => window.__clicks)).toEqual([true, true, true, true, true]);
    expect(popups).toBe(0);
  });

  test('外部スキームと、パスが / の口はタブで開かない', async ({ page }) => {
    await routeBodyLinks(page);
    await openFolder(page);
    await recordClicks(page);
    await openB(page);
    const tabs = await page.locator('.md-tab').count();

    // vscode: は拡張子が .md でも横取りしない（移動の判定に任せる）。
    await page.locator('#preview-pane #vscode').click();
    // `/?file=` は横取りして、何も開かない（窓では md の画面の読み込み直しになるため）。
    await page.locator('#preview-pane #query').click();
    await page.waitForTimeout(200);

    expect(await page.evaluate(() => window.__clicks)).toEqual([false, true]);
    await expect(page.locator('.md-tab')).toHaveCount(tabs);
    await expect(tab(page, 'b.md')).toHaveClass(/active/);
  });

  test('フォルダへのリンクは横取りして、何も開かない', async ({ page }) => {
    await routeBodyLinks(page);
    await openFolder(page);
    await recordClicks(page);
    await openB(page);
    const tabs = await page.locator('.md-tab').count();

    await page.locator('#preview-pane #dir').click();
    await page.waitForTimeout(200);

    expect(await page.evaluate(() => window.__clicks)).toEqual([true]);
    await expect(page.locator('.md-tab')).toHaveCount(tabs);
    await expect(tab(page, 'b.md')).toHaveClass(/active/);
  });
});

/// フィクスチャの page.html に、窓の中を指すリンクと入れ子の iframe を足して開く。
async function openPage(page) {
  await page.route((url) => url.hostname.endsWith(EXT), (route) => route.abort());
  await page.route((url) => url.pathname === '/page.html', async (route) => {
    const resp = await route.fetch();
    const body = (await resp.text()).replace('<body>', '<body>' + [
      '<p><a id="txt" href="notes.txt">メモへ</a></p>',
      '<p><a id="blank" target="_blank" href="b.md">b を新しい窓で</a></p>',
      '<svg width="200" height="30"><a id="svg" href="b.md"><text x="2" y="20">SVG のリンク</text></a></svg>',
      `<p><map name="m"><area shape="rect" coords="0,0,40,20" href="notes.txt"></map><img id="map" usemap="#m" width="40" height="20" src="${GIF}"></p>`,
      '<p><a id="mail" href="mailto:a@example.com">メール</a></p>',
      '<p><a id="vscode" href="vscode://file/Users/me/a.md">vscode で開く</a></p>',
      '<p><a id="obsidian" href="obsidian://open?vault=x">obsidian で開く</a></p>',
      '<p><a id="dir" href="sub/">フォルダへ</a></p>',
      `<iframe id="nest" srcdoc="<a id=n href=https://${EXT}/nested>入れ子のリンク</a><a id=self href=#x>錨</a><a id=nm href=mailto:b@example.com>入れ子のメール</a>"></iframe>`,
    ].join('\n'));
    await route.fulfill({ response: resp, body });
  });
  await openFolder(page);
  await treeItem(page, 'page.html').click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  return frame;
}

const sent = (page) => page.evaluate(() =>
  window.__mdIpc.filter((m) => m.startsWith('open-url:')).map((m) => m.slice('open-url:'.length)));

test.describe('.html の中のリンク', () => {
  // 入れ子の外部リンクは、合成でないクリックでも移動が CSP に止められうる。外部の住所のものだけを数えない。
  test.use({ cspAllowed: [(v) => (v.blocked || '').includes(EXT)] });

  test('窓の中を指すリンクは、テキスト・_blank・SVG・area でもタブで開く', async ({ page }) => {
    const cases = [
      ['#txt', 'notes.txt'],
      ['#blank', 'b.md'],
      ['#svg text', 'b.md'],
    ];
    for (const [sel, rel] of cases) {
      const frame = await openPage(page);
      await frame.locator(sel).click();
      await expect(tab(page, rel), sel).toHaveClass(/active/);
      await page.unrouteAll({ behavior: 'ignoreErrors' });
    }
    const frame = await openPage(page);
    await frame.locator('#map').click({ position: { x: 5, y: 5 } });
    await expect(tab(page, 'notes.txt')).toHaveClass(/active/);
  });

  test('フォルダへのリンクはタブを開かない', async ({ page }) => {
    const frame = await openPage(page);
    const tabs = await page.locator('.md-tab').count();
    await frame.locator('#dir').click();
    await page.waitForTimeout(200);
    await expect(page.locator('.md-tab')).toHaveCount(tabs);
    await expect(tab(page, 'page.html')).toHaveClass(/active/);
    await expect(frame.locator('#dir')).toBeVisible();
  });

  // 外部スキームも CSP が移動の判定より手前で止めるので、http(s) と同じく内線へ回す。
  // どれを開くか（許可リスト）は Rust の open_url_decision が決めるので、ここでは送るだけ。
  test('外部スキームのリンクも open-url: を送る。合成のクリックでは送らない', async ({ page }) => {
    const frame = await openPage(page);
    for (const [sel, url] of [
      ['#mail', 'mailto:a@example.com'],
      ['#vscode', 'vscode://file/Users/me/a.md'],
      ['#obsidian', 'obsidian://open?vault=x'],
    ]) {
      await page.evaluate(() => { window.__mdIpc.length = 0; });
      await frame.locator(sel).click();
      await expect.poll(() => sent(page), sel).toEqual([url]);
    }

    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await frame.frameLocator('#nest').locator('#nm').click();
    await expect.poll(() => sent(page)).toEqual(['mailto:b@example.com']);

    // 文書の script が click() で押したもの（isTrusted でない）は送らない。
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await frame.locator('#mail').evaluate((a) => {
      a.addEventListener('click', (e) => e.preventDefault(), { once: true });
      a.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    });
    await page.waitForTimeout(200);
    expect(await sent(page)).toEqual([]);
  });

  test('入れ子の iframe の外部リンクも open-url: を 1 回だけ送り、後から足した iframe にも効く', async ({ page }) => {
    const frame = await openPage(page);
    const nested = frame.frameLocator('#nest');
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await nested.locator('#n').click();
    await expect.poll(() => sent(page)).toEqual([`https://${EXT}/nested`]);

    // 錨は送らない。
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await nested.locator('#self').click();
    await page.waitForTimeout(200);
    expect(await sent(page)).toEqual([]);

    // 文書が後から足した iframe（ここでは外から足す）。
    await frame.locator('body').evaluate((body, ext) => {
      const f = document.createElement('iframe');
      f.id = 'late';
      f.srcdoc = `<a id=m href=https://${ext}/late>後から</a>`;
      body.appendChild(f);
    }, EXT);
    const late = frame.frameLocator('#late');
    await expect(late.locator('#m')).toBeVisible();
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await late.locator('#m').click();
    await expect.poll(() => sent(page)).toEqual([`https://${EXT}/late`]);
  });
});
