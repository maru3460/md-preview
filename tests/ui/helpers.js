// UI テスト共通の足場。
//
// 各 spec が実物のページを開くまでの手順（オンボーディング抑止・初期描画待ち）は
// どこでも同じなので、ここ 1 か所に置く。サーバは playwright.config.js が
// 3 つ立てていて、URL の使い分けだけをここで持つ。
const { expect } = require('@playwright/test');

/// フォルダモード（root = tests/ui-fixtures）。baseURL なので '/' で足りる。
const FOLDER_URL = '/';
/// 単一ファイルモード（cwd 外の a.md を 1 枚もので開いた状態）。
const SINGLE_URL = 'http://127.0.0.1:7879/';
/// 複数ファイル起動（`md a.md b.md` 相当。タブが 2 枚並んだ状態で始まる）。
const MULTI_URL = 'http://127.0.0.1:7880/';

/// 初回オンボーディング（? のヘルプ自動表示）を抑止して開く。
/// 出したままだと isOverlayOpen が true になり、素キーが全部止まる。
async function open(page, url) {
  await page.addInitScript(() => {
    try { localStorage.setItem('md-help-onboarded', '1'); } catch (e) {}
  });
  await page.goto(url || FOLDER_URL);
  // 初期描画の完了（init.js / folder.js が ready を投げたところ）を待つ。
  await page.waitForFunction(() => window.__mdIpc && window.__mdIpc.includes('ready'));
}

/// フォルダモードで、ツリーが描かれるのを待つ。
async function openFolder(page) {
  await open(page, FOLDER_URL);
  await expect(page.locator('.tree-item').first()).toBeVisible();
}

/// rAF スロットルの更新（サイドバーのハイライト）が反映されるまで待つ。
async function nextFrames(page) {
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

module.exports = { FOLDER_URL, SINGLE_URL, MULTI_URL, open, openFolder, nextFrames };
