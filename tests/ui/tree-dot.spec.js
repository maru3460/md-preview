// ツリーのドット（`?has_md=` の判定）の機構。
//
// ドットは「配下に md がある」の印で、サーバ側の走査は予算付き（issue #5）。
// ここが守るのは見た目ではなく、判定を投げる側の約束: 同時に飛ぶ本数に上限が
// あること、畳んだフォルダの保留は投げないこと、答えが出なかった（不明）ものを
// 確定させずに開き直しで拾い直すこと。
//
// 1 本目だけは実サーバ相手にして、残りの stub がワイヤ契約から浮かないようにする。
const { test, expect } = require('@playwright/test');
const { FOLDER_URL, open, openFolder } = require('./helpers');

/// `?has_md=` だけを横取りする。glob ではなく述語で見るのは、クエリ無しの `/`
/// （ページ本体）を巻き込まないため。
function routeHasMd(page, handler) {
  return page.route(
    (url) => url.searchParams.has('has_md'),
    handler
  );
}

/// `?dir=` を捏造したツリーに差し替える。`names` は root 直下のフォルダ名。
function routeDirs(page, tree) {
  return page.route(
    (url) => url.searchParams.has('dir'),
    (route, request) => {
      const rel = new URL(request.url()).searchParams.get('dir');
      const names = tree[rel] || [];
      const items = names.map((name) => ({
        name,
        path: rel ? rel + '/' + name : name,
        kind: 'dir',
      }));
      route.fulfill({ contentType: 'application/json', body: JSON.stringify(items) });
    }
  );
}

/// 保留中の判定が捌け切るまで待つ。時間ではなく状態で待つ。
async function settled(page) {
  await expect
    .poll(() => page.locator('.tree-item[data-md-dot="pending"]').count())
    .toBe(0);
}

test('md を含むフォルダにだけ点が付く', async ({ page }) => {
  await openFolder(page);

  const sub = page.locator('.tree-item[data-path="sub"]');
  const noMd = page.locator('.tree-item[data-path="no-md"]');

  await expect(sub).toHaveAttribute('data-md-dot', 'yes');
  await expect(sub).toHaveClass(/has-md/);

  await expect(noMd).toHaveAttribute('data-md-dot', 'no');
  await expect(noMd).not.toHaveClass(/has-md/);
});

test('保留中の判定は 3 本までしか同時に飛ばない', async ({ page }) => {
  const names = [];
  for (let i = 0; i < 12; i++) names.push('d' + i);

  const release = [];

  await routeDirs(page, { '': names });
  await routeHasMd(page, (route) => {
    // 応答を握ったまま溜める。溜まった数＝同時にサーバへ届いた数。
    release.push(() => route.fulfill({ contentType: 'application/json', body: '{"has_md":"no"}' }));
  });

  await open(page, FOLDER_URL);
  await expect.poll(() => release.length).toBe(3);
  // 一拍おいても 3 のまま（通過点をたまたま捉えたのではないこと）。
  await page.waitForTimeout(100);
  expect(release.length).toBe(3);

  // 溜めた分を 1 本ずつ返すと、空いた枠に次が入る。
  while (release.length) {
    await release.shift()();
    await page.waitForTimeout(10);
  }
  await settled(page);

  await expect(page.locator('.tree-item[data-md-dot="no"]')).toHaveCount(12);
});

test('フォルダを畳むと、まだ投げていない配下の判定は飛ばない', async ({ page }) => {
  const children = [];
  for (let i = 0; i < 12; i++) children.push('c' + i);

  const asked = [];
  const release = [];

  await routeDirs(page, { '': ['big'], big: children });
  await routeHasMd(page, (route, request) => {
    const rel = new URL(request.url()).searchParams.get('has_md');
    asked.push(rel);
    release.push(() => route.fulfill({ contentType: 'application/json', body: '{"has_md":"no"}' }));
  });

  await open(page, FOLDER_URL);
  // root 直下の big が 1 本目。返して枠を空けてから展開する。
  await expect.poll(() => release.length).toBe(1);
  await release.shift()();

  await page.locator('.tree-item[data-path="big"]').click();
  await expect.poll(() => release.length).toBe(3);

  // 3 本が飛んだ状態で畳む。残り 9 本は投げられないまま捨てられる。
  await page.locator('.tree-item[data-path="big"]').click();
  while (release.length) {
    await release.shift()();
    await page.waitForTimeout(10);
  }
  await settled(page);

  const askedUnderBig = asked.filter((p) => p.indexOf('big/') === 0);
  expect(askedUnderBig.length).toBeLessThanOrEqual(3);
});

test('判定が不明のフォルダには点を出さない', async ({ page }) => {
  await routeDirs(page, { '': ['huge'] });
  await routeHasMd(page, (route) =>
    route.fulfill({ contentType: 'application/json', body: '{"has_md":"unknown"}' })
  );

  await open(page, FOLDER_URL);
  await settled(page);

  const huge = page.locator('.tree-item[data-path="huge"]');
  await expect(huge).toHaveAttribute('data-md-dot', 'unknown');
  await expect(huge).not.toHaveClass(/has-md/);
});

test('畳んで開き直すと、不明だったフォルダをもう一度判定する', async ({ page }) => {
  let answers = 0;

  await routeDirs(page, { '': ['x'], x: ['y'] });
  await routeHasMd(page, (route, request) => {
    const rel = new URL(request.url()).searchParams.get('has_md');
    // x/y だけ、1 回目は予算切れ・2 回目は見つかった、と答える。
    const md = rel === 'x/y' && answers++ === 0 ? 'unknown' : 'yes';
    route.fulfill({ contentType: 'application/json', body: '{"has_md":"' + md + '"}' });
  });

  await open(page, FOLDER_URL);
  const x = page.locator('.tree-item[data-path="x"]');
  const y = page.locator('.tree-item[data-path="x/y"]');

  await x.click();
  await expect(y).toHaveAttribute('data-md-dot', 'unknown');

  await x.click(); // 畳む
  await x.click(); // 開き直す
  await expect(y).toHaveAttribute('data-md-dot', 'yes');
  await expect(y).toHaveClass(/has-md/);
});

test('祖父を畳むと、間のフォルダが開いたままでも配下の判定は飛ばない', async ({ page }) => {
  // isRowHidden が祖先を遡ること。直近の親だけ見る実装でも 1 段のテストは通るので、
  // 3 段（a > b > c）にして、間の b を開いたまま a を畳む形で押さえる。
  const children = [];
  for (let i = 0; i < 12; i++) children.push('c' + i);

  const asked = [];
  const release = [];

  await routeDirs(page, { '': ['a'], a: ['b'], 'a/b': children });
  await routeHasMd(page, (route, request) => {
    asked.push(new URL(request.url()).searchParams.get('has_md'));
    release.push(() => route.fulfill({ contentType: 'application/json', body: '{"has_md":"no"}' }));
  });

  await open(page, FOLDER_URL);
  await expect.poll(() => release.length).toBe(1); // a
  await release.shift()();

  await page.locator('.tree-item[data-path="a"]').click();
  await expect.poll(() => release.length).toBe(1); // a/b
  await release.shift()();

  await page.locator('.tree-item[data-path="a/b"]').click();
  await expect.poll(() => release.length).toBe(3);

  // b は開いたまま、祖父の a だけを畳む。b 配下の保留は捨てられる。
  await page.locator('.tree-item[data-path="a"]').click();
  await expect(page.locator('.tree-item[data-path="a/b"]')).toHaveClass(/dir-open/);

  while (release.length) {
    await release.shift()();
    await page.waitForTimeout(10);
  }
  await settled(page);

  const askedUnderB = asked.filter((p) => p.indexOf('a/b/') === 0);
  expect(askedUnderB.length).toBeLessThanOrEqual(3);
});

test('応答が失敗しても点を確定させず、待ち行列の枠を返す', async ({ page }) => {
  // 非 200 と通信失敗の 2 経路。どちらも unknown 扱いで、inflight を戻すこと
  // （戻らないと 2 本目以降が永久に飛ばず、残りが pending のまま止まる）。
  const names = [];
  for (let i = 0; i < 8; i++) names.push('d' + i);

  let seen = 0;
  await routeDirs(page, { '': names });
  await routeHasMd(page, (route) => {
    const n = seen++;
    if (n % 2 === 0) route.fulfill({ status: 500, body: 'boom' });
    else route.abort();
  });

  await open(page, FOLDER_URL);
  await settled(page);

  await expect(page.locator('.tree-item[data-md-dot="unknown"]')).toHaveCount(8);
  await expect(page.locator('.tree-item.has-md')).toHaveCount(0);
});
