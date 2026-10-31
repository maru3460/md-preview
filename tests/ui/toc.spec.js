// TOC（⌘T のアウトライン）の開閉。
//
// TOC は md の通常表示で、⌘T / × で閉じられていない間だけ出る。見出しの数・窓幅・
// サイドバーの開閉には左右されない。md 以外の画面（html・ソース表示・⌘R・設定・
// 何も開いていない）では引っ込め、md へ戻れば元の開閉に戻る。
const { test, expect, openFolder, treeItem } = require('./helpers');

const panel = (page) => page.locator('.md-toc-panel');

async function openMd(page, rel, heading) {
  await treeItem(page, rel).click();
  await expect(page.locator('#preview-pane h1')).toContainText(heading);
}

async function openHtml(page) {
  await treeItem(page, 'page.html').click();
  await expect(page.frameLocator('iframe.html-frame').locator('h1')).toContainText('HTML フィクスチャ');
}

test('見出しが少ない md でも、開けば TOC が出ている', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'b.md', '見出し B');
  await expect(panel(page)).not.toHaveClass(/hidden/);
  await expect(page.locator('.md-toc-item')).toHaveCount(2);
});

test('⌘B でサイドバーを開閉しても、窓幅を変えても TOC は閉じない', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'b.md', '見出し B');
  await expect(panel(page)).not.toHaveClass(/hidden/);

  await page.keyboard.press('Meta+b');
  await expect(page.locator('body')).toHaveClass(/sidebar-closed/);
  await page.keyboard.press('Meta+b');
  await expect(page.locator('body')).not.toHaveClass(/sidebar-closed/);
  await page.setViewportSize({ width: 700, height: 720 });
  // 以前は 200ms 後の再評価や resize で閉じていた。それより長く待ってから見る。
  await page.waitForTimeout(400);
  await expect(panel(page)).not.toHaveClass(/hidden/);
});

test('html では引っ込み、md へ戻ると出る', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'b.md', '見出し B');
  await openHtml(page);
  await expect(panel(page)).toHaveClass(/hidden/);

  await openMd(page, 'a.md', '見出し A');
  await expect(panel(page)).not.toHaveClass(/hidden/);
  // 戻った文書の見出しで作り直されている。
  await expect(page.locator('.md-toc-item').first()).toContainText('見出し A');
});

test('⌘R のソース表示では引っ込み、戻すと出る', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'a.md', '見出し A');
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(panel(page)).toHaveClass(/hidden/);

  await page.keyboard.press('Meta+r');
  await expect(page.locator('#preview-pane h1')).toContainText('見出し A');
  await expect(panel(page)).not.toHaveClass(/hidden/);
});

test('md 以外のファイル（ソース表示）と、何も開いていない状態では出さない', async ({ page }) => {
  await openFolder(page);
  // フォルダ起動はタブ 0 枚で、本文ペインは空。
  await expect(panel(page)).toHaveClass(/hidden/);
  await treeItem(page, 'notes.txt').click();
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(panel(page)).toHaveClass(/hidden/);
});

test('× で閉じたら、ファイルを切り替えても html を挟んでも閉じたまま', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'a.md', '見出し A');
  await page.locator('.md-toc-close').click();
  await expect(panel(page)).toHaveClass(/hidden/);

  await openMd(page, 'b.md', '見出し B');
  await expect(panel(page)).toHaveClass(/hidden/);
  await openHtml(page);
  await openMd(page, 'a.md', '見出し A');
  await expect(panel(page)).toHaveClass(/hidden/);

  await page.keyboard.press('Meta+t');
  await expect(panel(page)).not.toHaveClass(/hidden/);
});

test('md 以外の画面での ⌘T は何もしない', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'a.md', '見出し A');
  // md 以外では TOC が常に引っ込んでいるので、⌘T が効くと「開く」側に倒れる。
  // それが見えるよう、先に × で閉じておく。
  await page.locator('.md-toc-close').click();
  await expect(panel(page)).toHaveClass(/hidden/);
  await openHtml(page);
  // フォーカスが iframe の中にあると ⌘T が親へ届く経路が変わるので、本文ペインで押す。
  await page.locator('#preview-pane').focus();
  await page.keyboard.press('Meta+t');
  await expect(panel(page)).toHaveClass(/hidden/);

  // 押したぶんが「戻ったら開いている」として残っていないこと。
  await openMd(page, 'a.md', '見出し A');
  await expect(panel(page)).toHaveClass(/hidden/);
});

test('md 以外の画面で ⌘T でコメントモードを抜けても、閉じた TOC は閉じたまま', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'a.md', '見出し A');
  await page.locator('.md-toc-close').click();
  await treeItem(page, 'notes.txt').click();
  await expect(page.locator('.source-view')).toBeVisible();

  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  await page.keyboard.press('Meta+t');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
  await expect(panel(page)).toHaveClass(/hidden/);

  await openMd(page, 'a.md', '見出し A');
  await expect(panel(page)).toHaveClass(/hidden/);
});

/// パネルの出し入れのたびに、そのときスライドを止めていたか（no-anim）を記録する。
async function recordAnim(page) {
  await page.evaluate(() => {
    window.__tocAnim = [];
    const el = document.querySelector('.md-toc-panel');
    let wasHidden = el.classList.contains('hidden');
    new MutationObserver(() => {
      const hidden = el.classList.contains('hidden');
      if (hidden === wasHidden) return;
      wasHidden = hidden;
      window.__tocAnim.push((hidden ? 'hide' : 'show') + (el.classList.contains('no-anim') ? ':instant' : ':slide'));
    }).observe(el, { attributes: true, attributeFilter: ['class'] });
  });
}

test('画面の種類で出入りするときは滑らせず、⌘T / × のときだけ滑らせる', async ({ page }) => {
  await openFolder(page);
  await openMd(page, 'a.md', '見出し A');
  await recordAnim(page);

  await openHtml(page);
  await openMd(page, 'b.md', '見出し B');
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.keyboard.press('Meta+r');
  await expect(page.locator('#preview-pane h1')).toContainText('見出し B');
  await page.locator('.md-toc-close').click();
  await page.keyboard.press('Meta+t');
  await expect(panel(page)).not.toHaveClass(/hidden/);

  expect(await page.evaluate(() => window.__tocAnim)).toEqual([
    'hide:instant', 'show:instant', // html へ行って戻る
    'hide:instant', 'show:instant', // ⌘R で入って抜ける
    'hide:slide', 'show:slide',     // × と ⌘T
  ]);
});
