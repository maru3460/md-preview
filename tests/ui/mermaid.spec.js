// mermaid の securityLevel（common.js の runMermaid）。
//
// 見ているのは「図の中から文書の JS が動かないこと」と「それを図の側の設定で
// 緩め直せないこと」。mermaid は `%%{init}%%` と frontmatter の `config:` で
// 図ごとに設定を上書きできるので、ここが loose に戻せると strict にした意味が無い。
const { test, expect } = require('@playwright/test');
const { openFolder } = require('./helpers');

// 図を本文に差し込み、アプリと同じ経路（MdCommon.runMermaid）で描かせる。
// フィクスチャを足すとツリーの並びを見ている spec に響くので、その場で作る。
async function renderMermaid(page, src) {
  await page.evaluate((src) => {
    const pre = document.createElement('pre');
    pre.className = 'mermaid';
    pre.id = 'mermaid-probe';
    pre.textContent = src;
    document.querySelector('#preview-pane').appendChild(pre);
    window.MdCommon.runMermaid(pre.parentNode);
  }, src);
  await expect(page.locator('#mermaid-probe svg')).toBeVisible();
}

// ラベルの HTML（`onerror` など）は loose でも DOMPurify が落とすので、ここでは見ない。
// 見るのは、loose で通ってしまうクリックの呼び出しと `javascript:` のリンクの 2 つ。
const PAYLOAD = [
  'flowchart LR',
  '  A --> B',
  '  click A call __mdCall()',
  '  click B href "javascript:window.__mdPwn=3"',
].join('\n');

const OVERRIDES = {
  '%%{init}%%': '%%{init: {"securityLevel": "loose"}}%%\n' + PAYLOAD,
  'frontmatter の config:': '---\nconfig:\n  securityLevel: loose\n---\n' + PAYLOAD,
};

for (const [how, src] of Object.entries(OVERRIDES)) {
  test(`${how} で securityLevel を loose に戻せず、クリックの呼び出しも javascript: のリンクも残らない`, async ({ page }) => {
    await openFolder(page);
    await page.evaluate(() => { window.__mdCall = () => { window.__mdPwn = 2; }; });
    await renderMermaid(page, src);

    await page.locator('#mermaid-probe .node').first().click();
    expect(await page.evaluate(() => window.__mdPwn)).toBeUndefined();
    // mermaid はリンクを xlink:href に書くので、名前空間を問わずに探す。
    await expect(page.locator('#mermaid-probe svg a[*|href^="javascript:"]')).toHaveCount(0);
  });
}

test('strict でもラベルの <br> と <b> は残る', async ({ page }) => {
  await openFolder(page);
  await renderMermaid(page, 'flowchart LR\n  A["一行目<br>二行目 <b>太字</b>"] --> B');
  const label = page.locator('#mermaid-probe .node').first();
  await expect(label.locator('b')).toHaveText('太字');
  await expect(label.locator('br')).toHaveCount(1);
});

// 図の <style> は本文の CSS の組み直し（docguard.js）より後に描かれる。図の側の設定から
// CSS の文字列を流し込めると、@font-face がページ全体に効く。@import は mermaid の CSS の
// 組み立てで落ちるが、念のため読みに行かないことも見る。
const CSS_INJECTIONS = {
  themeCSS: '%%{init: {"themeCSS": "@import url(/__probe-mermaid.css); @font-face { font-family: md-probe-font; src: local(Courier); } .node rect { fill: rgb(255, 0, 0) !important; }"}}%%',
  // 値の位置に入るが、括弧の数が合っていれば入れ子の at-rule を書ける（描くときに外へ出される）。
  fontFamily: '%%{init: {"fontFamily": "x; @font-face { font-family: md-probe-font; src: local(Courier); }"}}%%',
};

for (const [key, init] of Object.entries(CSS_INJECTIONS)) {
  test(`%%{init}%% の ${key} では、図の外（アプリの UI の字形や CSS）を変えられない`, async ({ page }) => {
    const hits = [];
    await page.route('**/__probe-mermaid.css', (route) => {
      hits.push(route.request().url());
      return route.fulfill({ contentType: 'text/css', body: '#sidebar-col { background: rgb(255, 0, 0) !important; }' });
    });
    await openFolder(page);
    const before = await page.evaluate(() => getComputedStyle(document.getElementById('sidebar-col')).backgroundColor);
    await renderMermaid(page, init + '\nflowchart LR\n  A --> B');
    await page.waitForTimeout(300);
    expect(hits).toEqual([]);
    expect(await page.evaluate(() => getComputedStyle(document.getElementById('sidebar-col')).backgroundColor)).toBe(before);
    // @font-face が document に入っていない（入るとアプリが使うファミリ名も差し替えられる）。
    expect(await page.evaluate(() => Array.from(document.fonts).some((f) => /md-probe-font/.test(f.family)))).toBe(false);
    // 図の中の色付けも効いていない（themeCSS そのものが捨てられている）。
    expect(await page.locator('#mermaid-probe .node rect').first().evaluate((e) => getComputedStyle(e).fill)).not.toBe('rgb(255, 0, 0)');
  });
}

test('themeVariables の色は、これまでどおり図ごとに変えられる', async ({ page }) => {
  await openFolder(page);
  await renderMermaid(page, '%%{init: {"theme": "base", "themeVariables": {"primaryColor": "#00ff00"}}}%%\nflowchart LR\n  A --> B');
  expect(await page.locator('#mermaid-probe .node rect').first().evaluate((e) => getComputedStyle(e).fill)).toBe('rgb(0, 255, 0)');
});

// 図の <style> は classDef / style の文字列から組まれる。mermaid は規則の頭に図の id を
// 付けるが、`&`（図の id に置き換わる）を使えば `body:has(&) #sidebar-col` のように
// 図の外を指す規則が書ける。描いた図の <style> は図の SVG の中にだけ効くこと。
const ESCAPES = ['body:has(&) #sidebar-col', 'body:has(&) #tabbar', 'body:has(&)']
  .map((sel) => `fill:#ff0000} ${sel} {background:rgb(255 0 0)`);
const STYLE_DIAGRAMS = {
  'block-beta': [
    'block-beta', '  columns 1', '  a["A"]', '  classDef good fill:#00ff00', '  class a good',
    ...ESCAPES.map((e, i) => `  classDef evil${i} ${e}`), '  b["B"]', '  class b evil0',
  ],
  stateDiagram: [
    'stateDiagram-v2', '  [*] --> S1', '  S1 --> S2', '  classDef good fill:#00ff00', '  class S1 good',
    ...ESCAPES.map((e, i) => `  classDef evil${i} ${e}`), '  class S2 evil0',
  ],
  classDiagram: [
    'classDiagram', '  class Foo', '  class Bar', '  style Foo fill:#00ff00',
    ...ESCAPES.map((e) => `  style Bar ${e}`),
  ],
  'treemap-beta': [
    'treemap-beta', '"Root"', '    "A": 10:::good', '    "B": 20:::evil0',
    'classDef good fill:#00ff00', ...ESCAPES.map((e, i) => `classDef evil${i} ${e}`),
  ],
};

for (const [type, lines] of Object.entries(STYLE_DIAGRAMS)) {
  test(`${type} の classDef / style から、図の外（サイドバー・タブ帯・body）は塗れず、図の中の色付けは効く`, async ({ page }) => {
    await openFolder(page);
    const look = () => page.evaluate(() => ['#sidebar-col', '#tabbar', 'body']
      .map((s) => getComputedStyle(document.querySelector(s)).backgroundColor));
    const before = await look();
    await renderMermaid(page, lines.join('\n'));
    // 図の外へ向けた規則が、組み直されずに残っていたら効いてしまう形で書かれていること。
    await expect.poll(() => page.evaluate(() => document.querySelector('#mermaid-probe svg style').textContent)).toContain('#sidebar-col');
    expect(await look()).toEqual(before);
    expect(await page.evaluate(() => Array.from(document.querySelectorAll('#mermaid-probe svg *'))
      .some((e) => getComputedStyle(e).fill === 'rgb(0, 255, 0)'))).toBe(true);
  });
}
