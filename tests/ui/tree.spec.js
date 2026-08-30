// ファイルツリーとファイル移動の導線（folder.js / palette.js）。
//
// 「別のファイルを開く」入口（ツリー・[ ]・⌘P）と、サイドバーのフォーカス排他。
// 起動時に何が開いていても同じ導線が揃っていること（issue #12 でモードを 1 本に
// 畳んだので、cwd 外のファイルを 1 つ渡した起動でもツリーとタブが出る）もここ。
const { test, expect } = require('@playwright/test');
const { ONE_FILE_URL, open, openFolder } = require('./helpers');

test('⌘P でファイル検索が開く', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('Meta+p');
  const palette = page.locator('#md-pal-backdrop');
  await expect(palette).toBeVisible();
  // 一覧にフィクスチャのファイルが出ること（サーバの走査結果が届いている）。
  await expect(page.locator('.md-pal-row').first()).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(palette).toHaveCount(0);
});

test('cwd の外のファイルを 1 つ渡した起動でも、ツリー・タブ・⌘P が揃う', async ({ page }) => {
  // かつては「単一ファイルモード」（サイドバー無し・タブ無し・⌘P 無し）だった経路。
  // root はそのファイルの親フォルダになり、渡したファイルがタブ 1 枚で開く。
  await open(page, ONE_FILE_URL);

  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', 'a.md');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  // ツリーは親フォルダ（tests/ui-fixtures）の中身。渡したファイルが選択されている。
  await expect(page.locator('.tree-item[data-path="b.md"]')).toBeVisible();
  await expect(page.locator('.tree-item.active')).toHaveText(/a\.md/);

  // 別ファイルへ移る入口もある（] で次のファイル、⌘P でファイル検索）。
  await page.keyboard.press(']');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し B');
  await page.keyboard.press('Meta+p');
  await expect(page.locator('#md-pal-backdrop')).toBeVisible();
});

test('] で次のファイルへ移動する', async ({ page }) => {
  await openFolder(page);
  const bodyText = page.locator('#preview-pane .markdown-body');

  // 未選択の状態から ] を押すと、一覧の先頭（a.md）から入る。
  await page.keyboard.press(']');
  await expect(bodyText).toContainText('見出し A');
  await expect(page.locator('.tree-item.active')).toHaveText(/a\.md/);

  // もう一度で次のファイル（b.md）へ。
  await page.keyboard.press(']');
  await expect(bodyText).toContainText('見出し B');
  await expect(page.locator('.tree-item.active')).toHaveText(/b\.md/);

  // [ で戻る。
  await page.keyboard.press('[');
  await expect(bodyText).toContainText('見出し A');
});

test('Tab でツリーへ移り、j/k がツリー操作になる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md を開く（本文にフォーカスがある状態から始める）
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  await page.keyboard.press('Tab');
  await expect(page.locator('body')).toHaveClass(/nav-tree/);
  // ツリーにフォーカスがあるので j/k は本文スクロールではなくカーソル移動になる。
  const cursor = page.locator('.tree-item.cursor');
  await expect(cursor).toHaveCount(1);
  const first = await cursor.textContent();
  await page.keyboard.press('j');
  expect(await cursor.textContent()).not.toBe(first);

  // Tab で本文へ戻る。
  await page.keyboard.press('Tab');
  await expect(page.locator('body')).not.toHaveClass(/nav-tree/);
});

test('⌘B でファイルツリーを畳み、もう一度押すと元の幅で戻る', async ({ page }) => {
  await openFolder(page);
  const sidebar = page.locator('#sidebar');
  const width = async () => (await sidebar.boundingBox()).width;
  const before = await width();
  expect(before).toBeGreaterThan(0);

  await page.keyboard.press('Meta+b');
  await expect(page.locator('body')).toHaveClass(/sidebar-closed/);
  await expect.poll(width).toBe(0); // 0.15s の transition が終わるまで待つ

  await page.keyboard.press('Meta+b');
  await expect(page.locator('body')).not.toHaveClass(/sidebar-closed/);
  await expect.poll(width).toBe(before); // 幅は CSS 変数に残っているので元通り
});

test('ツリーにフォーカスがある状態で畳むと、本文へフォーカスが戻る', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('Tab');
  await expect(page.locator('body')).toHaveClass(/nav-tree/);

  // 見えないツリーにカーソルが居座ると j/k がツリー操作のままになるので、本文へ返す。
  await page.keyboard.press('Meta+b');
  await expect(page.locator('body')).toHaveClass(/sidebar-closed/);
  await expect(page.locator('body')).not.toHaveClass(/nav-tree/);

  // 畳んだ状態の Tab は「開いてからツリーへ」。
  await page.keyboard.press('Tab');
  await expect(page.locator('body')).not.toHaveClass(/sidebar-closed/);
  await expect(page.locator('body')).toHaveClass(/nav-tree/);
});
