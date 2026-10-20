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
