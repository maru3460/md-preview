// 表示モード（viewmode.js）。raw（⌘R）と 差分（⌘D）の排他と、読み位置の持ち回り。
//
// 2 つが同時に active にならないこと、非 md ではそもそも raw を出さないことを見る。
// 読み位置は行（data-src-line）で持ち回っているので、ピクセルではなく行で測る。
const { test, expect } = require('@playwright/test');
const { openFolder } = require('./helpers');

// 画面上端に来ている行番号。レンダリング結果とソースで高さが違うので、ピクセルの
// scrollTop ではなくこの値が保たれることを見る。背の高いユニット（埋め込み）は
// 上端をまたいでいても「そこを読んでいる」で正しいので、そのまま採る。
const topLine = (page) => page.evaluate(() => {
  const p = document.getElementById('preview-pane');
  const vt = p.getBoundingClientRect().top;
  for (const u of p.querySelectorAll('[data-src-line]')) {
    if (u.getBoundingClientRect().bottom > vt) return parseInt(u.dataset.srcLine, 10);
  }
  return null;
});

// エンジンの助けを切って開く。WebKit の scroll anchoring は画面外で高さが増えたぶんを
// 補ってしまい、実機の WKWebView では抜けているズレを緑にする（playwright.config.js の
// 「このスイートが守れないもの」参照）。読み位置のテストは全部これを通す。
async function openBare(page, file) {
  await openFolder(page);
  await page.addStyleTag({ content: '#preview-pane, #preview-pane * { overflow-anchor: none !important; }' });
  await page.locator('.tree-item', { hasText: file }).click();
  await expect(page.locator('#preview-pane .markdown-body')).toBeVisible();
  await settled(page);
}

// 読み位置が動かなくなるまで待つ。固定の sleep では common.js の追随窓（1.5 秒）の
// 途中で測ってしまう。
async function settled(page) {
  await page.waitForFunction(() => {
    const p = document.getElementById('preview-pane');
    const now = p.scrollTop + ':' + p.scrollHeight;
    if (window.__mdLast === now) window.__mdStill = (window.__mdStill || 0) + 1;
    else { window.__mdLast = now; window.__mdStill = 0; }
    return window.__mdStill >= 5;
  }, null, { polling: 100, timeout: 15000 });
}

// ⌘R を押して、本文が差し替わり・遅延描画が終わり・読み位置が落ち着くまで待つ。
// mermaid の描画を待たずに測ると、伸びる前の（正しい）位置を見てバグを取りこぼす。
async function toggleView(page) {
  const gen = await page.evaluate(() => MdCommon.bodyGen());
  await page.keyboard.press('Meta+r');
  await page.waitForFunction((g) => MdCommon.bodyGen() > g, gen);
  await page.waitForFunction(() => {
    const p = document.getElementById('preview-pane');
    for (const n of p.querySelectorAll('pre.mermaid')) if (!n.querySelector('svg')) return false;
    return true;
  }, null, { timeout: 15000 });
  await settled(page);
}

// raw が出ているか（フラグメントの外枠は .markdown-body なので、それでは判別できない）。
const rawShown = (page) => page.locator('.source-view');

test('⌘R と ⌘D は排他で、非 md では raw が無効になる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md を開く
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  const raw = page.locator('.md-raw-toggle');
  const diff = page.locator('.md-diff-toggle');

  // raw ON → ソースが出る（レンダリング前の `# 見出し A` が見える）。
  await page.keyboard.press('Meta+r');
  await expect(raw).toHaveClass(/active/);
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(page.locator('#preview-pane')).toContainText('# 見出し A');

  // diff ON → raw は自動で畳まれる（同時に 2 つ active にならない）。
  await page.keyboard.press('Meta+d');
  await expect(diff).toHaveClass(/active/);
  await expect(raw).not.toHaveClass(/active/);

  // もう一度 ⌘D で通常表示へ戻る。
  await page.keyboard.press('Meta+d');
  await expect(diff).not.toHaveClass(/active/);
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  // 非 md（notes.txt）は通常表示が既にソースなので raw トグルを隠す。
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('#preview-pane')).toContainText('md ではないテキストファイル');
  await expect(raw).toBeHidden();
});

test('⌘R の往復で読み位置（行）が保たれる', async ({ page }) => {
  await openBare(page, 'long.md');

  // 真ん中まで読み進めた状態を作る。
  await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    p.scrollTop = Math.round(p.scrollHeight * 0.5);
  });
  const before = await topLine(page);
  expect(before).toBeGreaterThan(50);

  // raw へ。段落は 1 行 1 ユニットへ割れるので厳密一致は求めず、数行の幅で見る。
  await toggleView(page);
  await expect(rawShown(page)).toHaveCount(1);
  expect(Math.abs((await topLine(page)) - before)).toBeLessThanOrEqual(3);

  // 通常表示へ戻しても同じ行に居ること（戻り側も同じ錨を通る）。
  await toggleView(page);
  await expect(rawShown(page)).toHaveCount(0);
  expect(Math.abs((await topLine(page)) - before)).toBeLessThanOrEqual(3);
});

// 錨の行がレンダリング結果に存在しないケース。見出しの直前の空行はソース表示では
// 1 行 1 ユニットだが、レンダリング結果には対応する要素が無い。ここで錨を諦めると
// ピクセルへ落ちて読み位置が飛ぶ（sample.md で ⌘R を連打すると中見出しの手前まで
// 巻き戻された）。空行を確実に錨にするため、raw 側から始めて空行を上端に置く。
test('空行を錨にしても ⌘R 連打で読み位置が動かない', async ({ page }) => {
  await openBare(page, 'long.md');

  // raw に切り替えてから、「後半の見出し」の直前の空行を画面上端に置く。
  await toggleView(page);
  await expect(rawShown(page)).toHaveCount(1);
  const blank = await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    const rows = Array.from(p.querySelectorAll('.md-src-row'));
    const h2 = rows.find((r) => r.textContent.includes('## 後半の見出し'));
    const prev = rows[rows.indexOf(h2) - 1];
    if (prev.textContent.trim() !== '') throw new Error('直前が空行ではない: ' + prev.textContent);
    // +1px。ちょうど揃えると 1 つ上の行の下端が端数ぶん画面に残り、そちらが錨になる。
    p.scrollTop += prev.getBoundingClientRect().top - p.getBoundingClientRect().top + 1;
    return parseInt(prev.dataset.srcLine, 10);
  });
  await settled(page);
  // 錨がその空行になっていること（この行が通常表示に無いのが試したい条件）。
  const a0 = await page.evaluate(() => MdCommon.readAnchor(document.getElementById('preview-pane')));
  expect(a0.line).toBe(blank);
  await page.evaluate((l) => {
    if (document.querySelector('#preview-pane .markdown-body').querySelectorAll('[data-src-line="' + l + '"]').length !== 1) {
      throw new Error('raw 側に行が無い');
    }
  }, blank);

  // 通常表示にはこの行に対応する要素が無い。いちばん近い下のユニット（見出し）へ
  // 寄るので、往復しても数行の幅に収まる。
  for (let i = 0; i < 6; i++) {
    await toggleView(page);
    expect(Math.abs((await topLine(page)) - blank)).toBeLessThanOrEqual(3);
  }
});

// 背の高い埋め込み（md の 1 行がレンダリングでは数百 px）の中ほどから ⌘R すると、
// 行内オフセットがソース側の 20px の行に乗って十数行ぶん下へ抜けていた。
test('背の高い埋め込みの中から ⌘R しても、その行から離れない', async ({ page }) => {
  await openBare(page, 'long.md');

  const embedLine = await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    const embeds = p.querySelectorAll('.code-embed');
    if (embeds.length !== 1) throw new Error('埋め込みが 1 つである前提: ' + embeds.length);
    const e = embeds[0];
    const r = e.getBoundingClientRect();
    p.scrollTop += r.top - p.getBoundingClientRect().top + r.height * 0.5;  // 中ほど
    return parseInt(e.closest('[data-src-line]').dataset.srcLine, 10);
  });
  await settled(page);

  for (let i = 0; i < 4; i++) {
    await toggleView(page);
    expect(Math.abs((await topLine(page)) - embedLine)).toBeLessThanOrEqual(3);
  }
});

// 差し替えの後で高さが変わる要素（mermaid / drawio / 画像）が錨より上にあるケース。
// hydrate の直後に寄せた時点では未描画で、そこから伸びたぶん読み位置が上へ抜ける。
// ⌘R を往復するたびに少しずつ上がって、読み位置が mermaid 自身に届くと止まる、
// という形で出た（sample.md で 211 → 207 → 193 → 183 → 157 → 115 行目）。
test('錨より上の mermaid が後から描画されても読み位置が上へ抜けない', async ({ page }) => {
  await openBare(page, 'zz-mermaid.md');
  await expect(page.locator('#preview-pane .mermaid svg').first()).toBeVisible();

  await page.evaluate(() => {
    Array.from(document.querySelectorAll('#preview-pane h2'))
      .find((h) => h.textContent.includes('目印の見出し'))
      .scrollIntoView({ block: 'start' });
  });
  await settled(page);
  const before = await topLine(page);
  expect(before).toBeGreaterThan(80);

  for (let i = 0; i < 8; i++) {
    await toggleView(page);
    expect(Math.abs((await topLine(page)) - before)).toBeLessThanOrEqual(3);
  }
});

// 閉じた <details> の中身は、レイアウトされていて矩形も持つ（WebKit では
// offsetParent も null にならない）。錨がそれを掴むと、見えていない行を指してしまう。
test('閉じた <details> の中身は錨にならない', async ({ page }) => {
  await openBare(page, 'zz-details.md');

  const hidden = await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    const d = p.querySelector('details:not([open])');
    // 畳まれた中のユニットの行番号（これを錨にしてはいけない）
    const inside = Array.from(d.querySelectorAll('[data-src-line]'))
      .map((u) => parseInt(u.dataset.srcLine, 10));
    // 閉じた details のすぐ下を読んでいる状態にする
    const r = d.getBoundingClientRect();
    p.scrollTop += r.bottom - p.getBoundingClientRect().top;
    return inside;
  });
  await settled(page);
  expect(hidden.length).toBeGreaterThan(0);

  const a = await page.evaluate(() => MdCommon.readAnchor(document.getElementById('preview-pane')));
  expect(hidden).not.toContain(a.line);

  const before = await topLine(page);
  for (let i = 0; i < 4; i++) {
    await toggleView(page);
    expect(Math.abs((await topLine(page)) - before)).toBeLessThanOrEqual(3);
  }
});

// <details> は生 HTML 経由なので data-src-end-line を持たず、本文全体を囲む
// 「1 行のユニット」になる。文書順の最初で錨を決めると、開いた <details> の中は
// どの行も錨になれず、読み位置がブロックの開きタグまで戻される。
test('開いた <details> の中の行が錨になる', async ({ page }) => {
  await openBare(page, 'zz-details.md');

  const want = await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    const d = p.querySelector('details[open]');
    const inner = Array.from(d.querySelectorAll('p[data-src-line]'))
      .find((e) => e.textContent.includes('開いた中の段落 2'));
    p.scrollTop += inner.getBoundingClientRect().top - p.getBoundingClientRect().top;
    return { inner: parseInt(inner.dataset.srcLine, 10), open: parseInt(d.dataset.srcLine, 10) };
  });
  await settled(page);

  const a = await page.evaluate(() => MdCommon.readAnchor(document.getElementById('preview-pane')));
  expect(a.line).toBe(want.inner);
  expect(a.line).not.toBe(want.open);
});

// ユニットに覆われていない領域（生 HTML のブロック・余白・<hr>）が画面上端に来ると、
// 錨の行は「その下で最初に見つかるユニット」になり、オフセットはその領域の高さぶん
// 正の値になる。この高さは表示ごとに違うので持ち回れない——ピクセルのまま渡すと、
// ソース側では行の高さの何十倍にもなって大きく下へ抜ける。
test('行番号を持たない領域の高さは持ち回らない', async ({ page }) => {
  await openBare(page, 'zz-details.md');

  const want = await page.evaluate(() => {
    const p = document.getElementById('preview-pane');
    const box = Array.from(p.querySelectorAll('#preview-pane > * div, div'))
      .find((d) => d.style && d.style.height === '600px');
    const next = Array.from(p.querySelectorAll('p[data-src-line]'))
      .find((e) => e.textContent.includes('区切りの段落'));
    const r = box.getBoundingClientRect();
    p.scrollTop += r.top - p.getBoundingClientRect().top + Math.round(r.height * 0.5);
    return { boxH: Math.round(r.height), next: parseInt(next.dataset.srcLine, 10) };
  });
  await settled(page);
  expect(want.boxH).toBeGreaterThan(500);  // 枠線ぶん 600 より少し大きい

  // 錨はその下のユニットで、オフセットは持ち回らない（0）。
  const a = await page.evaluate(() => MdCommon.readAnchor(document.getElementById('preview-pane')));
  expect(a.line).toBe(want.next);
  expect(a.offset).toBe(0);

  // raw でもその行の近くに着地する（600px を 20px の行に乗せて 30 行下へ抜けない）。
  await toggleView(page);
  await expect(rawShown(page)).toHaveCount(1);
  expect(Math.abs((await topLine(page)) - want.next)).toBeLessThanOrEqual(3);
});

// 追随（common.js の restoreAnchor）の解放条件。表示の切り替え（⌘R / ⌘D）以外のキーは
// 「読み進めた」として手を離す。修飾キー付きを丸ごと除外すると、⌘F（検索は開くと
// 1 件目へ飛ぶ）や ⌘↑ / ⌘↓ が引き戻される。逆に ⌘R を除外しないと、連打が自分の
// 直前の追随を止めて、未描画のまま次の錨を読みズレが積もる。
//
// 追随が生きているかは「錨と違う位置へ移してから高さを動かす」で見分ける。生きて
// いれば錨へ引き戻され、離していればその場に留まる（scroll anchoring が支える）。
test('表示の切り替え以外のキーは読み位置の追随に手を離させる', async ({ page }) => {
  const setup = async () => {
    await page.evaluate(() => { document.getElementById('preview-pane').scrollTop = 0; });
    await settled(page);
    const gen = await page.evaluate(() => MdCommon.bodyGen());
    await page.keyboard.press('Meta+r');           // ここで追随が張られる（錨 = 先頭）
    await page.waitForFunction((g) => MdCommon.bodyGen() > g, gen);
    // 錨とは違う位置へ。代入はイベントを出さないので追随は張られたまま。
    return await page.evaluate(() => {
      const p = document.getElementById('preview-pane');
      p.scrollTop = Math.round(p.scrollHeight * 0.6);
      return p.scrollTop;
    });
  };
  // 高さを動かして追随を発火させ、その後の scrollTop を返す。openBare は scroll
  // anchoring を切っているので、手を離していれば scrollTop は動かない。生きていれば
  // 錨（先頭）へ引き戻されて小さくなる。
  const nudge = async () => {
    await page.evaluate(() => {
      const body = document.querySelector('#preview-pane > *');
      const pad = document.createElement('div');
      pad.style.height = '400px';
      pad.className = 'zz-pad';
      body.insertBefore(pad, body.firstChild);
    });
    await page.waitForTimeout(400);
    return await page.evaluate(() => {
      const p = document.getElementById('preview-pane');
      const top = p.scrollTop;
      p.querySelector('.zz-pad').remove();
      return Math.round(top);
    });
  };

  await openBare(page, 'zz-mermaid.md');

  // 修飾キー単体は「読み進めた」ではない。⌘R は Meta → r の 2 発来るので、
  // 1 発目で離すと表示の切り替えを見分ける前に解放してしまう。
  let moved = await setup();
  await page.keyboard.down('Shift');
  await page.keyboard.up('Shift');
  expect(await nudge(), 'Shift 単体で手を離してしまった').toBeLessThan(moved / 2);

  // ⌘F は表示の切り替えではないので手を離す（＝その場に留まる）。
  moved = await setup();
  await page.keyboard.press('Meta+f');
  await page.keyboard.press('Escape');
  expect(Math.abs((await nudge()) - moved), '⌘F で手を離していない').toBeLessThan(50);
});
