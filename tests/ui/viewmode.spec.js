// 表示モード（viewmode.js）。raw（⌘R）と 差分（⌘D）の排他。
//
// 2 つが同時に active にならないこと、非 md ではそもそも raw を出さないことを見る。
const { test, expect } = require('@playwright/test');
const { openFolder } = require('./helpers');

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
