// md の本文の CSS と要素が、本文の枠の外（サイドバー・タブ・ページ全体）へ届かないこと
// （docguard.js）。そのうえで、本文の中では普通の書き方の `<style>` がそのまま効くこと。
//
// 本文は a.md の応答を差し替えて作る（フィクスチャを足すとツリーの並びを見ている spec に
// 響く）。差し替えるのはサーバの応答なので、本物の読み込み経路（fetch → 差し込み →
// hydrate）を通る。
const { test, expect, openFolder, treeItem } = require('./helpers');

const RED = 'rgb(255, 0, 0)';

/// a.md の本文を `html` にして開く。開く前のサイドバー・タブ帯の見た目を返す。
async function openMdBody(page, html) {
  await page.route((url) => /[?&]file=[^&]*a\.md$/.test(url.search), (route) => route.fulfill({
    contentType: 'text/html; charset=utf-8',
    body: '<div class="markdown-body"><h1>見出し A</h1>' + html + '<p id="tail">TAIL-TEXT</p></div>',
  }));
  await openFolder(page);
  const before = await appLook(page);
  await treeItem(page, 'a.md').click();
  await expect(page.locator('#preview-pane .markdown-body h1')).toContainText('見出し A');
  return before;
}

function appLook(page) {
  return page.evaluate(() => {
    const cs = (sel) => getComputedStyle(document.querySelector(sel));
    return {
      sidebarBg: cs('#sidebar-col').backgroundColor,
      sidebarDisplay: cs('#sidebar-col').display,
      tabbarDisplay: cs('#tabbar').display,
      tabbarBg: cs('#tabbar').backgroundColor,
      bodyBg: cs('body').backgroundColor,
      htmlBg: cs('html').backgroundColor,
      probeVar: cs('#sidebar-col').getPropertyValue('--md-probe'),
    };
  });
}

function color(page, sel) {
  return page.evaluate((s) => getComputedStyle(document.querySelector('#preview-pane ' + s)).color, sel);
}

test('文書の <style> の普通の書き方は、本文の中で効く', async ({ page }) => {
  await openMdBody(page, `
<style>
:root { --c: rgb(0, 0, 3); }
.a1 { color: rgb(0, 0, 1); }
.markdown-body .a2 { color: rgb(0, 0, 2); }
.a3 { color: var(--c); }
body .a4 { color: rgb(0, 0, 4); }
.markdown-body > p.a5 { color: rgb(0, 0, 5); }
html body .markdown-body .a6 { color: rgb(0, 0, 6); }
table td { color: rgb(0, 0, 7); }
@keyframes slide { from { margin-left: 9px; } to { margin-left: 9px; } }
.a8 { animation: slide 100s linear paused; }
</style>
<p class="a1">a1</p><p class="a2">a2</p><p class="a3">a3</p><p class="a4">a4</p>
<p class="a5">a5</p><p class="a6">a6</p>
<table><tbody><tr><td class="td">td</td></tr></tbody></table>
<p class="a8">a8</p>
`);
  expect(await color(page, '.a1')).toBe('rgb(0, 0, 1)');
  expect(await color(page, '.a2')).toBe('rgb(0, 0, 2)');
  expect(await color(page, '.a3')).toBe('rgb(0, 0, 3)');
  expect(await color(page, '.a4')).toBe('rgb(0, 0, 4)');
  expect(await color(page, '.a5')).toBe('rgb(0, 0, 5)');
  expect(await color(page, '.a6')).toBe('rgb(0, 0, 6)');
  expect(await color(page, '.td')).toBe('rgb(0, 0, 7)');
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('#preview-pane .a8')).marginLeft)).toBe('9px');
});

test('@media の条件は本文の中でそのまま効く', async ({ page }) => {
  await openMdBody(page, `
<style>
@media (width < 100000px) { .m1 { color: rgb(0, 1, 0); } }
@media (width > 100000px) { .m2 { color: rgb(0, 2, 0); } }
</style>
<p class="m1">m1</p><p class="m2">m2</p>
`);
  expect(await color(page, '.m1')).toBe('rgb(0, 1, 0)');
  expect(await color(page, '.m2')).not.toBe('rgb(0, 2, 0)');
});

test('文書の <style> はサイドバー・タブ帯・ページ全体を塗り替えられない', async ({ page }) => {
  const before = await openMdBody(page, `
<style>
#sidebar-col { background: ${RED} !important; display: none !important; }
#tabbar { background: ${RED} !important; display: none !important; }
body { background: ${RED} !important; }
html { background: ${RED} !important; }
:root { --md-probe: 1px; }
@property --md-probe { syntax: '<length>'; inherits: true; initial-value: 44px; }
@media (width > 0px) { #sidebar-col { background: ${RED} !important; } }
</style>
`);
  expect(await appLook(page)).toEqual(before);
  await expect(page.locator('#sidebar-col')).toBeVisible();
  await expect(page.locator('#tabbar')).toBeVisible();
});

test('外へ読みに行く CSS・<base>・<meta refresh>・popover はアプリに効かない', async ({ page }) => {
  const sheetHits = [];
  await page.route('**/__probe-*.css', (route) => {
    sheetHits.push(route.request().url());
    return route.fulfill({ contentType: 'text/css', body: `#sidebar-col { background: ${RED} !important; }` });
  });
  const before = await openMdBody(page, `
<link rel="stylesheet" href="/__probe-link.css">
<style>@import url("/__probe-import.css"); .after-import { color: rgb(0, 0, 9); }</style>
<base href="https://example.invalid/">
<meta http-equiv="refresh" content="0;url=about:blank">
<div id="pop" popover="manual">POPOVER</div>
<button id="pop-btn" popovertarget="pop" popovertargetaction="show">open</button>
<p class="after-import">x</p>
`);
  const url = page.url();
  await page.locator('#pop-btn').click();
  await page.waitForTimeout(500);

  expect(page.url()).toBe(url);
  expect(sheetHits).toEqual([]);
  expect(await appLook(page)).toEqual(before);
  expect(await page.evaluate(() => document.querySelector('#pop').matches(':popover-open'))).toBe(false);
  // @import を落としても、同じ <style> の残りは効く。
  expect(await color(page, '.after-import')).toBe('rgb(0, 0, 9)');
  // アプリの相対 URL の読み込みは、文書の <base> に引きずられない。
  await page.unroute((url) => /[?&]file=[^&]*a\.md$/.test(url.search));
  await treeItem(page, 'b.md').click();
  await expect(page.locator('#preview-pane .markdown-body h1')).toContainText('見出し B');
});

test('<style> と書いてあるだけのコメントや属性で、その後ろの本文が消えない', async ({ page }) => {
  await openMdBody(page, `
<!-- <style> の書き方の例 -->
<span title="<style>">属性に style</span>
<svg width="10" height="10"><title>&lt;style&gt;</title></svg>
<p>middle</p>
`);
  await expect(page.locator('#preview-pane #tail')).toHaveText('TAIL-TEXT');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('middle');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('属性に style');
});

test('閉じ忘れや余計な } のある <style> も、本文の外へは効かない', async ({ page }) => {
  const before = await openMdBody(page, `
<style>
} #sidebar-col { background: ${RED} !important; }
.ok { color: rgb(0, 0, 10); }
@scope (html) { #tabbar { background: ${RED} !important; } }
#tabbar { background: ${RED} !important
</style>
<p class="ok">ok</p>
`);
  expect(await appLook(page)).toEqual(before);
  expect(await color(page, '.ok')).toBe('rgb(0, 0, 10)');
});

test('document の名前を上書きする name は外れ、アプリの操作は続けて動く', async ({ page }) => {
  await openMdBody(page, `
<img name="getElementById" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" alt="">
<form name="querySelector"><input name="x"></form>
`);
  expect(await page.evaluate(() => typeof document.getElementById)).toBe('function');
  expect(await page.evaluate(() => typeof document.querySelector)).toBe('function');
  await treeItem(page, 'b.md').click();
  await expect(page.locator('#preview-pane .markdown-body h1')).toContainText('見出し B');
});

test('html / body / :root に書いた規則は、本文には字・色・背景・変数だけが効き、箱の指定は効かない', async ({ page }) => {
  await openMdBody(page, '<p class="plain">plain</p>');
  const base = await page.evaluate(() => {
    const cs = getComputedStyle(document.querySelector('#preview-pane .markdown-body'));
    return { margin: cs.marginBottom, padding: cs.paddingTop, maxWidth: cs.maxWidth, bg: cs.backgroundColor };
  });
  await page.unroute((url) => /[?&]file=[^&]*a\.md$/.test(url.search));
  await page.goto('about:blank');
  await openMdBody(page, `
<style>
body { margin: 77px; padding: 33px; max-width: 10px; background: ${RED}; color: rgb(0, 0, 11); font-size: 21px; }
:root { --v: rgb(0, 0, 12); }
html, .mx { margin-top: 44px; font-style: italic; }
.markdown-body { padding-left: 13px; }
.v { color: var(--v); }
</style>
<p class="v">v</p><p class="mx">mx</p>
`);
  const got = await page.evaluate(() => {
    const cs = getComputedStyle(document.querySelector('#preview-pane .markdown-body'));
    const mx = getComputedStyle(document.querySelector('#preview-pane .mx'));
    return {
      margin: cs.marginBottom, padding: cs.paddingTop, maxWidth: cs.maxWidth, bg: cs.backgroundColor,
      marginTop: cs.marginTop, color: cs.color, fontSize: cs.fontSize, fontStyle: cs.fontStyle,
      paddingLeft: cs.paddingLeft,
      v: getComputedStyle(document.querySelector('#preview-pane .v')).color,
      mxMarginTop: mx.marginTop,
    };
  });
  // 箱の指定（余白・幅）は本文の列に当たらない。
  expect(got.margin).toBe(base.margin);
  expect(got.padding).toBe(base.padding);
  expect(got.maxWidth).toBe(base.maxWidth);
  expect(got.marginTop).not.toBe('44px');
  // 受け継がれる性質・背景・変数は効く。
  expect(got.bg).toBe(RED);
  expect(got.color).toBe('rgb(0, 0, 11)');
  expect(got.fontSize).toBe('21px');
  expect(got.fontStyle).toBe('italic');
  expect(got.v).toBe('rgb(0, 0, 12)');
  // 他のセレクタと並べて書いた分は、そちらには全部効く。
  expect(got.mxMarginTop).toBe('44px');
  // .markdown-body と名指しした規則は、箱の指定も効く。
  expect(got.paddingLeft).toBe('13px');
});

test('@namespace を落としても、同じ <style> の残りの規則は効く', async ({ page }) => {
  await openMdBody(page, `
<style>
@namespace svg url(http://www.w3.org/2000/svg);
.ns { color: rgb(0, 0, 15); }
@media (width > 0px) { .ns2 { color: rgb(0, 0, 16); } }
</style>
<p class="ns">ns</p><p class="ns2">ns2</p>
`);
  expect(await color(page, '.ns')).toBe('rgb(0, 0, 15)');
  expect(await color(page, '.ns2')).toBe('rgb(0, 0, 16)');
});

test('body に背景と字の色を組で書いた文書は、本文に両方が効いて読める', async ({ page }) => {
  const before = await openMdBody(page, `
<style>
html { background-color: rgb(255, 255, 254); }
body { background: #fff; color: #222; }
</style>
<p class="t">t</p>
`);
  const got = await page.evaluate(() => {
    const cs = getComputedStyle(document.querySelector('#preview-pane .markdown-body'));
    return { bg: cs.backgroundColor, color: getComputedStyle(document.querySelector('#preview-pane .t')).color };
  });
  // 後に書いた body の分が html の分に勝つ（どちらも本文の根に当たる同じ詳細度の規則）。
  expect(got.bg).toBe('rgb(255, 255, 255)');
  expect(got.color).toBe('rgb(34, 34, 34)');
  // ページ全体の背景は変わらない。
  expect(await appLook(page)).toEqual(before);
});

test('本文に書いた偽のソースビューは行の包み直しに通らず、<style> の中から要素を作り出せない', async ({ page }) => {
  const sheetHits = [];
  await page.route('**/__probe-*.css', (route) => {
    sheetHits.push(route.request().url());
    return route.fulfill({ contentType: 'text/css', body: `#sidebar-col { background: ${RED} !important; }` });
  });
  // CSS の文字列の中の `\3c /style>` は、直列化すると素の `</style>` になる。
  const breakout = '\\3c /style><link rel=stylesheet href=/__probe-src.css>' +
    '<meta http-equiv=refresh content=0;url=about:blank>' +
    '<div popover=manual id=srcpop>POP</div>' +
    `<style>#sidebar-col { background: ${RED} !important; }`;
  const before = await openMdBody(page, `
<div class="source-view"><div class="source-main"><pre><code data-highlighted="yes"><style>.fake::after { content: "${breakout}"; }</style>one
two
</code></pre></div></div>
`);
  const url = page.url();
  await page.waitForTimeout(500);
  expect(page.url()).toBe(url);
  expect(await appLook(page)).toEqual(before);
  expect(sheetHits).toEqual([]);
  expect(await page.evaluate(() => document.body.querySelectorAll('link, meta, [popover]').length)).toBe(0);
  // 偽物の <code> は 1 行 1 要素に包まれない。
  await expect(page.locator('#preview-pane .md-src-row')).toHaveCount(0);
});
