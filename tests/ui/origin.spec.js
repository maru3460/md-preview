// 送り手の名乗り（#68）。`--title` / `--description` / `--note` が、タブ・ベル・帯・
// コメントのラベルの決まった場所に出るまで（description と note は 1 か所ずつ、
// title は名前としてタブ・ベル・コメントのラベルに出る）。
//
// 名乗りを渡すのは Rust（起動時は MD_ORIGINS、転送は MdCommon.setOrigin を開く直前に、
// ベルは MdNotify.push の各行に載せて）。ここではその口を直接叩く。CLI からワイヤを
// 通って届くまでは tests/single_instance.rs と cli.rs のユニットテストが持つ。
const fs = require('fs');
const path = require('path');
const { test, expect, openFolder, id } = require('./helpers');

const tabNames = (page) => page.locator('.md-tab .md-tab-name');
const band = (page) => page.locator('#origin-band');
const rows = (page) => page.locator('.md-bell-row');

/// パイプ入力の実体化先と同じ形の識別子。中身は取りに行けないが、名前と帯は
/// 識別子と名乗りだけで決まる。親はページが知っている `$TMPDIR`（MD_STDIN_DIR）。
const spoolId = (page, n) => page.mdStdinDir + '/md-stdin-' + n + '/stdin.md';

/// フォルダ起動で開いて、パイプ入力の置き場所を拾っておく。
async function openShell(page) {
  await openFolder(page);
  page.mdStdinDir = await page.evaluate(() => window.MD_STDIN_DIR);
}

/// 転送で名乗り付きのパイプ入力が届いた、の再現（main.rs と同じ順に流す）。
async function forwardNamed(page, ids, origin) {
  await page.evaluate(([v, o]) => {
    window.MdCommon.setOrigin(v, o);
    window.MdOpenFiles(v);
  }, [ids, origin]);
}

test('名乗った title がタブ名になり、無ければ stdin.md のまま', async ({ page }) => {
  await openShell(page);
  await forwardNamed(page, [spoolId(page, '111')], { title: 'PR #12 のレビュー', description: null, note: null });
  await page.evaluate((v) => window.MdOpenFiles(v), [spoolId(page, '222')]);
  await expect(tabNames(page)).toHaveText(['PR #12 のレビュー', 'stdin.md']);
});

test('消えたパイプ入力を開いたときの知らせも、タブと同じ title で出る', async ({ page }) => {
  await openShell(page);
  // 実体の無い一時ファイル（タブを閉じた後にベルから開き直した、の形）。
  await forwardNamed(page, [spoolId(page, 'gone')], { title: '調査メモ', description: null, note: null });
  await expect(page.locator('#preview-pane')).toContainText('このファイルは開けませんでした: 調査メモ');
});

test('title の / で名前を割らず、同名の親フォルダも添えない', async ({ page }) => {
  await openShell(page);
  await forwardNamed(page, [spoolId(page, '333')], { title: 'api/review', description: null, note: null });
  await forwardNamed(page, [spoolId(page, '444')], { title: 'api/review', description: null, note: null });
  await expect(tabNames(page)).toHaveText(['api/review', 'api/review']);
  await expect(page.locator('.md-tab-dir')).toHaveCount(0);
});

test('パイプ入力の note はタブの下の帯に出て、名乗っていないタブでは帯ごと消える', async ({ page }) => {
  await openShell(page);
  await forwardNamed(page, [spoolId(page, '555')], {
    title: 't', description: 'ベルにだけ出る', note: '判断してほしいのは 3 節の 2 案',
  });
  await expect(band(page)).toBeVisible();
  // description は帯に出さない（1 つのフラグは 1 か所だけ）。
  await expect(band(page)).toHaveText('判断してほしいのは 3 節の 2 案');
  // 切れたぶんはホバーで読む。
  await expect(band(page)).toHaveAttribute('title', '判断してほしいのは 3 節の 2 案');

  // 値は HTML として解釈しない。
  await forwardNamed(page, [spoolId(page, '666')], { title: null, description: null, note: '<b>x</b>' });
  await expect(band(page)).toHaveText('<b>x</b>');
  await expect(band(page).locator('b')).toHaveCount(0);

  // note の無いパイプ入力と、名乗りの無いファイルでは帯ごと消える。
  await forwardNamed(page, [spoolId(page, '667')], { title: 'only title', description: null, note: null });
  await expect(band(page)).toBeHidden();
  await page.locator('#sidebar .tree-item', { hasText: 'b.md' }).click();
  await expect(band(page)).toBeHidden();
});

test('ベルの行は title / description / 時刻の 3 段で、description が無ければ 2 段のまま', async ({ page }) => {
  await openShell(page);
  await page.evaluate(([stdin, real]) => {
    const now = Math.floor(Date.now() / 1000);
    window.MdNotify.push([
      { path: stdin, at: now - 120, read: false,
        origin: { title: '調査メモ', description: 'md-preview: ベルの調査', note: '帯にだけ出る' } },
      { path: real, at: now - 120, read: false,
        origin: { title: '計画の更新', description: null, note: null } },
      { path: real.replace(/a\.md$/, 'b.md'), at: now - 120, read: false,
        origin: { title: null, description: null, note: null } },
    ]);
  }, [spoolId(page, '777'), id(page, 'sub/a.md')]);
  await page.keyboard.press('b');

  await expect(rows(page).nth(0).locator('.md-bell-name')).toHaveText('調査メモ');
  await expect(rows(page).nth(0).locator('.md-bell-desc')).toHaveText('md-preview: ベルの調査');
  // パイプ入力は置き場所を名前に出さない。note はベルに出さない。
  await expect(rows(page).nth(0).locator('.md-bell-sub')).toHaveText('2 分前');
  await expect(rows(page).nth(0)).not.toContainText('帯にだけ出る');
  // 実ファイルの title は名前の段に出し、ファイル名は時刻の段へ下ろす。
  await expect(rows(page).nth(1).locator('.md-bell-name')).toHaveText('計画の更新');
  await expect(rows(page).nth(1).locator('.md-bell-sub')).toHaveText('sub/a.md · 2 分前');
  await expect(rows(page).nth(1).locator('.md-bell-desc')).toHaveCount(0);
  await expect(rows(page).nth(2).locator('.md-bell-desc')).toHaveCount(0);

  await rows(page).nth(0).click();
  await expect(page.locator('.md-tab.active .md-tab-name')).toHaveText('調査メモ');
  await expect(band(page)).toHaveText('帯にだけ出る');
});

/// 実ファイル `rel` がベルに note 付きで積まれている状態を作る。
async function pushReal(page, rel, note) {
  await page.evaluate(([p, n]) => {
    window.MdNotify.push([{ path: p, at: Math.floor(Date.now() / 1000), read: false,
      origin: { title: '計画', description: 'md-preview: 計画', note: n } }]);
  }, [id(page, rel), note]);
}

test('実ファイルの帯は、ベルから開いたタブにだけ出て、タブを閉じれば消える', async ({ page }) => {
  await openShell(page);
  await pushReal(page, 'a.md', 'ベル経由の補足');

  // ふつうに開いたものには、ベルに行が残っていても出さない。
  await page.locator('#sidebar .tree-item', { hasText: 'a.md' }).first().click();
  await expect(band(page)).toBeHidden();

  // 同じファイルのタブが開いたままでも、ベルから開き直せば印が付く。
  await page.keyboard.press('b');
  await rows(page).first().click();
  await page.keyboard.press('Escape');
  await expect(band(page)).toHaveText('ベル経由の補足');

  // 他のタブへ行って戻っても、raw の往復や保存での読み直しの後でも残る。
  await page.locator('#sidebar .tree-item', { hasText: 'b.md' }).click();
  await expect(band(page)).toBeHidden();
  await page.locator('.md-tab', { hasText: 'a.md' }).click();
  await expect(band(page)).toHaveText('ベル経由の補足');
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toHaveCount(0);
  await page.evaluate((p) => window.MdReload(p), id(page, 'a.md'));
  await expect(page.locator('#preview-pane h1').first()).toBeVisible();
  // 読み直しの経路は帯に触らないので、DOM が前のまま残っているだけでも通ってしまう。
  // 印から描き直させて、印そのものが残っていることを見る。
  await page.evaluate(() => window.MdRefreshOriginBand());
  await expect(band(page)).toHaveText('ベル経由の補足');

  // タブを閉じたら印も消え、ツリーから開き直しても出ない。
  await page.evaluate((p) => window.MdTabs.closeByPath(p), id(page, 'a.md'));
  await expect(page.locator('.md-tab', { hasText: 'a.md' })).toHaveCount(0);
  await page.locator('#sidebar .tree-item', { hasText: 'a.md' }).first().click();
  await expect(band(page)).toBeHidden();
});

test('まとめて閉じたタブも、ベルから開いた印が外れる', async ({ page }) => {
  await openShell(page);
  for (const [fn, keep] of [['closeOthers', 'b.md'], ['closeAll', null]]) {
    await pushReal(page, 'a.md', '補足');
    await page.keyboard.press('b');
    await rows(page).first().click();
    await page.keyboard.press('Escape');
    await expect(band(page)).toHaveText('補足');
    await page.locator('#sidebar .tree-item', { hasText: 'b.md' }).click();
    await page.evaluate(([f, k]) => (k ? window.MdTabs[f](k) : window.MdTabs[f]()), [fn, keep && id(page, keep)]);
    await page.locator('#sidebar .tree-item', { hasText: 'a.md' }).first().click();
    await expect(band(page)).toBeHidden();
  }
});

test('帯の出ているタブから設定タブへ移るか、全部閉じると帯は消える', async ({ page }) => {
  await openShell(page);
  await pushReal(page, 'a.md', '補足');
  await page.keyboard.press('b');
  await rows(page).first().click();
  await page.keyboard.press('Escape');
  await expect(band(page)).toHaveText('補足');

  await page.keyboard.press('Meta+,');
  await expect(page.locator('.md-tab.active')).not.toContainText('a.md');
  await expect(band(page)).toBeHidden();
  // 設定タブを見ている間に届いた送り直しでも出さない。
  await pushReal(page, 'a.md', '補足 2');
  await expect(band(page)).toBeHidden();

  await page.locator('.md-tab', { hasText: 'a.md' }).click();
  await expect(band(page)).toHaveText('補足 2');
  await page.evaluate(() => window.MdTabs.closeAll());
  await expect(band(page)).toBeHidden();
});

test('窓が無いときの --notify で開いたタブは、ベルの行が届くと帯が出る', async ({ page }) => {
  // Rust は MD_BELL_OPENED で印を渡し、台帳は Ready で後から流す（main.rs）。
  // 起動スクリプトが入れる空の値に上書きされないよう、読み出しを差し替える。
  const root = fs.realpathSync(path.join(__dirname, '..', 'ui-fixtures'));
  const target = root + '/a.md';
  await page.addInitScript((t) => {
    Object.defineProperty(window, 'MD_BELL_OPENED', { configurable: true, get: () => [t], set: () => {} });
  }, target);
  await openShell(page);
  await page.locator('#sidebar .tree-item', { hasText: 'a.md' }).first().click();
  await expect(band(page)).toBeHidden();
  await pushReal(page, 'a.md', '起動時の補足');
  await expect(band(page)).toHaveText('起動時の補足');
});

test('帯の無いページでは、html の iframe は窓の下端まで使う', async ({ page }) => {
  await openShell(page);
  await page.locator('#sidebar .tree-item', { hasText: 'page.html' }).first().click();
  await expect(page.frameLocator('iframe.html-frame').locator('h1')).toBeVisible();
  await expect(band(page)).toBeHidden();
  const gap = await page.evaluate(() => {
    const r = document.querySelector('iframe.html-frame').getBoundingClientRect();
    return window.innerHeight - r.bottom;
  });
  // 残るのは html 表示の額縁（--md-pad: 8px）だけ。
  expect(gap).toBeCloseTo(8, 0);
});

test('送り直しで note が変われば開いているタブの帯も変わり、外せば消える', async ({ page }) => {
  await openShell(page);
  await pushReal(page, 'a.md', '1 版');
  await page.keyboard.press('b');
  await rows(page).first().click();
  await page.keyboard.press('Escape');
  await expect(band(page)).toHaveText('1 版');

  await pushReal(page, 'a.md', '2 版');
  await expect(band(page)).toHaveText('2 版');

  await pushReal(page, 'a.md', null);
  await expect(band(page)).toBeHidden();
});

test('html を帯付きで開くと、iframe は帯のぶんだけ縮んで窓の下端で止まる', async ({ page }) => {
  await openShell(page);
  await pushReal(page, 'page.html', 'html の補足');
  await page.keyboard.press('b');
  await rows(page).first().click();
  await page.keyboard.press('Escape');
  await expect(band(page)).toBeVisible();
  await expect(page.frameLocator('iframe.html-frame').locator('h1')).toBeVisible();
  const [bottom, vh] = await page.evaluate(() => {
    const r = document.querySelector('iframe.html-frame').getBoundingClientRect();
    return [r.bottom, window.innerHeight];
  });
  // 帯のぶんだけ引いて、残るのは帯の無いときと同じ額縁（--md-pad: 8px）だけ。
  expect(vh - bottom).toBeCloseTo(8, 0);
});

test('$TMPDIR の外にある md-stdin-*/stdin.md は、パイプ入力と見なさない', async ({ page }) => {
  // 形だけで判定すると、実ファイルが `stdin.md` と名乗り出す（コピーのラベルも変わる）。
  await openShell(page);
  const fake = '/work/md-stdin-old/stdin.md';
  expect(await page.evaluate((v) => window.MdCommon.isStdinId(v), fake)).toBe(false);
  expect(await page.evaluate((v) => window.MdCommon.isStdinId(v), spoolId(page, '1'))).toBe(true);
});

test('実ファイルに付いた title はタブ名にしない', async ({ page }) => {
  await openShell(page);
  // ファイルは自分の名前を持っている。ツリーから開き直せば元に戻る名前は付けない。
  await forwardNamed(page, [id(page, 'a.md')], { title: '別名', description: null, note: '補足' });
  await expect(tabNames(page)).toHaveText(['a.md']);
  await expect(band(page)).toBeHidden();
});

test('パイプ入力へのコメントは、コピーでも一時ファイルのパスではなく title で出る', async ({ page }) => {
  await page.addInitScript(() => {
    window.__copied = null;
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: (t) => { window.__copied = t; return Promise.resolve(); } },
    });
  });
  // 中身を取りに行けるよう、実体化先と同じ形の一時ファイルを本当に作る。
  await openShell(page);
  const dir = fs.mkdtempSync(path.join(page.mdStdinDir, 'md-stdin-'));
  const doc = path.join(dir, 'stdin.md');
  fs.writeFileSync(doc, '# 見出し\n\n本文の 1 行。\n');
  try {
    await forwardNamed(page, [doc], { title: '調査メモ', description: null, note: null });
    await expect(page.locator('#preview-pane h1')).toHaveText('見出し');

    await page.keyboard.press('c');
    await page.locator('#preview-pane [data-src-line]').first().click();
    await expect(page.locator('.md-cmt-popover-where')).toHaveText('調査メモ:1');
    await page.locator('.md-cmt-textarea').fill('ここ');
    await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
    await expect(page.locator('.md-cmt-side')).toContainText('調査メモ:1');

    await page.locator('.md-cmt-side .md-cmt-btn-primary').click();
    const copied = await page.evaluate(() => window.__copied);
    expect(copied).toContain('- 調査メモ:1');
    expect(copied).not.toContain(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
