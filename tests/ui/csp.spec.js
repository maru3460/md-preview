// 文書の JavaScript を止める CSP（csp.rs）。既定（「文書を信頼する」が OFF）のサーバで、md の本文と `.html` の
// 中の script・イベント属性・`javascript:` が動かないこと、アプリ自身の JS は動くこと。
//
// わざと止める文書を開くので、ここだけは違反が起きる。数えない違反を `cspAllowed` で渡し、
// 違反が実際に起きた（＝CSP が止めた）ことも `cspViolations` で確かめる。
// 本文は応答を差し替えて作る（フィクスチャを足すとツリーの並びを見ている spec に響く）。
// `.html` は本物の応答を取ってから中身だけを替える。ヘッダの CSP をそのまま残すため。
const fs = require('fs');
const os = require('os');
const path = require('path');
const { test, expect, openFolder, treeItem } = require('./helpers');

/// 文書の側で起きた違反（イベント属性・inline の script・`javascript:`）だけを数えない。
/// アプリのファイルや `/__lib/` が止められたら、ここでは許さずに落とす。
const docOnly = (v) => v.directive === 'script-src' || v.directive === 'script-src-elem' || v.directive === 'script-src-attr';

/// 動いたら印を付ける仕掛け。どれか 1 つでも動けば `data-pwned` に名前が残る。
const PROBES = (target) => `
<img id="p-onerror" src="/nope-for-csp.png" onerror="${target}.dataset.pwned = (${target}.dataset.pwned || '') + 'onerror,'">
<a id="p-js" href="javascript:${target}.dataset.pwned = (${target}.dataset.pwned || '') + 'href,'; void 0">js link</a>
<button id="p-click" onclick="${target}.dataset.pwned = (${target}.dataset.pwned || '') + 'onclick,'">click</button>
`;

test.describe('既定では文書の JavaScript が動かない', () => {
  test.use({ cspAllowed: [docOnly] });

  test('md の本文のイベント属性・javascript:・srcdoc の script は動かない', async ({ page, cspViolations }) => {
    const top = 'top.document.documentElement';
    await page.route((url) => /[?&]file=[^&]*a\.md$/.test(url.search), (route) => route.fulfill({
      contentType: 'text/html; charset=utf-8',
      body: '<div class="markdown-body"><h1>見出し A</h1>' + PROBES('document.documentElement') +
        `<iframe id="p-srcdoc" srcdoc="<script>${top}.dataset.pwned = 'srcdoc'</script>"></iframe>` +
        '<p id="tail">TAIL</p></div>',
    }));
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();

    await page.locator('#preview-pane #p-click').click();
    await page.locator('#preview-pane #p-js').click();
    // srcdoc は読み込みが別に走るので、違反が届くまで待つ。
    await expect.poll(() => cspViolations.length).toBeGreaterThanOrEqual(3);
    expect(await page.evaluate(() => document.documentElement.dataset.pwned)).toBeUndefined();

    // 止めたのは md の画面の meta。ハッシュとライブラリ 2 つだけを許している。
    const meta = await page.evaluate(() => document.querySelector('meta[http-equiv="Content-Security-Policy"]').content);
    expect(meta).toContain("script-src 'sha256-");
    expect(meta).not.toContain("'unsafe-inline' 'unsafe-eval'");
    // 起動時の CSP は、文書が同じ名前の要素を置いてもすり替わらないよう必ず定義してある。
    expect(await page.evaluate(() => window.MD_TRUSTED)).toBe(false);
  });

  test('.html の中の script・イベント属性・javascript: は動かず、アプリの検索とコメントは効く', async ({ page, cspViolations }) => {
    let header = null;
    await page.route((url) => url.pathname === '/page.html', async (route) => {
      const resp = await route.fetch();
      header = resp.headers()['content-security-policy'];
      const body = (await resp.text()).replace('<body>',
        '<body><script>document.documentElement.dataset.pwned = "script";</script>' + PROBES('document.documentElement'));
      await route.fulfill({ response: resp, body });
    });
    await openFolder(page);
    await treeItem(page, 'page.html').click();
    const frame = page.frameLocator('iframe.html-frame');
    await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');

    expect(header).toContain("script-src 'none'");
    await frame.locator('#p-click').click();
    await frame.locator('#p-js').click();
    await expect.poll(() => cspViolations.length).toBeGreaterThanOrEqual(3);
    const pwned = await page.evaluate(() =>
      document.querySelector('iframe.html-frame').contentDocument.documentElement.dataset.pwned);
    expect(pwned).toBeUndefined();

    // 親が外から動かしている機能は、中の script が止まっていても効く。
    await page.evaluate(() => document.activeElement && document.activeElement.blur());
    await page.keyboard.press('Meta+f');
    await page.locator('.md-search-input').fill('フィクスチャ');
    await expect(page.locator('.md-search-counter')).not.toHaveText('0/0');
    await page.keyboard.press('Escape');
    // コメント: c でモードに入ると、iframe の中の行にカーソルの枠が重なる。
    await page.evaluate(() => document.activeElement && document.activeElement.blur());
    await page.keyboard.press('c');
    await expect(page.locator('.md-cmt-frame-mark[data-state="kbcursor"]')).toBeVisible();
    await page.keyboard.press('Escape');
  });
});

test('iframe の中に開いた md の画面では、アプリの script が何もせずに抜ける', async ({ page }) => {
  await page.route((url) => /[?&]file=[^&]*a\.md$/.test(url.search), (route) => route.fulfill({
    contentType: 'text/html; charset=utf-8',
    body: '<div class="markdown-body"><h1>見出し A</h1><iframe id="inner" src="/" style="width:600px;height:300px"></iframe></div>',
  }));
  await openFolder(page);
  await treeItem(page, 'a.md').click();
  const inner = page.frames().find((f) => f.parentFrame() === page.mainFrame() && f.url().endsWith('/'))
    || await (await page.locator('#preview-pane #inner').elementHandle()).contentFrame();
  await inner.waitForLoadState('load');
  // 同じ script でも、トップのフレームでなければ何も定義せず、ツリーも組まない。
  expect(await inner.evaluate(() => ({
    common: typeof window.MdCommon,
    keymap: typeof window.MdKeymap,
    tabs: typeof window.MdTabs,
    ready: document.documentElement.dataset.mdReady || null,
    rows: document.querySelectorAll('.tree-item').length,
  }))).toEqual({ common: 'undefined', keymap: 'undefined', tabs: 'undefined', ready: null, rows: 0 });
  // 外側のアプリは普通に動いている。
  expect(await page.evaluate(() => typeof window.MdCommon)).toBe('object');
});

// 数えない違反は無し（既定）。drawio は読み込んだ時点で数式（MathJax）の script を
// 足しにいくので、それを止められていないと、図のあるページごとに違反が 1 件出る。
test('draw.io の図は、数式の script を読みにいかずに描かれる', async ({ page, cspViolations }) => {
  const asked = [];
  page.on('request', (r) => { if (/startup\.js|math4|\/__lib\/none/.test(r.url())) asked.push(r.url()); });
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await openFolder(page);
  const xml = (math, label) => `<mxGraphModel math="${math}"><root><mxCell id="0"/><mxCell id="1" parent="0"/>` +
    `<mxCell id="2" value="${label}" style="rounded=1;html=1;" vertex="1" parent="1">` +
    '<mxGeometry x="20" y="20" width="160" height="50" as="geometry"/></mxCell></root></mxGraphModel>';
  // 本文に図を差し込み、アプリと同じ経路（MdCommon.runDrawio）で描かせる。数式を有効にした
  // 図も並べる（数式の部品を飛ばしても、図そのものは描かれること）。
  await page.evaluate((diagrams) => {
    const pane = document.querySelector('#preview-pane');
    diagrams.forEach(([id, xml]) => {
      const wrap = document.createElement('div');
      wrap.className = 'drawio-wrap';
      const node = document.createElement('div');
      node.className = 'mxgraph';
      node.id = id;
      node.setAttribute('data-mxgraph', JSON.stringify({ toolbar: 'lightbox', lightbox: true, xml }));
      wrap.appendChild(node);
      pane.appendChild(wrap);
    });
    window.MdCommon.runDrawio(pane);
  }, [['drawio-probe', xml(0, 'DRAWIO-PROBE')], ['drawio-math', xml(1, 'MATH-PROBE $x^2$')]]);
  await expect(page.locator('#drawio-probe svg')).toBeVisible();
  await expect(page.locator('#drawio-probe')).toContainText('DRAWIO-PROBE');
  await expect(page.locator('#drawio-math svg')).toBeVisible();
  await expect(page.locator('#drawio-math')).toContainText('MATH-PROBE');
  // 違反のイベントは読み込みの後から届くので、ひと呼吸おいてから数える。
  await page.waitForTimeout(300);
  expect(asked).toEqual([]);
  expect(errors).toEqual([]);
  expect(cspViolations).toEqual([]);
});

test('手元の動画は Range で部分ずつ配られる', async ({ page }) => {
  const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'md-range-'));
  const clip = path.join(dir, 'clip.mp4');
  // 再生はさせない（中身は動画ではない）。配る側が Range に 206 で答えるかだけを見る。
  fs.writeFileSync(clip, Buffer.alloc(4096, 7));
  try {
    await openFolder(page);
    const url = '/__abs' + encodeURI(clip);
    const got = await page.evaluate(async (u) => {
      const r = await fetch(u, { headers: { Range: 'bytes=100-199' } });
      const body = new Uint8Array(await r.arrayBuffer());
      return {
        status: r.status,
        range: r.headers.get('content-range'),
        type: r.headers.get('content-type'),
        csp: r.headers.get('content-security-policy'),
        len: body.length,
      };
    }, url);
    expect(got).toMatchObject({ status: 206, range: 'bytes 100-199/4096', type: 'video/mp4', len: 100 });
    expect(got.csp).toContain("script-src 'none'");

    // <video> が自分で投げる要求も Range 付きで、206 が返る。
    const resp = page.waitForResponse((r) => r.url().includes('/clip.mp4'));
    await page.evaluate((u) => {
      const v = document.createElement('video');
      v.preload = 'auto';
      v.src = u;
      document.querySelector('#preview-pane').appendChild(v);
    }, url);
    const r = await resp;
    if (r.request().headers()['range']) {
      expect(r.status()).toBe(206);
    } else {
      expect(r.status()).toBe(200);
      expect(r.headers()['accept-ranges']).toBe('bytes');
    }
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
