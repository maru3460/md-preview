// UI テスト共通の足場。
//
// 各 spec が実物のページを開くまでの手順（オンボーディング抑止・初期描画待ち）は
// どこでも同じなので、ここ 1 か所に置く。サーバは playwright.config.js が
// 3 つ立てていて、URL の使い分けだけをここで持つ。
const { expect } = require('@playwright/test');

/// フォルダ起動（`md tests/ui-fixtures`。タブ 0 枚）。baseURL なので '/' で足りる。
const FOLDER_URL = '/';
/// 1 ファイル起動（`md <cwd 外の a.md>` 相当。root はその親で、タブが 1 枚）。
const ONE_FILE_URL = 'http://127.0.0.1:7879/';
/// 複数ファイル起動（`md a.md b.md` 相当。タブが 2 枚並んだ状態で始まる）。
const MULTI_URL = 'http://127.0.0.1:7880/';

/// 初回オンボーディング（? のヘルプ自動表示）を抑止して開く。
/// 出したままだと isOverlayOpen が true になり、素キーが全部止まる。
async function open(page, url) {
  await page.addInitScript(() => {
    try { localStorage.setItem('md-help-onboarded', '1'); } catch (e) {}
  });
  await page.goto(url || FOLDER_URL);
  // 初期描画の完了を待つ。印は folder.js の markInitialRenderDone が付ける。
  await page.waitForFunction(() => document.documentElement.dataset.mdReady === '1');
  // ツリー・タブが持つ識別子は絶対パスなので、root をここで拾って id() に渡す。
  // サーバごとに root が違う（フォルダ起動はフィクスチャ、ファイル起動は一時ディレクトリ）。
  page.mdRoot = await page.evaluate(() => window.MD_ROOT_DIR);
}

/// root 配下のファイル / ディレクトリの識別子。`rel` 省略で root 自身。
function id(page, rel) {
  return rel ? page.mdRoot + '/' + rel : page.mdRoot;
}

/// ツリーの行。`rel` は root 相対で書く（data-path は識別子なのでここで組む）。
function treeItem(page, rel) {
  return page.locator(`.tree-item[data-path="${id(page, rel)}"]`);
}

/// タブ。`rel` は root 相対。
function tab(page, rel) {
  return page.locator(`.md-tab[data-path="${id(page, rel)}"]`);
}

/// 識別子 → root 相対（root の外はそのまま）。data-path を読み出して比べるとき用。
function display(page, id) {
  if (!id) return id;
  return id.startsWith(page.mdRoot + '/') ? id.slice(page.mdRoot.length + 1) : id;
}

/// フォルダ起動で、ツリーが描かれるのを待つ。
async function openFolder(page) {
  await open(page, FOLDER_URL);
  await expect(page.locator('.tree-item').first()).toBeVisible();
}

/// rAF スロットルの更新（サイドバーのハイライト）が反映されるまで待つ。
async function nextFrames(page) {
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

module.exports = {
  FOLDER_URL, ONE_FILE_URL, MULTI_URL,
  open, openFolder, nextFrames,
  id, treeItem, tab, display,
};
