// タブ（tabs.js）。複数ファイルを開いたまま行き来する。
//
// 状態（パス・読み位置・ビューモード）は tabs.js だけが持っていて、Rust 側は
// 起動時の INITIAL_FILES しか知らない。つまりここが唯一の砦なので、実装中に
// 使い捨てのハーネスで見つかった 2 件（最初の 1 枚が作られない / タブ 0 枚の
// ⌘W が無反応）も回帰として残してある。
const { test, expect } = require('@playwright/test');
const { MULTI_URL, open, openFolder } = require('./helpers');

/// ツリーからファイルを開く。data-path は root 相対なので、同名ファイル
/// （a.md と sub/a.md）でも取り違えない。
async function openFile(page, relPath) {
  await page.locator(`.tree-item[data-path="${relPath}"]`).click();
  await expect(page.locator(`.md-tab[data-path="${relPath}"]`)).toHaveClass(/active/);
}

/// 折り畳まれているフォルダを開いて、子の行が出るまで待つ。
async function expandDir(page, relPath) {
  await page.locator(`.tree-item[data-path="${relPath}"]`).click();
  await expect(page.locator(`.tree-item[data-path="${relPath}/a.md"]`)).toBeVisible();
}

/// タブバーに並んでいるファイル名（左から順）。
function tabNames(page) {
  return page.locator('.md-tab .md-tab-name');
}

/// いま active なタブのパス。
function activePath(page) {
  return page.locator('.md-tab.active').getAttribute('data-path');
}

const pane = (page) => page.evaluate(() => document.getElementById('preview-pane').scrollTop);

test('開くたびにタブが増え、現在タブの右隣に挿さる', async ({ page }) => {
  await openFolder(page);
  // まだ何も開いていない間はタブバーごと出ない。
  await expect(page.locator('.md-tab')).toHaveCount(0);
  await expect(page.locator('body')).not.toHaveClass(/has-tabs/);

  // 最初の 1 枚。indexOf の -1 と初期 activeIdx の -1 が一致してしまうと、
  // ここで永久にタブが作られない。
  await openFile(page, 'a.md');
  await expect(page.locator('.md-tab')).toHaveCount(1);
  await expect(page.locator('body')).toHaveClass(/has-tabs/);

  await openFile(page, 'b.md');
  await openFile(page, 'long.md');
  await expect(tabNames(page)).toHaveText(['a.md', 'b.md', 'long.md']);

  // 同じファイルをもう一度開いてもタブは増えない。
  await openFile(page, 'long.md');
  await expect(page.locator('.md-tab')).toHaveCount(3);

  // 左端へ戻ってから開くと、末尾ではなく現在タブの右隣に挿さる（VSCode と同じ）。
  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('a.md');
  await openFile(page, 'notes.txt');
  await expect(tabNames(page)).toHaveText(['a.md', 'notes.txt', 'b.md', 'long.md']);
});

test('タブごとに読み位置が復元される', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'long.md');
  // 復元先を作る。長いファイルなので実際にスクロールできる。
  await page.evaluate(() => { document.getElementById('preview-pane').scrollTop = 600; });
  expect(await pane(page)).toBe(600);

  // 別タブへ移ると、そのタブは先頭から始まる。
  await openFile(page, 'a.md');
  await expect.poll(() => pane(page)).toBe(0);

  // 戻ると離れた位置に着地する（本文は再フェッチしているので、描画後の復元）。
  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('long.md');
  await expect.poll(() => pane(page)).toBe(600);
});

/// iframe の中の読み位置。html 表示でない間は -1（呼ばないこと）。
async function frameTop(page) {
  return page.evaluate(() => {
    const f = document.querySelector('iframe.html-frame');
    if (!f || !f.contentDocument) return -1;
    const d = f.contentDocument;
    return (d.scrollingElement || d.documentElement).scrollTop;
  });
}

// html は iframe の中がスクロール主体で、親ペインは 1 画面ぶんぴったりなので
// 常に 0 のまま動かない。ペインだけ見ていると保存する値が無く、戻ると先頭に落ちる。
test('html 表示（iframe）でもタブごとに読み位置が復元される', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'page.html');
  // 中身が縦に伸びるまで待つ（伸びる前に測ると復元先が作れない）。
  await expect.poll(() => page.evaluate(() => {
    const f = document.querySelector('iframe.html-frame');
    if (!f || !f.contentDocument) return 0;
    const d = f.contentDocument;
    const el = d.scrollingElement || d.documentElement;
    return el.scrollHeight - el.clientHeight;
  })).toBeGreaterThan(600);
  await page.evaluate(() => {
    const d = document.querySelector('iframe.html-frame').contentDocument;
    (d.scrollingElement || d.documentElement).scrollTop = 600;
  });
  expect(await frameTop(page)).toBe(600);

  // 別タブへ移る。ここで読み位置を拾えていないと、戻ったとき 0 になる。
  await openFile(page, 'a.md');
  await expect.poll(() => pane(page)).toBe(0);

  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('page.html');
  // iframe を作り直して load を待つぶん、復元は本文より遅れて届く。
  await expect.poll(() => frameTop(page)).toBe(600);
});

test('タブごとに Raw / 差分の状態が復元される', async ({ page }) => {
  await openFolder(page);
  const raw = page.locator('.md-raw-toggle');

  await openFile(page, 'a.md');
  await page.keyboard.press('Meta+r');
  await expect(raw).toHaveClass(/active/);

  // 新しいタブは直前のモードを引き継ぐ（raw のまま次のファイルへ、という従来の挙動）。
  await openFile(page, 'b.md');
  await expect(raw).toHaveClass(/active/);
  // こちらだけ raw を畳む。
  await page.keyboard.press('Meta+r');
  await expect(raw).not.toHaveClass(/active/);

  // a.md へ戻ると raw が戻る。
  await page.keyboard.press('Meta+1');
  await expect(raw).toHaveClass(/active/);

  // raw トグルを持たない非 md を経由しても、戻ってきた時に壊れていない。
  await openFile(page, 'notes.txt');
  await expect(raw).toBeHidden();
  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('a.md');
  await expect(raw).toHaveClass(/active/);

  // b.md（畳んだ側）は畳んだまま。
  await page.keyboard.press('Meta+3');
  expect(await activePath(page)).toBe('b.md');
  await expect(raw).not.toHaveClass(/active/);
});

test('raw 表示のままタブを移っても読み位置が復元される', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'long.md');
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.md-raw-toggle')).toHaveClass(/active/);
  // ソースが出て縦に伸びるまで待つ（伸びる前に測ると復元先が作れない）。
  await expect.poll(() => page.evaluate(() => {
    const el = document.getElementById('preview-pane');
    return el.scrollHeight - el.clientHeight;
  })).toBeGreaterThan(600);
  await page.evaluate(() => { document.getElementById('preview-pane').scrollTop = 600; });
  expect(await pane(page)).toBe(600);

  // 読み位置より短いファイルを経由する。ここが 0 になるのが復元を壊す条件だった。
  await openFile(page, 'a.md');
  await expect.poll(() => pane(page)).toBe(0);

  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('long.md');
  await expect(page.locator('.md-raw-toggle')).toHaveClass(/active/);
  await expect.poll(() => pane(page)).toBe(600);
});

test('⇧Tab の巡回は端で先頭へ折り返す', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'b.md');
  await openFile(page, 'long.md');

  // 右端に居るので、次は先頭へ折り返す。
  await page.keyboard.press('Shift+Tab');
  expect(await activePath(page)).toBe('a.md');
  await page.keyboard.press('Shift+Tab');
  expect(await activePath(page)).toBe('b.md');
  await page.keyboard.press('Shift+Tab');
  expect(await activePath(page)).toBe('long.md');

  // Tab（shift 無し）はツリーとのフォーカス切替のままで、タブは動かさない。
  await page.keyboard.press('Tab');
  await expect(page.locator('body')).toHaveClass(/nav-tree/);
  expect(await activePath(page)).toBe('long.md');
});

test('⌘1..⌘9 で番号のタブへ飛ぶ（⌘9 は最後、無い番号は無反応）', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'b.md');
  await openFile(page, 'long.md');

  await page.keyboard.press('Meta+2');
  expect(await activePath(page)).toBe('b.md');

  // ⌘9 は VSCode と同じく「最後のタブ」。
  await page.keyboard.press('Meta+9');
  expect(await activePath(page)).toBe('long.md');

  // 3 枚しか無いのに ⌘5。端へ寄せず何もしない（押し間違いで飛ばされないため）。
  await page.keyboard.press('Meta+5');
  expect(await activePath(page)).toBe('long.md');

  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('a.md');
});

test('⌘W はアクティブを閉じて右隣（右端なら左隣）へ移る', async ({ page }) => {
  await openFolder(page);
  await openFile(page, 'a.md');
  await openFile(page, 'b.md');
  await openFile(page, 'long.md');

  // 右端を閉じたら左隣。
  await page.keyboard.press('Meta+w');
  await expect(tabNames(page)).toHaveText(['a.md', 'b.md']);
  expect(await activePath(page)).toBe('b.md');

  // 左端を閉じたら右隣。
  await page.keyboard.press('Meta+1');
  expect(await activePath(page)).toBe('a.md');
  await page.keyboard.press('Meta+w');
  await expect(tabNames(page)).toHaveText(['b.md']);
  expect(await activePath(page)).toBe('b.md');
});

test('最後の 1 枚とタブ 0 枚の ⌘W はウィンドウを閉じる', async ({ page }) => {
  await openFolder(page);
  const ipc = () => page.evaluate(() => window.__mdIpc.filter((m) => m === 'close').length);

  // タブが 1 枚も無い状態（`md .` で起動してまだ何も開いていない）。ここで
  // 抜けてしまうと、⌘W に割り当てられているのは tab-close だけなので
  // ⌘W が完全に無反応になる。
  expect(await page.locator('.md-tab').count()).toBe(0);
  await page.keyboard.press('Meta+w');
  await expect.poll(ipc).toBe(1);

  // 最後の 1 枚も同じ（⌘W の従来の意味を残す）。タブは閉じずに残る。
  await openFile(page, 'a.md');
  await page.keyboard.press('Meta+w');
  await expect.poll(ipc).toBe(2);
  await expect(page.locator('.md-tab')).toHaveCount(1);
});

test('同名ファイルが並んだときだけ親ディレクトリ名が出る', async ({ page }) => {
  await openFolder(page);
  await expandDir(page, 'sub');

  // 1 枚だけなら添え字は要らない。
  await openFile(page, 'sub/a.md');
  await expect(tabNames(page)).toHaveText(['a.md']);
  await expect(page.locator('.md-tab-dir')).toHaveCount(0);

  // 同名が並んだので、区別のために親ディレクトリ名が付く。root 直下の a.md には
  // 親が無いので、付くのは sub 側だけ。
  await openFile(page, 'a.md');
  await expect(tabNames(page)).toHaveText(['a.md', 'a.md']);
  await expect(page.locator('.md-tab-dir')).toHaveText(['sub']);
  await expect(page.locator('.md-tab[data-path="sub/a.md"] .md-tab-dir')).toHaveText('sub');

  // 片方を閉じたら添え字も消える。
  await page.keyboard.press('Meta+w');
  await expect(page.locator('.md-tab-dir')).toHaveCount(0);
});

test('起動時に渡した複数ファイルがタブとして並ぶ', async ({ page }) => {
  // `md a.md sub/a.md` 相当のサーバ（playwright.config.js の MULTI_PORT）。
  await open(page, MULTI_URL);
  await expect(page.locator('.md-tab')).toHaveCount(2);
  await expect(tabNames(page)).toHaveText(['a.md', 'a.md']);
  // 先頭が最初に見えるタブ。
  expect(await activePath(page)).toBe('a.md');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  // 残りは開いた時に取りに行く（起動を N ファイルぶん遅らせない）。
  await page.keyboard.press('Meta+2');
  expect(await activePath(page)).toBe('sub/a.md');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('サブフォルダの見出し A');
});
