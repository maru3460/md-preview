// 相対パスの基準と、root の外を開く導線（issue #10 / #11 の回帰）。
//
// ここはブラウザでしか捕まらない。ページの URL は常に root なので、
// 本文に相対 src が残っていると、ブラウザが root 基準で解決して 404 になる——
// サーバ側の HTML を見るだけでは「相対のまま」が壊れていると分からない。
// なので実際に画像が読めたか（naturalWidth）まで確かめる。
const { test, expect } = require('@playwright/test');
const { id, openFolder, treeItem, tab } = require('./helpers');

/// 折り畳まれている sub/ を開いて、子の行が出るまで待つ。
async function openSubA(page) {
  await treeItem(page, 'sub').click();
  await treeItem(page, 'sub/a.md').click();
  await expect(tab(page, 'sub/a.md')).toHaveClass(/active/);
}

/// 本文の n 枚目の画像が実際に読めたか（読めていなければ naturalWidth は 0）。
function imageLoaded(page, index) {
  return page.locator('#preview-pane .markdown-body img').nth(index).evaluate(
    (img) => (img.complete ? img.naturalWidth : new Promise((r) => {
      img.addEventListener('load', () => r(img.naturalWidth));
      img.addEventListener('error', () => r(0));
    }))
  );
}

test('サブフォルダの md の相対画像は、root ではなくその md の場所を基準に引く', async ({ page }) => {
  await openFolder(page);
  await openSubA(page);

  const img = page.locator('#preview-pane .markdown-body img').first();
  // src は sub/ 基準に畳まれている（root 直下の /fig.svg ではない）。
  await expect(img).toHaveAttribute('src', '/sub/fig.svg');
  expect(await imageLoaded(page, 0)).toBeGreaterThan(0);
});

test('root の外を指すリンクを踏むと、絶対パスのタブとして開ける', async ({ page }) => {
  await openFolder(page);
  await openSubA(page);

  await page.locator('#preview-pane .markdown-body a', { hasText: 'root の外へ' }).click();
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('root の外の見出し');

  // 識別子は絶対パス。root の外なので root を接頭辞に持たない。
  const path = await page.locator('.md-tab.active').getAttribute('data-path');
  expect(path.startsWith('/')).toBe(true);
  expect(path.startsWith(page.mdRoot + '/')).toBe(false);
  expect(path).toMatch(/\/tests\/ui-outside\/out\.md$/);

  // root の外のファイルの、さらに相対画像も引ける（/__abs/ 経由）。
  const src = await page.locator('#preview-pane .markdown-body img').first().getAttribute('src');
  expect(src).toMatch(/^\/__abs\/.*\/tests\/ui-outside\/fig\.svg$/);
  expect(await imageLoaded(page, 0)).toBeGreaterThan(0);

  // ホットリロードのために、開いたファイルを個別監視へ足すよう頼んでいること。
  const ipc = await page.evaluate(() => window.__mdIpc || []);
  expect(ipc.some((m) => m.startsWith('watch:') && m.endsWith('/tests/ui-outside/out.md'))).toBe(true);
});

test('iframe の中の相対リンクは、その html の場所を基準に解決する', async ({ page }) => {
  await openFolder(page);
  await treeItem(page, 'sub').click();
  await treeItem(page, 'sub/page.html').click();
  await expect(tab(page, 'sub/page.html')).toHaveClass(/active/);

  // iframe の中身はサーバが書き換えていないので、解決するのは JS 側（MdCommon.resolvePath）。
  // `../a.md` を root で黙って止めず、sub/ の親＝root 直下の a.md を開く。
  await page.frameLocator('iframe.html-frame').locator('a', { hasText: '上の a.md へ' }).click();
  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', id(page, 'a.md'));
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
});

// `md /` は塞がれていない（`app_config::plan_paths` は単一のディレクトリ引数を
// `files_root` の `/` 拒否より前に返す）。root が `/` のとき、素朴に `root + '/'` で
// 前方一致を見ると `'//'` になって root 配下が 1 つも一致せず、ツリーの祖先展開が
// 止まり、⌘P の全行にホームパスが並び、「相対パスをコピー」が絶対パスの複製になる。
// サーバを `/` で立てるのは現実的でないので、ヘルパを直に呼んで押さえる。
test('root が / でも、その配下は root の中だと判定される', async ({ page }) => {
  await openFolder(page);
  const r = await page.evaluate(() => {
    const saved = window.MD_ROOT_DIR;
    try {
      window.MD_ROOT_DIR = '/';
      return {
        outside: MdCommon.isOutsideRoot('/Applications/x.md'),
        display: MdCommon.idToDisplay('/Applications/x.md'),
        rootItself: MdCommon.idToDisplay('/'),
      };
    } finally {
      window.MD_ROOT_DIR = saved;
    }
  });
  expect(r.outside).toBe(false);
  expect(r.display).toBe('Applications/x.md');
  // root 自身は剥ぐと名前が消えるので、識別子のまま返す（Rust の display_id と同じ）。
  expect(r.rootItself).toBe('/');
});

test('root 自身と、名前が root で始まるだけのパスを取り違えない', async ({ page }) => {
  await openFolder(page);
  const r = await page.evaluate(() => {
    const root = window.MD_ROOT_DIR;
    return {
      rootItself: MdCommon.idToDisplay(root),
      lookalike: MdCommon.idToDisplay(root + '-backup/x.md'),
      inside: MdCommon.idToDisplay(root + '/a.md'),
    };
  });
  expect(r.rootItself).toBe(await page.evaluate(() => window.MD_ROOT_DIR));
  expect(r.lookalike).toMatch(/-backup\/x\.md$/);
  expect(r.inside).toBe('a.md');
});
