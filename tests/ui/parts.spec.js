// アプリが配る部品（parts.js）: コピーボタンと図の拡大。
//
// 文書の JS を止めても、目印の属性を書いた文書ではこの 2 つが動くこと。文書の中身
// （CSS・名前のすり替え・偽の目印）に、部品の動きと合図を乗っ取られないこと。
// 拡大は本文の枠の中に留まり、アプリの重ねものより下に出ること。
//
// クリップボードの中身は autocopy.spec.js と同じく、navigator.clipboard を失敗させて
// IPC スタブ（window.__mdIpc）へ落として見る。
const { test, expect, openFolder, treeItem } = require('./helpers');

async function stubClipboard(page) {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      get: () => ({ writeText: () => Promise.reject(new Error('stubbed')) })
    });
  });
}

function copies(page) {
  return page.evaluate(() =>
    window.__mdIpc.filter((m) => m.indexOf('copy:') === 0).map((m) => m.slice(5)));
}

/// 写しは非同期（クリップボードの失敗を待ってから IPC へ落ちる）なので、何も写らない
/// ことは少し待ってから確かめる。
async function expectNoCopies(page) {
  await page.waitForTimeout(400);
  expect(await copies(page)).toEqual([]);
}

const probe = (page, name) => page.evaluate((n) => MdParts.__test[n](), name);
const isZoomed = (page) => probe(page, 'isZoomed');
const signalText = (page) => probe(page, 'signalText');

/// a.md を開き、本文の末尾に html を差し込む（フィクスチャを足すとツリーの並びを見ている
/// spec に響くので、その場で作る）。
async function openMdWith(page, html) {
  await stubClipboard(page);
  await openFolder(page);
  await treeItem(page, 'a.md').click();
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.evaluate((html) => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('beforeend', html);
  }, html);
}

/// page.html を開き、iframe の body の先頭に html を差し込む。
async function openHtmlWith(page, html) {
  await stubClipboard(page);
  await openFolder(page);
  await treeItem(page, 'page.html').click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate((html) => {
    document.querySelector('iframe.html-frame').contentDocument.body.insertAdjacentHTML('afterbegin', html);
  }, html);
  return frame;
}

/// 重ねものの背景（図の外側）を押す。
async function clickBackdrop(page) {
  const r = await page.locator('#preview-pane > .markdown-body > .md-zoom-ov').boundingBox();
  await page.mouse.click(r.x + 4, r.y + 4);
}

const rectOf = (loc) => loc.evaluate((e) => {
  const r = e.getBoundingClientRect();
  return { w: r.width, h: r.height };
});

/// 実寸 200×100 の PNG（ラスター画像）。
async function pngDataUrl(page) {
  return page.evaluate(() => {
    const c = document.createElement('canvas');
    c.width = 200; c.height = 100;
    const g = c.getContext('2d');
    g.fillStyle = 'teal'; g.fillRect(0, 0, 200, 100);
    return c.toDataURL('image/png');
  });
}

// ── コピー ─────────────────────────────────────────────────────

test('md: data-md-copy は id の要素の文字を写し、写した中身を合図に出す', async ({ page }) => {
  await openMdWith(page, '<p><button data-md-copy="copy-x">写す</button> <code id="copy-x">npm run build</code></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => copies(page)).toEqual(['npm run build']);
  expect(await probe(page, 'signalShown')).toBe(true);
  expect(await signalText(page)).toContain('npm run build');
});

test('ページが右から左の向きでも、合図は左から右で並び、写した行は行ごとに向きを決める', async ({ page }) => {
  await openMdWith(page, '<p><button data-md-copy="copy-r">写す</button> <code id="copy-r">abc שלום</code></p>');
  await page.evaluate(() => {
    document.documentElement.setAttribute('dir', 'rtl');
    document.body.style.direction = 'rtl';
  });
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  expect(await probe(page, 'signalBidi')).toEqual({ box: 'ltr isolate', body: 'plaintext' });
});

test('html: id の形と、値なしの形（すぐ前の pre）の両方が写す', async ({ page }) => {
  const frame = await openHtmlWith(page,
    '<code id="copy-y">id の中身</code><button id="b1" data-md-copy="copy-y">写す</button>' +
    '<pre>pre の中身\n2 行目</pre><button id="b2" data-md-copy>写す</button>');
  await frame.locator('#b1').click();
  await frame.locator('#b2').click();
  await expect.poll(() => copies(page)).toEqual(['id の中身', 'pre の中身\n2 行目']);
});

// イングランドの旗。黒旗（U+1F3F4）に続くタグ文字（g b e n g）と終わりの印（U+E007F）。
const ENGLAND = '\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}';

test('写す文字から見えない制御の文字・タグ文字・アプリの UI を落とし、ゼロ幅接合子と旗の絵文字は残す', async ({ page }) => {
  await openMdWith(page,
    '<div id="copy-z">ab‮cd​ef<span class="md-cmt-badge">💬3</span>' +
    '<button class="copy-btn">Copy</button><div class="code-filename">a.rs</div>gh' +
    // 絵文字の並び（ZWJ）・ペルシア語の語中の切れ目（ZWNJ）・見えないタグ文字・旗の絵文字。
    '|👩‍💻|می‌خواهم|t\u{E0041}\u{E0042}\u{E007F}u|' + ENGLAND + '|\u{E0067}v</div>' +
    '<p><button data-md-copy="copy-z">写す</button></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => copies(page)).toEqual(['abcdefgh|👩‍💻|می‌خواهم|tu|' + ENGLAND + '|v']);
});

test('写しには文書の CSS が当たり、CSS で隠した文字は写さない', async ({ page }) => {
  await openMdWith(page,
    // 祖先を辿るセレクタ（.note > ...）でも、写す要素自身に当たるセレクタでも隠す。
    '<style>.note > #copy-h .x { display: none; } #copy-h .y { display: none; }</style>' +
    '<div class="note"><span id="copy-h">見える<span class="x">祖先で隠した</span>' +
    '<span class="y">自分で隠した</span>文字</span></div>' +
    '<p><button data-md-copy="copy-h">写す</button></p>');
  // 本文の <style> は本文の枠の中にだけ効く形に組み直される（実際の読み込みと同じに）。
  await page.evaluate(() => MdDocGuard.guard(document.querySelector('#preview-pane .markdown-body')));
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => copies(page)).toEqual(['見える文字']);
  // 写しは読み終えたら外す。
  expect(await page.evaluate(() => document.querySelectorAll('#copy-h').length)).toBe(1);
});

test('html: 写しには iframe の文書の CSS が当たる', async ({ page }) => {
  const frame = await openHtmlWith(page,
    '<style>.box > #copy-k .x { display: none; }</style>' +
    '<div class="box"><span id="copy-k">見える<span class="x">隠した</span>文字</span></div>' +
    '<button id="bk" data-md-copy="copy-k">写す</button>');
  await frame.locator('#bk').click();
  await expect.poll(() => copies(page)).toEqual(['見える文字']);
});

test('写しに画像などの埋め込みを持ち込まない', async ({ page }) => {
  await openMdWith(page,
    '<div id="copy-m">前<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" alt="画像">' +
    '<video></video>後</div><p><button data-md-copy="copy-m">写す</button></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => copies(page)).toEqual(['前後']);
});

test('見つからない id は、文書のボタンを書き換えず、id 入りの合図を出す', async ({ page }) => {
  await openMdWith(page, '<p><button data-md-copy="copy-none">写す</button></p>');
  const btn = page.locator('#preview-pane button[data-md-copy]');
  await btn.click();
  await expect(btn).toHaveText('写す');
  expect(await signalText(page)).toContain('見つかりません');
  expect(await signalText(page)).toContain('copy-none');
  await expectNoCopies(page);
});

test('copy- で始まらない値は、copy- で始めるよう値入りの合図を出して写さない', async ({ page }) => {
  await openMdWith(page, '<p id="plain">ふつうの段落</p><p><button data-md-copy="plain">写す</button></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => signalText(page)).toContain('copy- で始めて');
  expect(await signalText(page)).toContain('plain');
  await expectNoCopies(page);
});

test('合図は複数行の中身を真ん中の行まで全部見せ、上限を超えたら真ん中を省く', async ({ page }) => {
  await openMdWith(page,
    '<pre id="copy-l">1 行目\n真ん中の行\n最後の行</pre><p><button id="short" data-md-copy="copy-l">写す</button></p>' +
    '<pre id="copy-big">' + 'あ'.repeat(1000) + 'まんなか' + 'い'.repeat(1000) + '</pre>' +
    '<p><button id="big" data-md-copy="copy-big">写す</button></p>');
  await page.locator('#short').click();
  await expect.poll(() => signalText(page)).toContain('真ん中の行');
  const t = await signalText(page);
  expect(t).toContain('3 行');
  expect(t).toContain('1 行目\n真ん中の行\n最後の行');

  await page.locator('#big').click();
  await expect.poll(() => signalText(page)).toContain('省略');
  const big = await signalText(page);
  expect(big).toContain('あ'.repeat(10));
  expect(big).toContain('い'.repeat(10));
  expect(big).not.toContain('まんなか');
  // 写したもの自体は省かない。
  await expect.poll(async () => (await copies(page)).pop()).toBe('あ'.repeat(1000) + 'まんなか' + 'い'.repeat(1000));
});

test('目印の無いもの・本文の外の目印には何も起きない', async ({ page }) => {
  await openMdWith(page,
    '<p id="plain">ふつうの段落</p><p><button id="nomark">目印なし</button></p><figure id="nofig">図</figure>');
  // 本文の外（アプリの UI 側）に置いた目印。
  await page.evaluate(() => {
    const b = document.createElement('button');
    b.id = 'outside'; b.setAttribute('data-md-copy', 'copy-x'); b.textContent = '外';
    document.getElementById('tabbar').appendChild(b);
  });
  for (const sel of ['#nomark', '#nofig', '#outside']) await page.locator(sel).click();
  await expectNoCopies(page);
  expect(await isZoomed(page)).toBe(false);
  expect(await signalText(page)).toBe('');
});

test('合図は文書の CSS で消せない', async ({ page }) => {
  await openMdWith(page,
    // 合図の置き場所（html 直下・body 直下・トップレイヤー）を狙えるものを全部隠しにいく。
    '<style>html > *:not(body), body > div:not(.folder-layout), [popover], :popover-open, [role=status] {' +
    ' display:none!important; opacity:0!important; visibility:hidden!important; }</style>' +
    '<p><button data-md-copy="copy-s">写す</button> <code id="copy-s">見える？</code></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
});

test('合図は長い中身を行の境目で省き、残す行は途中で切らない', async ({ page }) => {
  const rows = Array.from({ length: 100 }, (_, i) => '行 ' + (i + 1));
  const wide = Array.from({ length: 30 }, (_, i) => String(i).padStart(2, '0') + 'x'.repeat(98));
  await openMdWith(page,
    '<pre id="copy-rows">' + rows.join('\n') + '</pre><p><button id="r" data-md-copy="copy-rows">写す</button></p>' +
    '<pre id="copy-wide">' + wide.join('\n') + '</pre><p><button id="w" data-md-copy="copy-wide">写す</button></p>');

  await page.locator('#r').click();
  await expect.poll(() => signalText(page)).toContain('行を省略');
  const t = await signalText(page);
  expect(t).toContain('100 行');
  expect(t).toContain('行 1\n行 2\n');
  expect(t).toContain('行 15\n…（70 行を省略）…\n行 86\n');
  expect(t).toContain('行 100');
  expect(t).not.toContain('行 50');

  // 1 行が長いときは、文字数の上限に収まる行数だけ残す。残した行は 1 行まるごと。
  await page.locator('#w').click();
  await expect.poll(() => signalText(page)).toContain('00' + 'x'.repeat(98));
  const body = (await signalText(page)).replace(/^コピーしました（[^）]*）/, '');
  const shown = body.split('\n');
  const marker = shown.findIndex((l) => /^…（\d+ 行を省略）…$/.test(l));
  expect(marker).toBeGreaterThan(0);
  shown.forEach((l, i) => { if (i !== marker) expect(wide).toContain(l); });
  expect(shown[shown.length - 1]).toBe(wide[29]);
});

test('合図は乗せている間は消えず、外すと消え、押すと閉じる。Esc でも閉じる', async ({ page }) => {
  await openMdWith(page, '<p><button data-md-copy="copy-v">写す</button> <code id="copy-v">読みたい中身</code></p>');
  const btn = page.locator('#preview-pane button[data-md-copy]');
  await btn.click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  const r = await probe(page, 'signalRect');
  await page.mouse.move(r.x + r.w / 2, r.y + r.h / 2);
  // 何もしなければ消える時間（3200ms と少し）を過ぎても、乗せている間は残る。
  await page.waitForTimeout(4000);
  expect(await probe(page, 'signalShown')).toBe(true);
  await page.mouse.move(r.x + r.w / 2, r.y + r.h + 200);
  await expect.poll(() => probe(page, 'signalShown'), { timeout: 4000 }).toBe(false);

  await btn.click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  const r2 = await probe(page, 'signalRect');
  await page.mouse.click(r2.x + r2.w / 2, r2.y + r2.h / 2);
  expect(await probe(page, 'signalShown')).toBe(false);

  await btn.click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  await page.keyboard.press('Escape');
  expect(await probe(page, 'signalShown')).toBe(false);
});

test('合図は本文の枠の真ん中に出る', async ({ page }) => {
  await openMdWith(page, '<p><button data-md-copy="copy-c2">写す</button> <code id="copy-c2">真ん中</code></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  const r = await probe(page, 'signalRect');
  const pane = await page.locator('#preview-pane').boundingBox();
  expect(Math.abs(r.x + r.w / 2 - (pane.x + pane.width / 2))).toBeLessThan(2);
});

test('コードブロックの Copy も、写した中身を合図に出す', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await treeItem(page, 'zz-code.md').click();
  await expect(page.locator('#preview-pane .copy-btn').first()).toBeAttached();
  await page.locator('#preview-pane .copy-btn').first().click({ force: true });
  // コードブロックの Copy は、フェンスの中身を末尾の改行ごと写す（これまでどおり）。
  await expect.poll(() => copies(page)).toEqual(['cargo install --path .\n']);
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  expect(await signalText(page)).toContain('cargo install --path .');
  // 受け口は 1 回しか取れないので、文書の JS から偽の合図を出す口にはならない。
  expect(await page.evaluate(() => MdCommon.claimCopySignal(() => {}))).toBe(false);
});

test('コードブロックの Copy も、見えない制御の文字を落とす', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await treeItem(page, 'zz-code.md').click();
  await expect(page.locator('#preview-pane .copy-btn').first()).toBeAttached();
  await page.evaluate(() => {
    const code = document.querySelector('#preview-pane pre > code');
    code.textContent = 'x‮y';
  });
  await page.locator('#preview-pane .copy-btn').first().click({ force: true });
  await expect.poll(() => copies(page)).toEqual(['xy']);
});

// ── 図の拡大 ───────────────────────────────────────────────────

const FIG = '<figure data-md-zoom id="fig"><svg width="80" height="40"><rect width="80" height="40" fill="teal"/></svg>' +
  '<figcaption>図の説明</figcaption></figure><p id="after">図の下の段落</p>';

test('md: data-md-zoom の図は広がり、Esc と背景のクリックで閉じて元の場所へ戻る', async ({ page }) => {
  await openMdWith(page, FIG);
  const fig = page.locator('#fig');
  const styleBefore = await fig.evaluate((e) => e.getAttribute('style'));
  await fig.click();
  expect(await isZoomed(page)).toBe(true);
  // 拡大中は素キーを止める（j でスクロールしない）。
  expect(await page.evaluate(() => MdCommon.isOverlayOpen())).toBe(true);
  await page.keyboard.press('Escape');
  expect(await isZoomed(page)).toBe(false);
  // 元の場所（本文の中）へ戻り、style 属性も元のまま。
  expect(await page.evaluate(() => !!document.querySelector('#preview-pane .markdown-body #fig'))).toBe(true);
  expect(await fig.evaluate((e) => e.getAttribute('style'))).toBe(styleBefore);

  await fig.click();
  await clickBackdrop(page);
  expect(await isZoomed(page)).toBe(false);
  expect(await page.evaluate(() => !!document.querySelector('#preview-pane .markdown-body #fig'))).toBe(true);
});

test('拡大中にコードが本文の枠をスクロールさせたら、拡大は閉じて素キーが効く', async ({ page }) => {
  await openMdWith(page, FIG + '<div style="height: 4000px"></div>');
  await page.locator('#fig').click();
  expect(await isZoomed(page)).toBe(true);
  // ⌘F の飛び先や目次のジャンプと同じく、scrollTop を直に動かす。
  await page.evaluate(() => { document.getElementById('preview-pane').scrollTop += 600; });
  await expect.poll(() => isZoomed(page)).toBe(false);
  expect(await page.evaluate(() => MdCommon.isOverlayOpen())).toBe(false);
  expect(await page.evaluate(() => !!document.querySelector('#preview-pane .markdown-body #fig'))).toBe(true);
  const top = await page.evaluate(() => document.getElementById('preview-pane').scrollTop);
  await page.keyboard.press('j');
  await expect.poll(() => page.evaluate(() => document.getElementById('preview-pane').scrollTop)).toBeGreaterThan(top);
});

test('1:1 で拡大した図を重ねものの中でスクロールしても、拡大は閉じない', async ({ page }) => {
  await openMdWith(page,
    '<figure data-md-zoom id="big"><svg width="4000" height="4000"><rect width="4000" height="4000" fill="teal"/></svg></figure>' +
    '<div style="height: 4000px"></div>');
  await page.locator('#big').click();
  expect(await isZoomed(page)).toBe(true);
  await page.locator('#big').click();
  expect(await page.locator('.md-zoom-ov').evaluate((e) => e.classList.contains('md-zoom-actual'))).toBe(true);
  // 重ねもの自身が動いたことも確かめる（動かなければ、見ているものが無い）。
  expect(await page.evaluate(() => { const o = document.querySelector('.md-zoom-ov'); o.scrollTop = 500; return o.scrollTop; })).toBeGreaterThan(0);
  await page.waitForTimeout(200);
  expect(await isZoomed(page)).toBe(true);
});

test('拡大している間、本文の並びはずれない（置き札が図と同じ並び方と大きさを取る）', async ({ page }) => {
  await openMdWith(page, FIG +
    '<p>行の中の<img id="inl" data-md-zoom width="30" height="15" src="data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%2230%22 height=%2215%22/%3E">画像</p>' +
    '<p id="after2">その下</p>');
  const top = (sel) => page.locator(sel).evaluate((e) => e.getBoundingClientRect().top);
  const before = [await top('#after'), await top('#after2')];
  await page.locator('#fig').click();
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-md-zoom-placeholder]')).display)).toBe('block');
  expect([await top('#after'), await top('#after2')]).toEqual(before);
  await page.keyboard.press('Escape');
  await page.locator('#inl').click();
  expect(await page.evaluate(() => getComputedStyle(document.querySelector('[data-md-zoom-placeholder]')).display)).toBe('inline-block');
  expect([await top('#after'), await top('#after2')]).toEqual(before);
});

test('画面に合わせると figure の中の svg が大きくなり、押すと 1:1 に戻る（閉じない）', async ({ page }) => {
  // mermaid と同じ形（width="100%"・インラインの max-width・viewBox）と、viewBox の無い svg。
  await openMdWith(page,
    '<figure data-md-zoom id="m"><svg id="msvg" width="100%" style="max-width: 80px;" viewBox="0 0 80 40">' +
    '<rect width="80" height="40" fill="teal"/></svg></figure>' + FIG);
  const svg = page.locator('#msvg');
  const small = await rectOf(svg);
  await page.locator('#m').click();
  const fit = await rectOf(svg);
  expect(fit.w).toBeGreaterThan(small.w * 2);
  expect(Math.abs(fit.w / fit.h - 2)).toBeLessThan(0.05);
  // 枠の外へはみ出さない。
  const ov = await rectOf(page.locator('#preview-pane > .markdown-body > .md-zoom-ov'));
  expect(fit.w).toBeLessThanOrEqual(ov.w);
  expect(fit.h).toBeLessThanOrEqual(ov.h);
  // 図の箱そのものは枠いっぱいに伸ばさない。
  expect((await rectOf(page.locator('#m'))).h).toBeLessThan(ov.h);

  await svg.click();
  expect(await isZoomed(page)).toBe(true);
  expect(await rectOf(svg)).toEqual({ w: 80, h: 40 });
  await page.keyboard.press('Escape');
  expect(await svg.evaluate((e) => e.getAttribute('style'))).toBe('max-width: 80px;');

  // viewBox の無い svg も、中身ごと大きくなる。閉じたら viewBox は外す。
  const inner = page.locator('#fig svg');
  await page.locator('#fig').click();
  expect((await rectOf(inner)).w).toBeGreaterThan(160);
  expect(await inner.evaluate((e) => e.querySelector('rect').getBoundingClientRect().width)).toBeGreaterThan(160);
  await page.keyboard.press('Escape');
  expect(await inner.evaluate((e) => e.hasAttribute('viewBox'))).toBe(false);
});

test('画面に合わせても本文の中より小さくしない（縦長の図は幅に合わせて縦にスクロール）', async ({ page }) => {
  // mermaid と同じ形で、縦にとても長い図。両辺を収めると本文の中より小さくなる。
  await openMdWith(page,
    '<figure data-md-zoom id="tall"><svg id="tsvg" width="100%" style="max-width: 300px;" viewBox="0 0 300 3000">' +
    '<rect width="300" height="3000" fill="teal"/></svg></figure>');
  const svg = page.locator('#tsvg');
  const inline = await rectOf(svg);
  expect(inline.w).toBe(300);
  await page.locator('#tall').click();
  expect(await isZoomed(page)).toBe(true);
  const fit = await rectOf(svg);
  expect(fit.w).toBeGreaterThanOrEqual(inline.w - 0.5);
  expect(Math.abs(fit.h / fit.w - 10)).toBeLessThan(0.05);
  const ov = page.locator('#preview-pane > .markdown-body > .md-zoom-ov');
  expect(fit.w).toBeLessThanOrEqual((await rectOf(ov)).w);
  // 縦にはスクロールして見る。
  expect(await ov.evaluate((e) => e.scrollHeight > e.clientHeight)).toBe(true);
});

test('本文の中で実寸以上に出ているラスター画像は、広げず拡大のカーソルも出さない', async ({ page }) => {
  await openMdWith(page, '<p>x</p>');
  const src = await pngDataUrl(page);
  await page.evaluate((src) => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('beforeend',
      '<p><img id="nat" data-md-zoom src="' + src + '"></p>' +
      '<p><img id="big" data-md-zoom width="300" height="150" src="' + src + '"></p>' +
      '<p><img id="sm" data-md-zoom width="60" height="30" src="' + src + '"></p>');
  }, src);
  for (const id of ['#nat', '#big', '#sm']) {
    await expect.poll(() => page.locator(id).evaluate((e) => e.complete && e.naturalWidth)).toBe(200);
  }
  const cursor = (sel) => page.locator(sel).evaluate((e) => getComputedStyle(e).cursor);
  for (const id of ['#nat', '#big']) {
    await page.locator(id).hover();
    expect(await cursor(id)).not.toBe('zoom-in');
    await page.locator(id).click();
    expect(await isZoomed(page)).toBe(false);
  }
  // 実寸より小さく出ている画像は、これまでどおり広げられる。
  await page.locator('#sm').hover();
  expect(await cursor('#sm')).toBe('zoom-in');
  await page.locator('#sm').click();
  expect(await isZoomed(page)).toBe(true);
});

test('拡大中は右上に操作の手がかりを出し、モードに合わせて文言が変わる。選択とコピーには入らない', async ({ page }) => {
  await openMdWith(page, FIG);
  await page.locator('#fig').click();
  const hint = page.locator('#preview-pane > .markdown-body > .md-zoom-hint');
  await expect(hint).toBeVisible();
  expect(await probe(page, 'hintText')).toBe('Esc で閉じる · クリックで 1:1');
  // 重ねものの右上に出る。
  const h = await hint.boundingBox();
  const ov = await page.locator('#preview-pane > .markdown-body > .md-zoom-ov').boundingBox();
  expect(h.y - ov.y).toBeLessThan(20);
  expect(ov.x + ov.width - (h.x + h.width)).toBeLessThan(20);
  // 文字は content で出すので、選択の文字にもクリップボードにも入らない。
  expect(await hint.evaluate((e) => e.textContent)).toBe('');
  expect(await page.evaluate(() => {
    getSelection().selectAllChildren(document.getElementById('preview-pane'));
    return String(getSelection());
  })).not.toContain('Esc');
  await page.evaluate(() => getSelection().removeAllRanges());
  // 裏をホイールで動かさない。
  expect(await page.locator('#preview-pane > .markdown-body > .md-zoom-ov').evaluate((e) => getComputedStyle(e).overscrollBehaviorY)).toBe('contain');

  await page.locator('#fig svg').click();
  expect(await probe(page, 'hintText')).toBe('Esc で閉じる · クリックで全体');
  await page.locator('#fig svg').click();
  expect(await probe(page, 'hintText')).toBe('Esc で閉じる · クリックで 1:1');
  await page.keyboard.press('Escape');
  await expect(page.locator('.md-zoom-hint')).toHaveCount(0);
});

test('ラスター画像は実寸より大きくせず、1:1 は width 属性より大きい実寸で出す', async ({ page }) => {
  await openMdWith(page, '<p>x</p>');
  const src = await pngDataUrl(page);
  await page.evaluate((src) => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('beforeend',
      '<img id="ras" data-md-zoom width="60" height="30" src="' + src + '">');
  }, src);
  const img = page.locator('#ras');
  await expect.poll(() => img.evaluate((e) => e.complete && e.naturalWidth)).toBe(200);
  expect((await rectOf(img)).w).toBe(60);
  await img.click();
  expect(await rectOf(img)).toEqual({ w: 200, h: 100 });
  await img.click();
  expect(await isZoomed(page)).toBe(true);
  expect(await rectOf(img)).toEqual({ w: 200, h: 100 });
  // 1:1 では重ねものにフォーカスがあり、矢印や Space で動かせる。
  expect(await page.evaluate(() => document.activeElement.classList.contains('md-zoom-ov'))).toBe(true);
  await page.keyboard.press('Escape');
  expect((await rectOf(img)).w).toBe(60);
});

test('拡大は本文の枠の中に留まり、サイドバーとタブは押せるまま', async ({ page }) => {
  await openMdWith(page, FIG);
  await page.locator('#fig').click();
  expect(await page.evaluate(() =>
    document.getElementById('preview-pane').contains(document.querySelector('.md-zoom-ov')))).toBe(true);
  const hit = await page.evaluate(() => {
    const r = document.getElementById('sidebar-col').getBoundingClientRect();
    const el = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return document.getElementById('sidebar-col').contains(el);
  });
  expect(hit).toBe(true);
  // 拡大中は枠のスクロールを止め、閉じたら元に戻す。
  expect(await page.evaluate(() => getComputedStyle(document.getElementById('preview-pane')).overflowY)).toBe('hidden');
  await page.keyboard.press('Escape');
  expect(await page.evaluate(() => getComputedStyle(document.getElementById('preview-pane')).overflowY)).toBe('auto');
});

test('拡大の上に ⌘P を開くとパレットが前に出て、Esc はパレットから閉じる', async ({ page }) => {
  await openMdWith(page, FIG);
  await page.locator('#fig').click();
  await page.keyboard.press('Meta+p');
  const pal = page.locator('#md-pal-backdrop');
  await expect(pal).toBeVisible();
  const onTop = await page.evaluate(() => {
    const p = document.getElementById('md-pal-backdrop');
    const r = document.getElementById('preview-pane').getBoundingClientRect();
    const el = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return p.contains(el);
  });
  expect(onTop).toBe(true);
  await page.keyboard.press('Escape');
  await expect(pal).toHaveCount(0);
  expect(await isZoomed(page)).toBe(true);
  await page.keyboard.press('Escape');
  expect(await isZoomed(page)).toBe(false);
});

test('ファイルを切り替えると拡大は畳まれ、枠のスクロールも戻る', async ({ page }) => {
  await openMdWith(page, FIG);
  await page.locator('#fig').click();
  await treeItem(page, 'b.md').click();
  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', /b\.md$/);
  expect(await isZoomed(page)).toBe(false);
  expect(await page.evaluate(() => MdCommon.isOverlayOpen())).toBe(false);
  expect(await page.evaluate(() => document.getElementById('preview-pane').style.overflowY)).toBe('');
});

test('hydrate を通らない差し替え（設定タブ・中身の消去）でも、拡大の状態が残らない', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.keyboard.press('Meta+Comma');
  await expect(page.locator('.md-tab[data-path="md:settings"]')).toHaveCount(1);
  await treeItem(page, 'a.md').click();
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.evaluate((html) => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('beforeend', html);
  }, FIG);
  await page.locator('#fig').click();
  expect(await isZoomed(page)).toBe(true);
  await page.locator('.md-tab[data-path="md:settings"]').click();
  await expect.poll(() => isZoomed(page)).toBe(false);
  expect(await page.evaluate(() => MdCommon.isOverlayOpen())).toBe(false);
  expect(await page.evaluate(() => document.getElementById('preview-pane').style.overflowY)).toBe('');

  // 本文を直に消したときも同じ。
  await treeItem(page, 'a.md').click();
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.evaluate((html) => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('beforeend', html);
  }, FIG);
  await page.locator('#fig').click();
  await page.evaluate(() => { document.getElementById('preview-pane').innerHTML = '<div class="markdown-body"></div>'; });
  await expect.poll(() => isZoomed(page)).toBe(false);
  expect(await page.evaluate(() => document.getElementById('preview-pane').style.overflowY)).toBe('');
});

test('窓の大きさが変わると拡大を畳む', async ({ page }) => {
  await openMdWith(page, FIG);
  await page.locator('#fig').click();
  const vp = page.viewportSize();
  await page.setViewportSize({ width: vp.width - 100, height: vp.height });
  await expect.poll(() => isZoomed(page)).toBe(false);
});

test('リンクで包んだ画像・図の中のリンクは、拡大せずリンクとして働く', async ({ page }) => {
  await openMdWith(page,
    '<p><a href="#after" id="wrap"><img data-md-zoom width="40" height="20" ' +
    'src="data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%2240%22 height=%2220%22/%3E"></a></p>' +
    '<figure data-md-zoom id="lf"><a href="#after" id="inner">中のリンク</a> 図</figure>' + FIG);
  await page.locator('#wrap img').click();
  expect(await isZoomed(page)).toBe(false);
  await page.locator('#inner').click();
  expect(await isZoomed(page)).toBe(false);
});

test('html: 図の拡大は iframe いっぱいに出て、iframe の中からの Esc で閉じる', async ({ page }) => {
  const frame = await openHtmlWith(page, '<img id="zimg" data-md-zoom width="60" height="30" src="data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%2260%22 height=%2230%22/%3E">');
  await frame.locator('#zimg').click();
  expect(await isZoomed(page)).toBe(true);
  const fill = await page.evaluate(() => {
    const d = document.querySelector('iframe.html-frame').contentDocument;
    const ov = d.querySelector('.md-zoom-ov');
    const r = ov.getBoundingClientRect();
    return { pos: getComputedStyle(ov).position, w: r.width, vw: d.documentElement.clientWidth };
  });
  expect(fill.pos).toBe('fixed');
  expect(fill.w).toBeGreaterThanOrEqual(fill.vw);
  // svg の画像はベクターなので、実寸より大きくしてよい。
  expect((await rectOf(frame.locator('#zimg'))).w).toBeGreaterThan(60);
  await frame.locator('.md-zoom-ov').press('Escape');
  expect(await isZoomed(page)).toBe(false);
});

test('文書が先に拡大らしいクラスや属性を書いておいても、開いたことにならない', async ({ page }) => {
  await openMdWith(page, '<figure data-md-zoom class="md-zoomed zoomed open md-zoom-ov" data-md-zoom-placeholder>図</figure>');
  expect(await isZoomed(page)).toBe(false);
  expect(await page.evaluate(() => MdCommon.isOverlayOpen())).toBe(false);
});

test('html: 名前がすり替わる要素が配線の前からあっても、部品は動く', async ({ page }) => {
  await openMdWith(page, '<p>x</p>');
  const names = ['createElement', 'createTextNode', 'body', 'head', 'documentElement', 'defaultView',
    'scrollingElement', 'activeElement', 'querySelector', 'getElementById', 'contains',
    'addEventListener', '__mdParts', '__mdBound'];
  const html = names.map((n) => `<img name="${n}">`).join('') +
    // form は子の name で自分のプロパティ（tagName など）もすり替わる。写す相手にする。
    '<form id="copy-f" name="querySelectorAll">フォームの文字<input name="tagName"><input name="cloneNode"></form>' +
    '<button id="bf" data-md-copy="copy-f">写す</button>' +
    '<code id="copy-c">すり替えに負けない</code><button id="bc" data-md-copy="copy-c">写す</button>' +
    '<img id="cz" data-md-zoom width="40" height="20" src="data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22 width=%2240%22 height=%2220%22/%3E">';
  // 配線（bindFrame）の前から要素がある文書を作るため、srcdoc の iframe を足して配線させる。
  // 配線されるのはサーバが作る形（本文ペインの直下の `.markdown-body.html-page` の中）だけなので、
  // ペインの中身をその形に入れ替える。
  await page.evaluate((html) => {
    const f = document.createElement('iframe');
    f.className = 'html-frame';
    f.id = 'clob';
    f.srcdoc = '<!doctype html><body>' + html + '</body>';
    const holder = document.createElement('div');
    holder.className = 'markdown-body html-page';
    holder.appendChild(f);
    document.getElementById('preview-pane').replaceChildren(holder);
    MdCommon.wireHtmlFrames(document.getElementById('preview-pane'));
  }, html);
  // Playwright のロケータは form のすり替えに巻き込まれるので、押すのは素の click() で。
  const clickIn = (sel) => page.evaluate((s) => {
    const d = document.getElementById('clob').contentDocument;
    Document.prototype.querySelector.call(d, s).click();
  }, sel);
  // 配線（bindFrame）は load の後で、済むと iframe の不透明度が 1 になる。
  await expect.poll(() => page.evaluate(() => {
    const f = document.getElementById('clob');
    const d = f.contentDocument;
    return !!(d && Document.prototype.querySelector.call(d, '#bc')) && f.style.opacity === '1';
  })).toBe(true);
  await clickIn('#bc');
  await clickIn('#bf');
  await expect.poll(() => copies(page)).toEqual(['すり替えに負けない', 'フォームの文字']);
  await clickIn('#cz');
  expect(await isZoomed(page)).toBe(true);
  // 重ねもの（iframe の中）にフォーカスがあるので、Esc は iframe の中から親へ渡る。
  await page.keyboard.press('Escape');
  expect(await isZoomed(page)).toBe(false);
});

test('md の本文に position:fixed の透明な要素を置いても、サイドバーとタブを覆えない', async ({ page }) => {
  await openMdWith(page,
    '<a href="https://example.com/trap" style="position:fixed;inset:0;z-index:2147483647;opacity:0;display:block">罠</a>');
  // サイドバーの行を押すと、罠のリンクではなく行が反応する。
  await treeItem(page, 'b.md').click();
  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', /b\.md$/);
});

// ── 合図の中身と置き場所 ───────────────────────────────────────

test('写す文字の CR は LF に揃え、向きの印（LRM・RLM・ALM）は残し、埋め込みと分離の制御は落とす', async ({ page }) => {
  await openMdWith(page, '<p>x</p>');
  const got = await page.evaluate(() => new Promise((resolve) => {
    MdCommon.copyText('a\r\nb\rc‎d‏e؜f‫g⁧h⁩i', resolve, () => resolve(null));
  }));
  expect(got).toBe('a\nb\nc‎d‏e؜fghi');
  await expect.poll(() => copies(page)).toEqual(['a\nb\nc‎d‏e؜fghi']);
});

test('見えない制御の文字だけをなぞっても、何も写さず失敗とも言わない', async ({ page }) => {
  await openMdWith(page, '<p id="only">‮​⁦</p>');
  await page.evaluate(() => {
    const p = document.getElementById('only');
    getSelection().selectAllChildren(p);
  });
  const r = await page.locator('#only').boundingBox();
  await page.mouse.move(r.x + 1, r.y + r.height / 2);
  await page.mouse.up();
  await expectNoCopies(page);
  await expect(page.locator('.md-toast.show')).toHaveCount(0);
});

test('末尾の改行 1 つは行数にも文字数にも数えない', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await treeItem(page, 'zz-code.md').click();
  await expect(page.locator('#preview-pane .copy-btn').first()).toBeAttached();
  await page.evaluate(() => { document.querySelector('#preview-pane pre > code').textContent = 'x\ny\nz\n'; });
  await page.locator('#preview-pane .copy-btn').first().click({ force: true });
  await expect.poll(() => copies(page)).toEqual(['x\ny\nz\n']);
  await expect.poll(() => signalText(page)).toContain('コピーしました（3 行・5 文字）');
});

test('合図は行を折り返さず、長い行は横にスクロールして見る', async ({ page }) => {
  await openMdWith(page,
    '<pre id="copy-long">' + 'word '.repeat(300) + '\n2 行目</pre><p><button data-md-copy="copy-long">写す</button></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  const r = await probe(page, 'signalRect');
  // 2 行と見出しぶんの高さに収まる（折り返していれば何十行にもなる）。
  expect(r.h).toBeLessThan(120);
});

test('真ん中を省いた合図は、押すと全部を出し、閉じるで閉じる', async ({ page }) => {
  const rows = Array.from({ length: 100 }, (_, i) => '行 ' + (i + 1));
  await openMdWith(page,
    '<pre id="copy-rows">' + rows.join('\n') + '</pre><p><button id="r" data-md-copy="copy-rows">写す</button></p>');
  await page.locator('#r').click();
  await expect.poll(() => signalText(page)).toContain('行を省略');
  expect(await page.evaluate(() => MdParts.__test.signalAct('more').label)).toBe('全部見る');
  // 中身のあたりを押すと、全部が出る（閉じない）。
  const r = await probe(page, 'signalRect');
  await page.mouse.click(r.x + r.w / 2, r.y + r.h / 2);
  expect(await probe(page, 'signalShown')).toBe(true);
  expect(await probe(page, 'signalFull')).toBe(true);
  const t = await signalText(page);
  expect(t).toContain('行 50\n');
  expect(t).not.toContain('省略');
  // 全部を出した後に中身を押しても閉じない（選ぶ・スクロールするため）。
  const r2 = await probe(page, 'signalRect');
  await page.mouse.click(r2.x + r2.w / 2, r2.y + r2.h / 2);
  expect(await probe(page, 'signalShown')).toBe(true);
  // 高さには上限があり、窓の下へはみ出さない。
  const vh = page.viewportSize().height;
  expect(r2.y + r2.h).toBeLessThanOrEqual(vh);
  // 時間が経っても、乗せている間は消えない。
  await page.waitForTimeout(3600);
  expect(await probe(page, 'signalShown')).toBe(true);
  const close = await page.evaluate(() => MdParts.__test.signalAct('close'));
  await page.mouse.click(close.x + close.w / 2, close.y + close.h / 2);
  expect(await probe(page, 'signalShown')).toBe(false);
});

test('合図は本文の枠の上端の少し下（タブ帯の下）に出る', async ({ page }) => {
  await openMdWith(page, '<p><button data-md-copy="copy-t">写す</button> <code id="copy-t">位置</code></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  const r = await probe(page, 'signalRect');
  const pane = await page.locator('#preview-pane').boundingBox();
  const tabbar = await page.locator('#tabbar').boundingBox();
  expect(r.y).toBeGreaterThanOrEqual(tabbar.y + tabbar.height);
  expect(Math.abs(r.y - (pane.y + 8))).toBeLessThan(2);
});

test('本文の描き直し（hydrate）は拡大を畳むが、出したばかりの合図は消さない', async ({ page }) => {
  await openMdWith(page, FIG + '<p><button data-md-copy="copy-hy">写す</button> <code id="copy-hy">残る</code></p>');
  await page.locator('#preview-pane button[data-md-copy]').click();
  await expect.poll(() => probe(page, 'signalShown')).toBe(true);
  await page.locator('#fig').click();
  expect(await isZoomed(page)).toBe(true);
  await page.evaluate(() => MdCommon.hydrate(document.querySelector('#preview-pane .markdown-body')));
  expect(await isZoomed(page)).toBe(false);
  expect(await probe(page, 'signalShown')).toBe(true);
});

test('見出しに自動で付いた id は data-md-copy に拾われず、書き手の同じ id が勝つ', async ({ page }) => {
  await openMdWith(page, '<h2 id="h-auto" class="h-auto">Copy settings</h2>');
  // 見出しだけが先にあり、自動の id が付いたあとで、書き手の要素が足される形。
  await page.evaluate(() => {
    const h = document.getElementById('h-auto');
    h.removeAttribute('id');
    MdCommon.ensureHeadingIds(document.querySelector('#preview-pane .markdown-body'));
  });
  expect(await page.evaluate(() => document.querySelector('#preview-pane .h-auto').id)).toBe('copy-settings');
  await page.evaluate(() => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('beforeend',
      '<p><button id="bs" data-md-copy="copy-settings">写す</button> <code id="copy-settings">書き手の中身</code></p>' +
      '<p><button id="bh" data-md-copy="copy-heading">写す</button></p>');
    MdCommon.ensureHeadingIds(document.querySelector('#preview-pane .markdown-body'));
  });
  await page.locator('#bs').click();
  await expect.poll(() => copies(page)).toEqual(['書き手の中身']);
  // 自動の id しか無いときは、見つからない扱い。
  await page.evaluate(() => {
    document.querySelector('#preview-pane .markdown-body').insertAdjacentHTML('afterbegin', '<h3>Copy heading</h3>');
    MdCommon.ensureHeadingIds(document.querySelector('#preview-pane .markdown-body'));
  });
  expect(await page.evaluate(() => !!document.getElementById('copy-heading'))).toBe(true);
  await page.locator('#bh').click();
  await expect.poll(() => signalText(page)).toContain('見つかりません');
  expect(await copies(page)).toEqual(['書き手の中身']);
});

// ── 拡大と文書の CSS ───────────────────────────────────────────

test('拡大した図にも文書の CSS が当たり続ける（重ねものは .markdown-body の中）', async ({ page }) => {
  await openMdWith(page,
    '<style>.paint { fill: rgb(1, 2, 3); } #cssfig figcaption { font-size: 21px; }</style>' +
    '<figure data-md-zoom id="cssfig"><svg width="80" height="40"><rect class="paint" width="80" height="40"/></svg>' +
    '<figcaption>説明</figcaption></figure>');
  await page.evaluate(() => MdDocGuard.guard(document.querySelector('#preview-pane .markdown-body')));
  const look = () => page.evaluate(() => ({
    fill: getComputedStyle(document.querySelector('#cssfig .paint')).fill,
    size: getComputedStyle(document.querySelector('#cssfig figcaption')).fontSize,
  }));
  expect(await look()).toEqual({ fill: 'rgb(1, 2, 3)', size: '21px' });
  await page.locator('#cssfig').click();
  expect(await isZoomed(page)).toBe(true);
  expect(await page.evaluate(() =>
    !!document.querySelector('#preview-pane > .markdown-body > .md-zoom-ov #cssfig'))).toBe(true);
  expect(await look()).toEqual({ fill: 'rgb(1, 2, 3)', size: '21px' });
});

test('枠を下までスクロールしてから開いても、重ねものは枠の見えている範囲にぴったり出る', async ({ page }) => {
  await openMdWith(page, '<div style="height:3000px">余白</div>' + FIG + '<div style="height:600px"></div>');
  await page.locator('#fig').scrollIntoViewIfNeeded();
  expect(await page.evaluate(() => document.getElementById('preview-pane').scrollTop)).toBeGreaterThan(1000);
  await page.locator('#fig').click();
  const got = await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    const pr = p.getBoundingClientRect();
    const o = document.querySelector('.md-zoom-ov').getBoundingClientRect();
    const h = document.querySelector('.md-zoom-hint').getBoundingClientRect();
    return {
      dx: o.left - (pr.left + p.clientLeft), dy: o.top - (pr.top + p.clientTop),
      dw: o.width - p.clientWidth, dh: o.height - p.clientHeight,
      hintTop: h.top - o.top, hintRight: o.right - h.right,
    };
  });
  expect(Math.abs(got.dx)).toBeLessThan(1);
  expect(Math.abs(got.dy)).toBeLessThan(1);
  expect(Math.abs(got.dw)).toBeLessThan(1);
  expect(Math.abs(got.dh)).toBeLessThan(1);
  expect(Math.abs(got.hintTop - 8)).toBeLessThan(1);
  expect(Math.abs(got.hintRight - 8)).toBeLessThan(1);
});

test('画面に合わせても 1:1 でも大きさが変わらない図は、手がかりが「Esc で閉じる」だけで押しても切り替えない', async ({ page }) => {
  await openMdWith(page, '<figure data-md-zoom id="tf"><span id="tfs">文字だけの図</span></figure>');
  await page.locator('#tf').click();
  expect(await isZoomed(page)).toBe(true);
  expect(await probe(page, 'hintText')).toBe('Esc で閉じる');
  await page.locator('#tfs').click();
  expect(await isZoomed(page)).toBe(true);
  expect(await probe(page, 'hintText')).toBe('Esc で閉じる');
  expect(await page.locator('.md-zoom-ov').evaluate((e) => e.classList.contains('md-zoom-actual'))).toBe(false);
});
