// 検索バー（search.js）。本文の全文検索と、そのフォーカス排他。
//
// 見ているのは「開く / 閉じるの往復で本文とオーバーレイの主導権が正しく移るか」。
// ここが壊れると入力欄に打った文字が本文のキー操作になったり、閉じたのに
// ハイライトだけ残ったりする。
const { test, expect } = require('@playwright/test');
const { openFolder } = require('./helpers');

test('/ で検索バーが開き、Esc で閉じる', async ({ page }) => {
  await openFolder(page);
  const bar = page.locator('#md-search-bar');
  await expect(bar).toHaveClass(/hidden/);

  await page.keyboard.press('/');
  await expect(bar).not.toHaveClass(/hidden/);
  // 入力欄にフォーカスが移っていること（移らないと打った文字が本文のキー操作になる）。
  await expect(page.locator('.md-search-input')).toBeFocused();

  // 打った語が実際に本文でヒットすること。
  await page.locator('.md-search-input').fill('見出し');
  await expect(page.locator('.md-search-counter')).not.toHaveText('0/0');

  await page.keyboard.press('Escape');
  await expect(bar).toHaveClass(/hidden/);
  // 閉じたらハイライトも消えていること（色だけ残る壊れ方をしない）。
  await expect(page.locator('mark.md-search-hit')).toHaveCount(0);
});

test('⌘F は検索の入力欄にフォーカスがあっても閉じられる', async ({ page }) => {
  await openFolder(page);
  const bar = page.locator('#md-search-bar');

  await page.keyboard.press('Meta+f');
  await expect(bar).not.toHaveClass(/hidden/);
  await expect(page.locator('.md-search-input')).toBeFocused();
  await page.locator('.md-search-input').fill('見出し');

  // 入力欄にフォーカスがある状態からのトグル。ここで閉じられないと、
  // オーバーレイに閉じ込められて素キーが全部死ぬ。
  await page.keyboard.press('Meta+f');
  await expect(bar).toHaveClass(/hidden/);

  // 直前の語は消さないので、開き直すとそのまま検索を続けられる。
  await page.keyboard.press('Meta+f');
  await expect(bar).not.toHaveClass(/hidden/);
  await expect(page.locator('.md-search-input')).toHaveValue('見出し');
});
