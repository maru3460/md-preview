// 既存の窓へ転送されたファイルが画面に出るまで（#31）。
//
// ソケット・送り側の pid・前面化・Rust 側の待ち行列はここでは触れない。叩けるのは
// `window.MdOpenFiles` から先だけで、本物もソケット → Rust → evaluate_script で
// 同じ関数を呼ぶ。ソケットの層は tests/single_instance.rs と手動確認が持つ。
const { test, expect } = require('@playwright/test');
const { openFolder, treeItem, tab, display } = require('./helpers');

/// 転送の口を直接叩く。
const forward = (page, ids) => page.evaluate((v) => window.MdOpenFiles(v), ids);

/// root（tests/ui-fixtures）の外にあるフィクスチャの識別子。
const outsideId = (page) => page.mdRoot.replace(/ui-fixtures$/, 'ui-outside') + '/out.md';

/// ツリーからファイルを開く（tabs.spec.js と同じ入口）。
async function openFile(page, relPath) {
  await treeItem(page, relPath).click();
  await expect(tab(page, relPath)).toHaveClass(/active/);
}

/// タブバーに並んでいるファイル名（左から順）。
const tabNames = (page) => page.locator('.md-tab .md-tab-name');

/// いま active なタブのパス（root 相対）。
async function activePath(page) {
  return display(page, await page.locator('.md-tab.active').getAttribute('data-path'));
}

const body = (page) => page.locator('#preview-pane .markdown-body');
const paneScroll = (page) => page.evaluate(() => document.getElementById('preview-pane').scrollTop);

test('転送されたファイルは現在タブの右隣に挿さって表示される', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'b.md');
  await page.keyboard.press('Meta+1');
  await expect(tab(page, 'a.md')).toHaveClass(/active/);

  await forward(page, [page.mdRoot + '/long.md']);

  await expect(tabNames(page)).toHaveText(['a.md', 'long.md', 'b.md']);
  expect(await activePath(page)).toBe('long.md');
  await expect(body(page)).toContainText('長い見出し');
});

test('複数ファイルの転送は全部タブに載り、先頭だけが表示される', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'b.md');
  await page.keyboard.press('Meta+1');

  await forward(page, [page.mdRoot + '/long.md', page.mdRoot + '/notes.txt']);

  await expect(tabNames(page)).toHaveText(['a.md', 'long.md', 'notes.txt', 'b.md']);
  expect(await activePath(page)).toBe('long.md');
  await expect(body(page)).toContainText('長い見出し');

  // 2 枚目は載っただけ。開いた時に取りに行く（起動時の複数ファイルと同じ）。
  await page.keyboard.press('Meta+3');
  await expect(body(page)).toContainText('md ではないテキストファイル');
});

test('既に開いているファイルの転送はタブを増やさず、読み位置ごと戻る', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'long.md');
  await page.evaluate(() => { document.getElementById('preview-pane').scrollTop = 600; });
  await openFile(page, 'b.md');

  await forward(page, [page.mdRoot + '/long.md']);

  await expect(page.locator('.md-tab')).toHaveCount(2);
  expect(await activePath(page)).toBe('long.md');
  await expect.poll(() => paneScroll(page)).toBe(600);
});

test('いま見ているファイルの転送は、タブも読み位置も動かさない', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'long.md');
  await page.evaluate(() => { document.getElementById('preview-pane').scrollTop = 600; });

  // 本文を空にしてから転送する。expect.poll は「600 になる」を待つだけなので、
  // 最初から 600 のまま測ると、再フェッチが読み位置を落としても 1 サンプル目で
  // 通ってしまう（= 応答が遅い環境で黙って空振りに転ぶ）。
  await page.evaluate(() => { document.querySelector('#preview-pane .markdown-body').dataset.stale = '1'; });

  await forward(page, [page.mdRoot + '/long.md']);

  // 本文が差し替わったことを見てから読み位置を測る。
  await expect(page.locator('#preview-pane .markdown-body[data-stale]')).toHaveCount(0);
  await expect(tabNames(page)).toHaveText(['a.md', 'long.md']);
  expect(await activePath(page)).toBe('long.md');
  expect(await paneScroll(page)).toBe(600);
});

test('既に開いているファイルを含む転送でも、渡した並びのまま右隣にまとまる', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'b.md');
  await page.keyboard.press('Meta+1');
  await expect(tab(page, 'a.md')).toHaveClass(/active/);

  // b.md は既存。挿し先を b.md の右隣まで進めないと long.md が b.md の左へ
  // 取り残されて、表示中の b.md より前に新しいタブが居る形になる。
  await forward(page, [page.mdRoot + '/b.md', page.mdRoot + '/long.md']);

  await expect(tabNames(page)).toHaveText(['a.md', 'b.md', 'long.md']);
  expect(await activePath(page)).toBe('b.md');
});

test('root の外のファイルも転送でタブに乗り、監視を頼む', async ({ page }) => {
  await openFolder(page);
  const out = outsideId(page);

  await forward(page, [out]);

  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', out);
  await expect(body(page)).toContainText('root の外の見出し');
  // root の再帰監視に載らないので、個別に監視を頼まないとホットリロードだけが効かない。
  const watched = await page.evaluate(() => window.__mdIpc.filter((m) => m.startsWith('watch:')));
  expect(watched).toContain('watch:' + out);
});

test('⌘P を開いたまま転送されると、パレットは畳まれて本文だけが変わる', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await page.keyboard.press('Meta+p');
  await expect(page.locator('#md-pal-backdrop')).toBeVisible();

  await forward(page, [page.mdRoot + '/long.md']);

  await expect(page.locator('#md-pal-backdrop')).toHaveCount(0);
  await expect(body(page)).toContainText('長い見出し');
  // 掃いてから開く順序の固定。逆にすると focusPreview が入力欄に譲って、
  // 本文は変わったのに j/k が効かない窓になる。
  await expect
    .poll(() => page.evaluate(() => document.activeElement && document.activeElement.id))
    .toBe('preview-pane');
});

test('ヘルプを開いたまま転送されると畳まれる', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await page.keyboard.press('?');
  await expect(page.locator('#md-help-backdrop')).toBeVisible();

  await forward(page, [page.mdRoot + '/b.md']);

  await expect(page.locator('#md-help-backdrop')).toHaveCount(0);
  await expect(body(page)).toContainText('見出し B');
});

test('右クリックメニューを開いたまま転送されると畳まれる', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await tab(page, 'a.md').click({ button: 'right' });
  await expect(page.locator('#md-context-menu')).toBeVisible();

  await forward(page, [page.mdRoot + '/b.md']);

  await expect(page.locator('#md-context-menu')).toHaveCount(0);
  await expect(body(page)).toContainText('見出し B');
});

test('検索バーは転送で畳まれる（掃きと loadPreview の二重で閉じている）', async ({ page }) => {
  // これは closeOverlays を消しても通る。`loadPreview` が昔から `MdSearch.reset()` を
  // 呼んでいて、そちらでも閉じるため。掃きの回帰ではなく「転送後に検索バーが
  // 残らない」という見え方の回帰として置いてある——二重のどちらかを外したときに、
  // ここが残っていれば見え方は守られる。
  await openFolder(page);
  await openFile(page, 'a.md');
  await page.keyboard.press('Meta+f');
  await expect(page.locator('#md-search-bar')).not.toHaveClass(/hidden/);

  await forward(page, [page.mdRoot + '/b.md']);

  await expect(page.locator('#md-search-bar')).toHaveClass(/hidden/);
});

test('コメントモードは転送で解除されず、入力中のポップオーバーだけが閉じる', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  await page.locator('#preview-pane [data-src-line]').first().click();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('書きかけ');

  await forward(page, [page.mdRoot + '/b.md']);

  await expect(page.locator('#md-cmt-popover')).toHaveCount(0);
  // 画面を覆わない「モード」は残す。ツリークリックでも ⌘P でも解除されないのに
  // 転送だけ解除すると、入口ごとに挙動が割れる。
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  await expect(body(page)).toContainText('見出し B');
});
