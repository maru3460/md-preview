// 通知ベル（notify.js / #32）。
//
// 一覧を持っているのは Rust 側なので、ここでは `MdNotify.push` を直接叩いて
// 「渡された」状態を作る。Rust との往復（既読を書いて渡し直す）は serve.rs に
// 無いので、押した結果として **IPC に何が出たか**で確かめる。
const { test, expect } = require('@playwright/test');
const { openFolder, id, tab, display } = require('./helpers');

/// Rust から全件が渡ってきた、という状態を作る。`at` は相対時刻を安定させるため
/// テスト時刻からの引き算で作る（固定の UNIX 秒を書くと日が変わるたび表示が動く）。
async function push(page, items) {
  await page.evaluate((rows) => {
    const now = Math.floor(Date.now() / 1000);
    window.MdNotify.push(rows.map((r) => ({ path: r.path, at: now - r.ago, read: !!r.read })));
  }, items);
}

/// 既定の 3 件（未読 2・既読 1）。
async function pushSample(page) {
  await push(page, [
    { path: id(page, 'a.md'), ago: 120 },
    { path: id(page, 'sub/a.md'), ago: 4000 },
    { path: id(page, 'b.md'), ago: 90000, read: true },
  ]);
}

const bell = (page) => page.locator('#tabbar-bell');
const count = (page) => page.locator('#tabbar-bell .md-bell-count');
const panel = (page) => page.locator('.md-bell-panel');
const rows = (page) => page.locator('.md-bell-row');
const ipc = (page) => page.evaluate(() => window.__mdIpc.filter((m) => m.startsWith('notify:')));

test('ベルはタブが 0 枚でもアイコン帯に出ていて、未読が無ければ数を出さない', async ({ page }) => {
  await openFolder(page);
  // #30 で帯が常設になったので、ファイルを 1 つも開いていなくてもベルは在る。
  await expect(bell(page)).toBeVisible();
  await expect(bell(page)).toHaveAttribute('title', '通知 (b)');
  await expect(count(page)).toBeHidden();

  await pushSample(page);
  // 数えるのは未読だけ。3 件届いていても既読の 1 件は数に入らない。
  await expect(count(page)).toHaveText('2');
});

test('b で開き、1 件がファイル名と「親フォルダ · 相対時刻」の 2 段で並ぶ', async ({ page }) => {
  await openFolder(page);
  await pushSample(page);

  await page.keyboard.press('b');
  await expect(panel(page)).toBeVisible();
  await expect(rows(page)).toHaveCount(3);

  // 渡された順（新しい順）がそのまま並び。
  await expect(rows(page).nth(0).locator('.md-bell-name')).toHaveText('a.md');
  // root 直下のファイルに親フォルダ名は添えない。
  await expect(rows(page).nth(0).locator('.md-bell-sub')).toHaveText('2 分前');
  // 同名（a.md）が 2 件あっても、root より下に居る方には親フォルダ名が付く。
  await expect(rows(page).nth(1).locator('.md-bell-name')).toHaveText('a.md');
  await expect(rows(page).nth(1).locator('.md-bell-sub')).toHaveText('sub · 1 時間前');
  await expect(rows(page).nth(2).locator('.md-bell-sub')).toHaveText('昨日');

  // 既読は沈むが消えない。
  await expect(rows(page).nth(2)).toHaveClass(/is-read/);
  await expect(rows(page).nth(0)).toHaveClass(/is-unread/);

  await page.keyboard.press('Escape');
  await expect(panel(page)).toHaveCount(0);
});

test('j/k でカーソルが動き、Enter でタブが開いて既読の要求が飛ぶ', async ({ page }) => {
  await openFolder(page);
  await pushSample(page);
  await page.keyboard.press('b');

  // 開いた直後は先頭。そのまま Enter で最新を開ける。
  await expect(rows(page).nth(0)).toHaveClass(/is-cursor/);
  await page.keyboard.press('j');
  await expect(rows(page).nth(1)).toHaveClass(/is-cursor/);
  await page.keyboard.press('k');
  await expect(rows(page).nth(0)).toHaveClass(/is-cursor/);
  // 端は端で止まる（巡回しない）。
  await page.keyboard.press('k');
  await expect(rows(page).nth(0)).toHaveClass(/is-cursor/);

  await page.keyboard.press('Enter');
  await expect(tab(page, 'a.md')).toHaveClass(/active/);
  expect(await ipc(page)).toEqual(['notify:read:' + id(page, 'a.md')]);
  // 開いても一覧は閉じない。
  await expect(panel(page)).toBeVisible();
});

test('行をクリックしても同じ経路を通る', async ({ page }) => {
  await openFolder(page);
  await pushSample(page);
  await page.keyboard.press('b');

  await rows(page).nth(1).click();
  expect(await display(page, await page.locator('.md-tab.active').getAttribute('data-path')))
    .toBe('sub/a.md');
  expect(await ipc(page)).toEqual(['notify:read:' + id(page, 'sub/a.md')]);
});

test('「すべて既読」は未読があるときだけ出る', async ({ page }) => {
  await openFolder(page);

  // 全部既読の状態では出さない。
  await push(page, [{ path: id(page, 'a.md'), ago: 60, read: true }]);
  await page.keyboard.press('b');
  await expect(page.locator('.md-bell-readall')).toHaveCount(0);
  await page.keyboard.press('Escape');

  await pushSample(page);
  await page.keyboard.press('b');
  await page.locator('.md-bell-readall').click();
  expect(await ipc(page)).toEqual(['notify:read-all']);
});

test('root の外のファイルも、絶対パスのまま名前とフォルダ名が出る', async ({ page }) => {
  await openFolder(page);
  // 送る側は絶対パスを持ってくるので、受け取る窓がどこを見ていても届く（#32 の前提）。
  await push(page, [{ path: '/tmp/other-repo/docs/README.md', ago: 300 }]);
  await page.keyboard.press('b');

  await expect(rows(page).nth(0).locator('.md-bell-name')).toHaveText('README.md');
  await expect(rows(page).nth(0).locator('.md-bell-sub')).toHaveText('docs · 5 分前');
  // ツールチップは root を剥がない絶対パス。
  await expect(rows(page).nth(0)).toHaveAttribute('title', '/tmp/other-repo/docs/README.md');
});

test('開いている最中に新着が届いても、カーソルは同じファイルに留まる', async ({ page }) => {
  await openFolder(page);
  await pushSample(page);
  await page.keyboard.press('b');

  await page.keyboard.press('j');
  await expect(rows(page).nth(1).locator('.md-bell-name')).toHaveText('a.md');
  const picked = await rows(page).nth(1).getAttribute('data-path');

  // 新着は先頭に挿さるので、添字を据え置くとカーソルが 1 つ上の行へずれる。
  await push(page, [
    { path: id(page, 'long.md'), ago: 1 },
    { path: id(page, 'a.md'), ago: 120 },
    { path: id(page, 'sub/a.md'), ago: 4000 },
    { path: id(page, 'b.md'), ago: 90000, read: true },
  ]);
  await expect(page.locator('.md-bell-row.is-cursor')).toHaveAttribute('data-path', picked);

  // カーソルの行が一覧から消えたら先頭へ戻る（どこも指さないままにしない）。
  await push(page, [{ path: id(page, 'long.md'), ago: 1 }]);
  await expect(rows(page).nth(0)).toHaveClass(/is-cursor/);
});

test('空のまま開いていても、届いた瞬間に Enter で開ける', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('b');
  await expect(page.locator('.md-bell-empty')).toBeVisible();

  await push(page, [{ path: id(page, 'a.md'), ago: 5 }]);
  await page.keyboard.press('Enter');
  await expect(tab(page, 'a.md')).toHaveClass(/active/);
});

test('1 件も無いときは、その旨を出して空の枠を見せない', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('b');

  await expect(page.locator('.md-bell-empty')).toHaveText('まだ何も届いていません');
  await expect(rows(page)).toHaveCount(0);
  await expect(page.locator('.md-bell-readall')).toHaveCount(0);
});

test('開いている間、j は本文をスクロールさせない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'long.md' }).first().click();
  await expect(tab(page, 'long.md')).toHaveClass(/active/);
  await page.locator('#preview-pane').click();

  await page.keyboard.press('b');
  const before = await page.evaluate(() => document.getElementById('preview-pane').scrollTop);
  // j はカーソル移動に使われ、裏の本文は動かない。
  await page.keyboard.press('j');
  await page.keyboard.press('j');
  const after = await page.evaluate(() => document.getElementById('preview-pane').scrollTop);
  expect(after).toBe(before);
});

test('入力欄に書きかけがあるときは、開いていても素キーを奪わない', async ({ page }) => {
  await openFolder(page);
  await pushSample(page);
  await page.locator('.tree-item', { hasText: 'long.md' }).first().click();
  await expect(tab(page, 'long.md')).toHaveClass(/active/);

  // コメントの入力欄にフォーカスを置いたまま、マウスでベルを開く
  // （ベルは mousedown を preventDefault するのでフォーカスは入力欄に残る）。
  await page.locator('#preview-pane').click();
  await page.keyboard.press('c');
  await page.locator('#preview-pane p').first().click();
  const box = page.locator('.md-cmt-textarea');
  await expect(box).toBeFocused();

  await bell(page).click();
  await expect(panel(page)).toBeVisible();
  await page.keyboard.type('jkb ずんだ');
  await expect(box).toHaveValue('jkb ずんだ');
  await expect(panel(page)).toBeVisible();
});

test('窓が低いときも、一覧は画面の中に収まる', async ({ page }) => {
  await openFolder(page);
  await page.setViewportSize({ width: 900, height: 260 });
  await push(page, Array.from({ length: 12 }, (_, i) => ({ path: id(page, 'a.md') + i, ago: i * 60 })));
  await page.keyboard.press('b');

  const m = await page.evaluate(() => {
    const r = document.querySelector('.md-bell-panel').getBoundingClientRect();
    return { bottom: r.bottom, vh: window.innerHeight };
  });
  expect(m.bottom).toBeLessThanOrEqual(m.vh);
});

test('ベルを押して開き、もう一度押して閉じる', async ({ page }) => {
  await openFolder(page);
  await pushSample(page);

  await bell(page).click();
  await expect(panel(page)).toBeVisible();
  await bell(page).click();
  await expect(panel(page)).toHaveCount(0);
});
