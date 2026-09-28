// Quick Access（#35）。サイドバー下部に留めた行から、フォルダなら root を動かし、
// ファイルならタブで開く。
//
// 台帳の永続化そのものは Rust（`quick_access.rs` のユニットテスト）が持つ。ここが
// 見るのは**ページ側の並びと、飛ぶ IPC の文言**まで——`examples/serve.rs` の
// スタブは `window.__mdIpc` に積むだけで、ディスクへは書かない。
// つまりページを開き直すたびに Quick Access は空に戻る（`MD_QUICK_ACCESS` が
// 常に `[]` で焼かれる）ので、テスト間の後片付けは要らない。
const { test, expect } = require('@playwright/test');
const { ROOT_URL, open, openFolder, treeItem, id } = require('./helpers');

/// Quick Access の行。ツリーの行と同じ `.tree-item` を着ているので、**必ず枠で絞る**。
function quickRows(page) {
  return page.locator('#quick-access-list .tree-item');
}

/// 送られた IPC の一覧（スタブが積んだもの）。
function sentIpc(page) {
  return page.evaluate(() => window.__mdIpc.slice());
}

/// ツリーへフォーカスを移してカーソルを先頭行へ置く（root.spec.js と同じ手順）。
async function focusTree(page) {
  const inTree = () => page.evaluate(() => window.MdCommon.isSidebarFocused());
  if (!(await inTree())) await page.keyboard.press('Tab');
  await expect.poll(inTree).toBe(true);
  await page.keyboard.press('g');
  await expect(page.locator('.tree-item.cursor')).toHaveCount(1);
}

test('空でも枠は出る。★ でいまのフォルダが 1 行になり、もう一度押すと外れる', async ({ page }) => {
  await openFolder(page);

  await expect(page.locator('#quick-access-title')).toBeVisible();
  await expect(page.locator('.qa-empty')).toBeVisible();
  await expect(quickRows(page)).toHaveCount(0);

  await page.locator('#root-star').click();

  await expect(quickRows(page)).toHaveCount(1);
  // フォルダは末尾の `/` で示す（ツリーの `›` は開閉の合図なので流用しない）。
  await expect(quickRows(page).first()).toContainText('ui-fixtures/');
  await expect(page.locator('.qa-empty')).toHaveCount(0);
  expect(await sentIpc(page)).toContain('quick:add:' + id(page));

  await page.locator('#root-star').click();

  await expect(quickRows(page)).toHaveCount(0);
  await expect(page.locator('.qa-empty')).toBeVisible();
  expect(await sentIpc(page)).toContain('quick:remove:' + id(page));
});

test('★ は登録済みかどうかを見せる', async ({ page }) => {
  await openFolder(page);

  const star = page.locator('#root-star');
  await expect(star).toHaveText('☆');

  await star.click();
  await expect(star).toHaveText('★');
  await expect(star).toHaveClass(/on/);

  await star.click();
  await expect(star).toHaveText('☆');
});

test('`m` はツリーのカーソル行を留めて、もう一度押すと外す', async ({ page }) => {
  await openFolder(page);
  await focusTree(page);

  // 先頭行は `no-md`（フォルダが先に並ぶ）。カーソル行そのものを対象にする。
  const cursorPath = await page.evaluate(
    () => document.querySelector('.tree-item.cursor').dataset.path
  );
  await page.keyboard.press('m');

  await expect(quickRows(page)).toHaveCount(1);
  await expect(quickRows(page).first()).toHaveAttribute('data-path', cursorPath);
  expect(await sentIpc(page)).toContain('quick:add:' + cursorPath);

  await page.keyboard.press('m');
  await expect(quickRows(page)).toHaveCount(0);
  expect(await sentIpc(page)).toContain('quick:remove:' + cursorPath);
});

test('留めたファイルの行を押すとタブで開く', async ({ page }) => {
  await openFolder(page);

  await treeItem(page, 'a.md').click({ button: 'right' });
  await page.locator('.md-context-menu-item', { hasText: 'Quick Access に追加' }).click();
  await expect(quickRows(page)).toHaveCount(1);

  // 一度別のファイルへ移ってから、Quick Access の行で戻る。
  await treeItem(page, 'b.md').click();
  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', id(page, 'b.md'));

  await quickRows(page).first().click();
  await expect(page.locator('.md-tab.active')).toHaveAttribute('data-path', id(page, 'a.md'));
});

test('留めたフォルダの行を押すと root がそこへ動く', async ({ page, request }) => {
  // root が動く ＝ **サーバの応答が変わる**ので、専用のサーバ（ROOT_URL）を使う。
  // 他の spec と同じ木を共有すると、後続が起動時に焼かれた MD_ROOT_DIR と
  // 食い違った木を引く。前後の両方で戻すのは root.spec.js と同じ理由。
  const fixtureRoot = require('fs').realpathSync(
    require('path').resolve(__dirname, '../ui-fixtures')
  );
  const reset = () =>
    request.get(ROOT_URL + '__setroot?p=' + encodeURIComponent(fixtureRoot));
  await reset();
  try {
    await open(page, ROOT_URL);
    await expect(page.locator('.tree-item').first()).toBeVisible();

    await treeItem(page, 'sub').click({ button: 'right' });
    await page.locator('.md-context-menu-item', { hasText: 'Quick Access に追加' }).click();
    await expect(quickRows(page)).toHaveCount(1);

    // 親へ上げてから、留めた行で戻る（ただ押しただけで動いたように見えないように）。
    await page.locator('#root-name').click();
    await expect(page.locator('#root-name')).toHaveText('tests');

    await quickRows(page).first().click();
    await expect(page.locator('#root-name')).toHaveText('sub');
  } finally {
    await reset();
  }
});

test('root の外のフォルダも留めて開ける', async ({ page, request }) => {
  // #35 の本題。**留めた先は root の外にある**のが普通で（別のリポジトリ）、
  // ツリーから辿れる範囲にしか飛べないなら Quick Access を置く意味が無い。
  const fixtureRoot = require('fs').realpathSync(
    require('path').resolve(__dirname, '../ui-fixtures')
  );
  const outside = require('fs').realpathSync(
    require('path').resolve(__dirname, '../ui-outside')
  );
  const reset = () =>
    request.get(ROOT_URL + '__setroot?p=' + encodeURIComponent(fixtureRoot));
  await reset();
  try {
    await open(page, ROOT_URL);
    await expect(page.locator('.tree-item').first()).toBeVisible();

    // いまの root の**外**を留める（右クリックからは届かない場所なので、
    // メニューと同じ口を直に叩く）。
    await page.evaluate((p) => window.MdQuick.toggle(p, true), outside);
    await expect(quickRows(page)).toHaveCount(1);

    await quickRows(page).first().click();

    await expect(page.locator('#root-name')).toHaveText('ui-outside');
    await expect.poll(() => page.evaluate(() => window.MD_ROOT_DIR)).toBe(outside);
  } finally {
    await reset();
  }
});

test('root が動くと ★ は新しいフォルダの登録状態を見せる', async ({ page, request }) => {
  const fixtureRoot = require('fs').realpathSync(
    require('path').resolve(__dirname, '../ui-fixtures')
  );
  const reset = () =>
    request.get(ROOT_URL + '__setroot?p=' + encodeURIComponent(fixtureRoot));
  await reset();
  try {
    await open(page, ROOT_URL);
    await page.locator('#root-star').click();
    await expect(page.locator('#root-star')).toHaveText('★');

    // 親へ上がると、留めたのは「1 つ下」なので ☆ に戻る。
    await page.locator('#root-name').click();
    await expect(page.locator('#root-name')).toHaveText('tests');
    await expect(page.locator('#root-star')).toHaveText('☆');

    // 戻れば ★。
    await page.locator('#root-back').click();
    await expect(page.locator('#root-name')).toHaveText('ui-fixtures');
    await expect(page.locator('#root-star')).toHaveText('★');
  } finally {
    await reset();
  }
});

test('`G` は木の末尾で止まり、`[` / `]` は枠へ入らない', async ({ page }) => {
  // 枠の行はツリーと同じ `.tree-item` を着ているので、絞り忘れるとどちらも
  // 枠を巻き込む。巻き込んでも**症状が出にくい**ぶん、ここで釘を打っておく。
  await openFolder(page);
  await page.locator('#root-star').click();
  await expect(quickRows(page)).toHaveCount(1);

  await focusTree(page);
  await page.keyboard.press('G');
  await expect(page.locator('#sidebar .tree-item.cursor')).toHaveCount(1);
  await expect(quickRows(page).first()).not.toHaveClass(/cursor/);

  // 最後のファイルを開いてから `]`。枠の行へ進んではいけない（端で無反応）。
  await treeItem(page, 'zz-mermaid.md').click();
  await expect(page.locator('.md-tab.active')).toHaveAttribute(
    'data-path', id(page, 'zz-mermaid.md'));
  await page.keyboard.press(']');
  await expect(page.locator('.md-tab.active')).toHaveAttribute(
    'data-path', id(page, 'zz-mermaid.md'));
});

test('ツリーから j でカーソルが Quick Access の枠へ降り、`h` は枠の中で効かない', async ({ page }) => {
  await openFolder(page);
  await page.locator('#root-star').click();
  await expect(quickRows(page)).toHaveCount(1);

  await focusTree(page);
  await page.keyboard.press('G'); // ツリーの末尾へ

  // 木の最後の行の次の `j` で枠へ入る。
  await page.keyboard.press('j');
  await expect(quickRows(page).first()).toHaveClass(/cursor/);

  // `h` はツリーの天井で root を上げるキー。枠の中では何も起こしてはいけない。
  await page.keyboard.press('h');
  await expect(page.locator('#root-name')).toHaveText('ui-fixtures');
  await expect(quickRows(page).first()).toHaveClass(/cursor/);

  // 戻り道は `k`。
  await page.keyboard.press('k');
  await expect(quickRows(page).first()).not.toHaveClass(/cursor/);
  await expect(page.locator('#sidebar .tree-item.cursor')).toHaveCount(1);
});

test('↻ は枠に置いたカーソルを動かさない', async ({ page }) => {
  // ↻ が作り直すのは木だけ。枠の行は生きたままなので、カーソルも残す。
  await openFolder(page);
  await page.locator('#root-star').click();
  await focusTree(page);
  await page.keyboard.press('G');
  await page.keyboard.press('j');
  await expect(quickRows(page).first()).toHaveClass(/cursor/);

  await page.locator('#tree-reload').click();
  await expect(page.locator('#sidebar .tree-item').first()).toBeVisible();

  await expect(quickRows(page).first()).toHaveClass(/cursor/);
  await expect(page.locator('#sidebar .tree-item.cursor')).toHaveCount(0);
});

test('消えたフォルダはクリックして初めて気づく', async ({ page }) => {
  // 起動時に存在確認はしない（台帳のぶんだけ走査が要る）ので、消えた行はそのまま
  // 並ぶ。押した時に理由が出ることだけを保証する。
  //
  // 仕込みは右クリックメニューと同じ口（`MdQuick.toggle`）から。`MD_QUICK_ACCESS` を
  // 先に差しても、起動スクリプトが `[]` で塗り直すので効かない。
  //
  // 知らせるのは**受け側**（`MdRootFailed`）。ページが先に `?dir=` で確かめる形には
  // できない——あの門は root の中しか答えないので、root の外を留める Quick Access は
  // 全部「無い」ことになる（それがこの spec の 1 つ上のテストが守っている性質）。
  await openFolder(page);
  await page.evaluate(() => window.MdQuick.toggle('/no/such/folder', true));

  await expect(quickRows(page)).toHaveCount(1);
  await quickRows(page).first().click();

  await expect(page.locator('.md-toast.show')).toContainText('フォルダを開けませんでした');
  // 行は残す。外すかどうかは人が決める。
  await expect(quickRows(page)).toHaveCount(1);
});

test('右クリックのラベルは登録済みかどうかで入れ替わる', async ({ page }) => {
  await openFolder(page);

  await treeItem(page, 'a.md').click({ button: 'right' });
  await page.locator('.md-context-menu-item', { hasText: 'Quick Access に追加' }).click();
  await expect(quickRows(page)).toHaveCount(1);

  await quickRows(page).first().click({ button: 'right' });
  await page.locator('.md-context-menu-item', { hasText: 'Quick Access から外す' }).click();
  await expect(quickRows(page)).toHaveCount(0);
});
