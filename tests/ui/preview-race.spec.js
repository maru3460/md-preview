// 本文フェッチ（`?file=`）の世代ガード。
//
// 応答は必ずしも投げた順に返らない（main.rs も examples/serve.rs も
// リクエストごとにスレッドを立てる）。守りが無いと、先に投げた古い応答が
// 後から着地して「タブとツリーは新しいファイル・本文は古いファイル」という
// 状態分裂になり、その後の ⌘R / ⌘D / コメントが見えていないファイルに効く。
//
// 素の状態では窓が 0.3ms 前後しかないので、ここでは `?file=` を遅らせて窓を
// 開ける。遅らせるのは Playwright 側だけで、サーバは触らない。
const { test, expect } = require('@playwright/test');
const { openFolder, treeItem, tab } = require('./helpers');

/// 最初の `?file=` だけを `ms` ミリ秒遅らせる。述語で見るのは、クエリ無しの `/`
/// （ページ本体）と `?dir=` を巻き込まないため（tree-dot.spec.js に倣う）。
function delayFirstFile(page, ms) {
  let delayed = false;
  return page.route(
    (url) => url.searchParams.has('file'),
    async (route) => {
      if (!delayed) {
        delayed = true;
        await new Promise((r) => setTimeout(r, ms));
      }
      await route.continue();
    }
  );
}

/// `?file=` を数えながら、最初の 1 本だけ遅らせる。
function countFileRequests(page, delayFirstMs) {
  const seen = [];
  let delayed = false;
  page.route(
    (url) => url.searchParams.has('file'),
    async (route, request) => {
      seen.push(new URL(request.url()).searchParams.get('file'));
      if (!delayed && delayFirstMs) {
        delayed = true;
        await new Promise((r) => setTimeout(r, delayFirstMs));
      }
      await route.continue();
    }
  );
  return seen;
}

const bodyText = (page) => page.locator('#preview-pane .markdown-body');

test('遅れて届いた本文は、あとから開いたファイルを上書きしない', async ({ page }) => {
  await openFolder(page);
  await delayFirstFile(page, 300);

  // long.md の応答を遅らせたまま a.md へ移る。守りが無いと 300ms 後に long.md が
  // 着地して、タブは a.md のまま本文だけ long.md になる。
  await treeItem(page, 'long.md').click();
  await treeItem(page, 'a.md').click();

  await expect(tab(page, 'a.md')).toHaveClass(/active/);
  await expect(bodyText(page)).toContainText('見出し A');
  await page.waitForTimeout(600);
  await expect(bodyText(page)).toContainText('見出し A');
  await expect(bodyText(page)).not.toContainText('長い見出し');
  await expect(tab(page, 'a.md')).toHaveClass(/active/);
});

test('本文の取得中に raw へ切り替えても、あとから本文が上書きしない', async ({ page }) => {
  // 世代（reqSeq）はモードの ON では進まない。ここを世代だけで守ると、raw が出て
  // いるのに本文が後ろから被さって「⌘R は点いているのにレンダリング結果が見える」
  // という食い違った画面になる。
  await openFolder(page);
  await delayFirstFile(page, 300);

  await treeItem(page, 'a.md').click();
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();

  await page.waitForTimeout(600);
  // 生ソースは `# 見出し A` のまま（レンダリング結果なら `#` が消えて h1 になる）。
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(page.locator('#preview-pane')).toContainText('# 見出し A');
});

test('本文の取得中にすべてのタブを閉じても、あとから本文が戻ってこない', async ({ page }) => {
  await openFolder(page);
  await treeItem(page, 'a.md').click();
  await expect(bodyText(page)).toContainText('見出し A');

  await delayFirstFile(page, 300);
  await treeItem(page, 'long.md').click();
  await tab(page, 'a.md').click({ button: 'right' });
  await page.locator('.md-context-menu-item', { hasText: 'すべてのタブを閉じる' }).click();

  await expect(page.locator('.md-tab')).toHaveCount(0);
  await page.waitForTimeout(600);
  await expect(bodyText(page)).toBeEmpty();
  await expect(page.locator('.md-tab')).toHaveCount(0);
});

test('本文がまだ届いていない間のホットリロードは、取りに行き直さない', async ({ page }) => {
  // preserveScroll の経路は「いま見えている位置」を錨として持ち回る。ペインの中身が
  // まだ前のファイルのままだと、その錨は前のファイルのものなので、**新しいファイルを
  // 前のファイルの読み位置へ着地させる**。世代ガードが入ったぶん「必ず後から来た方が
  // 勝つ」ようになったので、ここを塞がないと毎回そうなる。
  //
  // 「エディタで保存した直後にそのファイルを開く」で踏める（監視のデバウンスは 80ms）。
  await openFolder(page);
  const seen = countFileRequests(page, 300);

  await treeItem(page, 'long.md').click();
  const id = page.mdRoot + '/long.md';
  await page.evaluate((v) => window.MdReload(v), id);

  await page.waitForTimeout(600);
  await expect(bodyText(page)).toContainText('長い見出し');
  expect(seen.filter((f) => f === id)).toHaveLength(1);
});

test('⌘R を連打しても、OFF のときに飛んだ遅い本文が raw を上書きしない', async ({ page }) => {
  // 上の raw のテストと守っている行は同じだが、通る道が違う。あちらは「初回の本文が
  // 飛んでいる最中にモードを ON」で、こちらは「ON → OFF（ここで本文フェッチが飛ぶ）
  // → ON」。OFF の `reloadNormal` は loadPreview を通るので世代を進めてしまい、
  // そのあと ON にしても**その本文は自分の世代のまま**着地する。世代だけでは止まらない。
  await openFolder(page);
  await treeItem(page, 'a.md').click();
  await expect(bodyText(page)).toContainText('見出し A');

  // ここから先の `?file=` を遅らせる（初回はもう着地している）。
  await delayFirstFile(page, 400);

  const raw = page.locator('.md-raw-toggle');
  await page.keyboard.press('Meta+r');   // ON
  await expect(page.locator('.source-view')).toBeVisible();
  await page.keyboard.press('Meta+r');   // OFF → 遅い本文フェッチが飛ぶ
  await page.keyboard.press('Meta+r');   // ON → 速い raw フェッチが先に着く

  await expect(raw).toHaveClass(/active/);
  await expect(page.locator('.source-view')).toBeVisible();
  // 遅い本文が着いたあとも raw が生きていること。
  await page.waitForTimeout(900);
  await expect(raw).toHaveClass(/active/);
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(page.locator('#preview-pane')).toContainText('# 見出し A');
});
