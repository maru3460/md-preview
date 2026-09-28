// root（ツリーの頂点）の移動（#34）。
//
// 窓では root を動かすのが Rust（監視の張り替えと窓のタイトルがある）なので、
// ここでは `examples/serve.rs` の IPC スタブが `/__setroot` へ回してから
// `MdSetRoot` を呼ぶ。順序は窓側と同じ——サーバの root を先に差し替えてから
// ページへ知らせる。ページから先に知らせると、新しい root で古いツリーを引く。
const { test, expect } = require('@playwright/test');
const { ROOT_URL, open, treeItem, id } = require('./helpers');

// root が動くと**サーバの応答も変わる**ので、このファイルは専用のサーバ
// （playwright.config.js の ROOT_PORT）を使う。そのうえで毎回戻す——戻さないと
// 次のテストがページの `MD_ROOT_DIR`（起動時に焼き込まれる）と食い違った木を引く。
// サーバを立てた root。ページの `MD_ROOT_DIR` から拾うと「焼き込みなので動かない」
// ことに寄りかかることになるので、実体から取る（サーバは `tests/ui-fixtures` で立つ）。
const fixtureRoot = require('fs').realpathSync(
  require('path').resolve(__dirname, '../ui-fixtures')
);

async function openRoot(page) {
  await open(page, ROOT_URL);
  await expect(page.locator('.tree-item').first()).toBeVisible();
}

async function resetRoot(request) {
  await request.get(ROOT_URL + '__setroot?p=' + encodeURIComponent(fixtureRoot));
}

// 前後の両方で戻す。`reuseExistingServer` はローカルだと真なので、afterEach を
// 取りこぼした回（テストごと落ちた等）の汚れた root が、**次の実行にまで**
// 持ち越される（`MD_ROOT_DIR` は起動時固定なので、ページとサーバが食い違ったまま
// 残る）。前で戻しておけば、持ち越しはそのテスト 1 本で止まる。
test.beforeEach(async ({ request }) => { await resetRoot(request); });
test.afterEach(async ({ request }) => { await resetRoot(request); });

/// root が `rel`（起動時の root からの相対。'..' で親）へ移り終わるのを待つ。
/// 名前だけでなく `MD_ROOT_DIR` も見るのは、ヘッダだけ書き換えて中身が
/// 付いてこない実装でも通ってしまうため。
async function expectRoot(page, name) {
  await expect(page.locator('#root-name')).toHaveText(name);
  await expect
    .poll(() => page.evaluate(() => window.MD_ROOT_DIR.split('/').pop()))
    .toBe(name);
}

/// ツリーにフォーカスを移し、カーソルを先頭行へ置く。
/// ツリーの行をクリックした後は既にサイドバーへフォーカスが入っている
/// （`#sidebar` が tabindex=-1 なので、WebKit が祖先を focus する）。
/// そこで Tab を押すと本文側へ戻ってしまうので、入っていない時だけ押す。
async function focusTree(page) {
  const inTree = () => page.evaluate(() => window.MdCommon.isSidebarFocused());
  if (!(await inTree())) await page.keyboard.press('Tab');
  await expect.poll(inTree).toBe(true);
  await page.keyboard.press('g');
  await expect(page.locator('.tree-item.cursor')).toHaveCount(1);
}

test('ヘッダにいまのフォルダ名が出て、クリックすると親フォルダへ上がる', async ({ page }) => {
  await openRoot(page);
  await expectRoot(page, 'ui-fixtures');

  await page.locator('#root-name').click();

  await expectRoot(page, 'tests');
  // 木が入れ替わっている。親には ui-fixtures が 1 行として並ぶ。
  await expect(page.locator('.tree-item', { hasText: 'ui-fixtures' }).first()).toBeVisible();
});

test('`h` はツリーの天井で root を親へ上げる', async ({ page }) => {
  await openRoot(page);
  await focusTree(page);

  // 天井（トップレベルの行）で h。畳む相手も親の行も無いので root が動く。
  await page.keyboard.press('h');

  await expectRoot(page, 'tests');
});

test('`h` の押しっぱなしは天井を越えない（指を離して押し直せば上がる）', async ({ page }) => {
  await openRoot(page);
  // sub を開いてその中のファイルを開く。Tab で戻ると、カーソルは開いている
  // ファイルの行（sub の中）に置かれる。ここから h を押しっぱなしにすると
  // 「子 → 親の行 → 畳む → 天井」と進む。
  await treeItem(page, 'sub').click();
  await expect(treeItem(page, 'sub/a.md')).toBeVisible();
  await treeItem(page, 'sub/a.md').click();
  await page.keyboard.press('Tab');
  await expect(page.locator('.tree-item.cursor')).toHaveAttribute('data-path', id(page, 'sub/a.md'));

  // 2 回目以降の down は autoRepeat（e.repeat === true）で届く。
  await page.evaluate(() => { window.__mdIpc.length = 0; });
  await page.keyboard.down('h');
  await page.keyboard.down('h');
  await page.keyboard.down('h');
  await page.keyboard.down('h');
  await page.keyboard.up('h');

  // 「まだ ui-fixtures のまま」を poll で測ると、IPC の往復より先にサンプルした
  // だけでも通ってしまう。**要求そのものが出ていない**ことを見る。
  const rootRequests = () =>
    page.evaluate(() => window.__mdIpc.filter((m) => m.indexOf('root:') === 0).length);
  expect(await rootRequests()).toBe(0);
  await expectRoot(page, 'ui-fixtures');

  // 離して押し直した 1 回だけが越える。
  await page.keyboard.press('h');
  await expectRoot(page, 'tests');
  expect(await rootRequests()).toBe(1);
});

test('右クリックの「ここを root にする」で潜る', async ({ page }) => {
  await openRoot(page);
  await treeItem(page, 'sub').click({ button: 'right' });

  const item = page.locator('#md-context-menu .md-context-menu-item', { hasText: 'ここを root にする' });
  await expect(item).toBeVisible();
  await item.click();

  await expectRoot(page, 'sub');
  // 潜った先の中身だけが並ぶ。
  await expect(page.locator('.tree-item', { hasText: 'a.md' }).first()).toBeVisible();
  await expect(page.locator('.tree-item', { hasText: 'zz-code.md' })).toHaveCount(0);
});

test('ファイルを右クリックしても「ここを root にする」は出ない', async ({ page }) => {
  await openRoot(page);
  await treeItem(page, 'a.md').click({ button: 'right' });

  await expect(page.locator('#md-context-menu')).toBeVisible();
  await expect(
    page.locator('#md-context-menu .md-context-menu-item', { hasText: 'ここを root にする' })
  ).toHaveCount(0);
});

test('いま居る root（ヘッダ）には「ここを root にする」を出さない', async ({ page }) => {
  await openRoot(page);
  await page.locator('#sidebar-header').click({ button: 'right', position: { x: 4, y: 4 } });

  await expect(page.locator('#md-context-menu')).toBeVisible();
  // 押しても何も起きない行は、灰色でも並べない。
  await expect(
    page.locator('#md-context-menu .md-context-menu-item', { hasText: 'ここを root にする' })
  ).toHaveCount(0);

  // ヘッダが対象になっていること自体は、**押して確かめる**。行が 1 個あることを
  // 数えるだけだと、対象がプレビュー中のファイルへ落ちていても通ってしまう。
  await page.evaluate(() => { window.__mdIpc.length = 0; });
  await page.locator('#md-context-menu .md-context-menu-item', { hasText: '絶対パスをコピー' }).click();
  expect(await page.evaluate(() => window.__mdIpc.filter((m) => m.indexOf('menu:abs:') === 0)))
    .toEqual(['menu:abs:' + fixtureRoot]);
});

test('root を潜ると、表示していないタブの監視も頼み直す', async ({ page }) => {
  // 張り替えで個別監視は消えるので、ページが送り直さないと root の外になった
  // タブだけホットリロードが黙って死ぬ。
  //
  // **表示していないタブで測る。** 表示中のファイルは、この後に走る
  // `loadPreview(…, true)` が自分で `watch:` を頼むので、そちらだけ見ると
  // 送り直しのループを消しても通ってしまう（実際に一度そう書いて空振りした）。
  await openRoot(page);
  await treeItem(page, 'a.md').click();
  await treeItem(page, 'b.md').click();
  await expect(page.locator('.md-tab')).toHaveCount(2);
  await expect(page.locator('.md-tab.active')).toHaveAttribute('title', 'b.md');

  await page.evaluate(() => { window.__mdIpc.length = 0; });
  await page.evaluate((dir) => window.MdRoot.set(dir), id(page, 'sub'));
  await expectRoot(page, 'sub');

  // 重複は畳む（表示中のぶんは 2 回飛ぶ。Rust 側は重ねて watch しても畳む）。
  await expect
    .poll(() => page.evaluate(() =>
      Array.from(new Set(window.__mdIpc.filter((m) => m.indexOf('watch:') === 0))).sort()))
    .toEqual(['watch:' + fixtureRoot + '/a.md', 'watch:' + fixtureRoot + '/b.md']);
});

test('⌘P を開いたまま root が動いたら、一覧も追従する', async ({ page }) => {
  // 一覧は root 配下を集めたもの。捨てるだけだと DOM は古い root のファイルを
  // 並べたまま残り、Enter で開けてしまう（`md <dir>` の転送で踏める）。
  await openRoot(page);
  await page.keyboard.press('Meta+p');
  await expect(page.locator('.md-pal-row').first()).toBeVisible();
  await expect(page.locator('.md-pal-row', { hasText: 'zz-code.md' })).toHaveCount(1);

  // 転送と同じ経路（ページからの root:）で、パレットを開いたまま潜る。
  await page.evaluate((dir) => window.MdRoot.set(dir), id(page, 'sub'));
  await expectRoot(page, 'sub');

  // sub の中に zz-code.md は無い。残っていたら古い root の一覧なのだ。
  await expect(page.locator('#md-pal-backdrop')).toBeVisible();
  await expect(page.locator('.md-pal-row', { hasText: 'zz-code.md' })).toHaveCount(0);
  await expect(page.locator('.md-pal-row', { hasText: 'a.md' }).first()).toBeVisible();
});

test('右クリックメニューに「再読み込み」は無い', async ({ page }) => {
  // 本文の追従は watcher、ツリーはヘッダの ↻ が担うので、この項目に残る仕事が無い。
  await openRoot(page);
  await treeItem(page, 'a.md').click({ button: 'right' });

  await expect(page.locator('#md-context-menu')).toBeVisible();
  await expect(
    page.locator('#md-context-menu .md-context-menu-item', { hasText: '再読み込み' })
  ).toHaveCount(0);
});

test('⌘[ / ⌘] でフォルダの履歴を往復する', async ({ page }) => {
  await openRoot(page);
  await page.locator('#root-name').click();
  await expectRoot(page, 'tests');

  await page.keyboard.press('Meta+[');
  await expectRoot(page, 'ui-fixtures');

  await page.keyboard.press('Meta+]');
  await expectRoot(page, 'tests');
});

test('履歴の端ではボタンが無効になる', async ({ page }) => {
  await openRoot(page);
  await expect(page.locator('#root-back')).toBeDisabled();
  await expect(page.locator('#root-forward')).toBeDisabled();

  await page.locator('#root-name').click();
  await expectRoot(page, 'tests');

  await expect(page.locator('#root-back')).toBeEnabled();
  await expect(page.locator('#root-forward')).toBeDisabled();
});

test('root を変えてもタブは残り、名前が root 相対で付け替わる', async ({ page }) => {
  await openRoot(page);
  await treeItem(page, 'a.md').click();
  await expect(page.locator('.md-tab')).toHaveCount(1);
  const path = id(page, 'a.md');

  await page.locator('#root-name').click();
  await expectRoot(page, 'tests');

  // タブは識別子（絶対パス）で持っているので、root が動いても消えない。
  await expect(page.locator(`.md-tab[data-path="${path}"]`)).toHaveCount(1);
  // 名前は root を剥いだ形なので、親へ上がると 1 段深くなる。
  await expect(page.locator('.md-tab')).toHaveAttribute('title', 'ui-fixtures/a.md');
});

test('root が動いたら本文を出し直す（相対リンクが別のファイルを指さない）', async ({ page }) => {
  await openRoot(page);
  await treeItem(page, 'sub').click();
  await treeItem(page, 'sub/a.md').click();
  // sub/a.md の `![](fig.svg)` は root 配下なので root 相対の URL で出る。
  const img = page.locator('#preview-pane img').first();
  await expect(img).toHaveAttribute('src', '/sub/fig.svg');

  await page.locator('#root-name').click();
  await expectRoot(page, 'tests');

  // 出し直さないと、この URL が新しい root（tests）から解決されて 404 になる。
  // 画像は取得済みなので見た目は変わらず、壊れるのはリンクを踏んだ時だけ——
  // だからここで URL そのものを見る。
  await expect(img).toHaveAttribute('src', '/ui-fixtures/sub/fig.svg');
  // 開いているファイルは変えない。
  await expect(page.locator('.md-tab.active')).toHaveAttribute('title', 'ui-fixtures/sub/a.md');
});

test('↻ はツリーだけを作り直し、展開状態とカーソルを戻す', async ({ page }) => {
  await openRoot(page);
  await treeItem(page, 'sub').click();
  await expect(treeItem(page, 'sub/a.md')).toBeVisible();
  await focusTree(page);
  await page.keyboard.press('j');
  const cursorPath = await page.locator('.tree-item.cursor').getAttribute('data-path');

  await page.locator('#tree-reload').click();

  // 開いていたフォルダは開いたまま戻る。
  await expect(treeItem(page, 'sub/a.md')).toBeVisible();
  await expect(treeItem(page, 'sub')).toHaveClass(/dir-open/);
  // カーソルも同じ行へ戻る。
  await expect(page.locator('.tree-item.cursor')).toHaveAttribute('data-path', cursorPath);
});
