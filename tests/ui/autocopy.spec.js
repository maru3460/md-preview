// 選択即コピー（autocopy.js）。マウスでなぞるとその場でクリップボードへ入る。
//
// 見ているのは主に「走ってはいけない時に走らないか」。この機能はわざと選択を
// 残すので、素朴に mouseup だけで判定すると以後あらゆるクリックが再コピーになる。
// 選択を保つために mousedown を preventDefault している UI（右クリックメニュー・
// タブ・サイドバーのリサイザ）がいくつもあり、実装中にそこで全部誤爆した。
// 下の「再コピーされない」系はその回帰である。
//
// クリップボードの中身は window.__mdIpc（serve.rs の IPC スタブ）で見る。
// そのためには navigator.clipboard 側を失敗させて Rust への経路へ落とす必要がある
// ——実機の mdpreview:// でも、ユーザー ジェスチャの外から呼ぶと同じ道を通る。
const { test, expect } = require('@playwright/test');
const { MULTI_URL, open, openFolder } = require('./helpers');

/// clipboard を必ず失敗させて IPC 経路に寄せる。中身を観測するのはこの道しかない。
async function stubClipboard(page) {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      get: () => ({ writeText: () => Promise.reject(new Error('stubbed')) })
    });
  });
}

/// これまでに飛んだ copy: の一覧（本文だけ）。
function copies(page) {
  return page.evaluate(() =>
    window.__mdIpc.filter((m) => m.indexOf('copy:') === 0).map((m) => m.slice(5)));
}

async function clearIpc(page) {
  await page.evaluate(() => { window.__mdIpc.length = 0; });
}

/// 要素の 1 行目を左から右へなぞる。
async function dragAcross(page, locator, width) {
  const box = await locator.boundingBox();
  const y = box.y + Math.min(box.height / 2, 10);
  await page.mouse.move(box.x + 4, y);
  await page.mouse.down();
  await page.mouse.move(box.x + Math.min(width || 200, box.width - 8), y, { steps: 8 });
  await page.mouse.up();
}

const tick = (page) => page.locator('.md-copy-tick');

test('本文をなぞるとコピーされ、選択は残り、カーソル脇にチェックが出る', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.locator('.tree-item[data-path="a.md"]').click();
  const para = page.locator('.markdown-body p').first();
  await expect(para).toBeVisible();

  await clearIpc(page);
  await dragAcross(page, para);

  const got = await copies(page);
  expect(got).toHaveLength(1);
  expect(got[0].length).toBeGreaterThan(0);

  // なぞった範囲がそのままクリップボードへ入っていること。
  const selected = await page.evaluate(() => String(getSelection()));
  expect(got[0]).toBe(selected);

  // 選択は消えない（⌘C 無しで続けて読めるのがこの機能の前提）。
  expect(await page.evaluate(() => getSelection().isCollapsed)).toBe(false);

  // 合図はカーソルの脇。画面下中央のトーストは出さない。
  await expect(tick(page)).toHaveClass(/show/);
  expect(await tick(page).evaluate((el) => el.style.left !== '' && el.style.top !== '')).toBe(true);
  await expect(page.locator('.md-toast.show')).toHaveCount(0);

  // 読み上げ用の文字は持つ（チェックだけでは視覚に依存するため）。
  await expect(tick(page)).toHaveAttribute('role', 'status');
  await expect(tick(page).locator('.md-a11y')).toHaveText('コピーしました');

  // 表示は一瞬で、文言も残さない（DOM に residue を置かない）。
  await expect(tick(page)).not.toHaveClass(/show/);
  await expect(tick(page).locator('.md-a11y')).toHaveText('');
});

test('ただのクリックでは走らない', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.locator('.tree-item[data-path="a.md"]').click();
  const para = page.locator('.markdown-body p').first();
  await expect(para).toBeVisible();

  await clearIpc(page);
  await para.click();
  expect(await copies(page)).toHaveLength(0);
});

test('選択が残ったままでも、右クリックでは再コピーされない', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.locator('.tree-item[data-path="a.md"]').click();
  const para = page.locator('.markdown-body p').first();
  await expect(para).toBeVisible();
  await dragAcross(page, para);

  // 右クリックはメニューを開くだけ。選択は残るが、コピーし直してはいけない。
  await clearIpc(page);
  const box = await para.boundingBox();
  await page.mouse.move(box.x + 20, box.y + 5);
  await page.mouse.down({ button: 'right' });
  await page.mouse.up({ button: 'right' });
  await expect(page.locator('#md-context-menu')).toBeVisible();
  expect(await copies(page)).toHaveLength(0);

  // メニューの行を選んでも同じ。ここは mousedown を preventDefault して
  // 選択を保つので、素朴な実装だと必ず誤爆する。
  await page.locator('#md-context-menu .md-context-menu-item', { hasText: '再読み込み' }).first().click();
  expect(await copies(page)).toHaveLength(0);
});

test('選択が残ったままタブを切り替えても再コピーされない', async ({ page }) => {
  await stubClipboard(page);
  await open(page, MULTI_URL);
  const para = page.locator('.markdown-body p').first();
  await expect(para).toBeVisible();
  await dragAcross(page, para);

  // タブも mousedown を preventDefault する（フォーカスを奪わないため）。
  await clearIpc(page);
  await page.locator('.md-tab').nth(1).click();
  expect(await copies(page)).toHaveLength(0);
});

test('検索バーの入力欄でなぞってもクリップボードを奪わない', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.keyboard.press('/');
  const input = page.locator('.md-search-input');
  await input.fill('検索語をここで選び直す');

  await clearIpc(page);
  await dragAcross(page, input, 120);
  expect(await copies(page)).toHaveLength(0);
});

test('コメントモード中は走らない（ドラッグが行の範囲選択に割り当たっている）', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.locator('.tree-item[data-path="a.md"]').click();
  const para = page.locator('.markdown-body p').first();
  await expect(para).toBeVisible();

  await page.keyboard.press('c');
  expect(await page.evaluate(() => window.MdComment.isMode())).toBe(true);

  await clearIpc(page);
  await dragAcross(page, para);
  expect(await copies(page)).toHaveLength(0);
});

test('html の iframe 内でも効き、合図は親の画面に出る', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.locator('.tree-item[data-path="page.html"]').click();
  const frame = page.locator('iframe.html-frame');
  await expect(frame).toBeVisible();

  const para = page.frameLocator('iframe.html-frame').locator('p').first();
  await clearIpc(page);
  await dragAcross(page, para, 300);

  const got = await copies(page);
  expect(got).toHaveLength(1);
  const inner = await page.evaluate(() =>
    String(document.querySelector('iframe.html-frame').contentDocument.getSelection()));
  expect(got[0]).toBe(inner);

  // 合図は親の document に出す。座標は iframe のビューポート基準で来るので、
  // iframe の位置ぶん足していないと、まったく違う場所に出る。
  await expect(tick(page)).toHaveClass(/show/);
  const geom = await page.evaluate(() => {
    const t = document.querySelector('.md-copy-tick');
    const r = document.querySelector('iframe.html-frame').getBoundingClientRect();
    return { top: parseFloat(t.style.top), frameTop: r.top, inParent: t.ownerDocument === document };
  });
  expect(geom.inParent).toBe(true);
  expect(geom.top).toBeGreaterThan(geom.frameTop);
});

test('チェックは画面の端で内側へ折り返す', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);
  await page.locator('.tree-item[data-path="a.md"]').click();
  await expect(page.locator('.markdown-body p').first()).toBeVisible();

  // 座標だけを指定してジェスチャを合成する（端まで実際にドラッグできる本文が
  // あるとは限らないため）。順序は実物と同じ mousedown → 選択 → mouseup。
  const at = async (x, y) => {
    await page.evaluate(([x, y]) => {
      const el = document.querySelector('.markdown-body p');
      getSelection().removeAllRanges();
      el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0, clientX: x, clientY: y }));
      const r = document.createRange();
      r.selectNodeContents(el);
      getSelection().addRange(r);
      el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, button: 0, clientX: x, clientY: y }));
    }, [x, y]);
    await expect(tick(page)).toHaveClass(/show/);
    return page.evaluate(() => {
      const b = document.querySelector('.md-copy-tick').getBoundingClientRect();
      return { left: b.left, top: b.top, right: b.right, bottom: b.bottom,
               w: window.innerWidth, h: window.innerHeight };
    });
  };

  const { width, height } = page.viewportSize();
  const corner = await at(width - 4, height - 4);
  expect(corner.right).toBeLessThanOrEqual(corner.w);
  expect(corner.bottom).toBeLessThanOrEqual(corner.h);

  const origin = await at(2, 2);
  expect(origin.left).toBeGreaterThanOrEqual(0);
  expect(origin.top).toBeGreaterThanOrEqual(0);
});

test('NUL を含むテキストは黙って切らずに失敗として返す', async ({ page }) => {
  await stubClipboard(page);
  await openFolder(page);

  // wry は IPC の本文を CStr で読むので、NUL 以降が落ちる。
  // 一部だけ入れて成功と言うと、コピーできたつもりで貼りに行くことになる。
  await clearIpc(page);
  const outcome = await page.evaluate(() => new Promise((res) => {
    MdCommon.copyText('a' + String.fromCharCode(0) + 'b', () => res('done'), () => res('fail'));
  }));
  expect(outcome).toBe('fail');
  expect(await copies(page)).toHaveLength(0);
});
