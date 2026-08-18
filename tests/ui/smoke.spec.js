// UI スモークテスト。
//
// 狙いは網羅ではなく「作り替えたときに黙って死んだら困る導線」を押さえること。
// 具体的には、フォーカス排他とオーバーレイの譲り合いに依存していて、Rust 側の
// テストでは一切触れない部分:
//   ・素キー（/ と c）が本文にフォーカスがある状態で効くか
//   ・モード限定のショートカット（⌘P）が、そのモードでだけ効くか
//   ・ファイル移動（]）が実際に本文を差し替えるか
//   ・表示モード（⌘R / ⌘D）が排他になっているか
//   ・オーバーレイの Esc が 1 回で 1 つだけ閉じるか
const { test, expect } = require('@playwright/test');

const SINGLE_URL = 'http://127.0.0.1:7879/';

/// 初回オンボーディング（? のヘルプ自動表示）を抑止して開く。
/// 出したままだと isOverlayOpen が true になり、素キーが全部止まる。
async function open(page, url) {
  await page.addInitScript(() => {
    try { localStorage.setItem('md-help-onboarded', '1'); } catch (e) {}
  });
  await page.goto(url || '/');
  // 初期描画の完了（init.js / folder.js が ready を投げたところ）を待つ。
  await page.waitForFunction(() => window.__mdIpc && window.__mdIpc.includes('ready'));
}

/// フォルダモードで、ツリーが描かれるのを待つ。
async function openFolder(page) {
  await open(page, '/');
  await expect(page.locator('.tree-item').first()).toBeVisible();
}

test('/ で検索バーが開き、Esc で閉じる', async ({ page }) => {
  await openFolder(page);
  const bar = page.locator('#md-search-bar');
  await expect(bar).toHaveClass(/hidden/);

  await page.keyboard.press('/');
  await expect(bar).not.toHaveClass(/hidden/);
  // 入力欄にフォーカスが移っていること（移らないと打った文字が本文のキー操作になる）。
  await expect(page.locator('.md-search-input')).toBeFocused();

  // 打った語が実際に本文でヒットすること。
  await page.locator('.md-search-input').fill('見出し');
  await expect(page.locator('.md-search-counter')).not.toHaveText('0/0');

  await page.keyboard.press('Escape');
  await expect(bar).toHaveClass(/hidden/);
  // 閉じたらハイライトも消えていること（色だけ残る壊れ方をしない）。
  await expect(page.locator('mark.md-search-hit')).toHaveCount(0);
});

test('c でコメントモードに入り、Esc で抜ける', async ({ page }) => {
  await openFolder(page);
  // コメント対象は本文のユニット（[data-src-line]）なので、まずファイルを開く。
  await page.keyboard.press(']');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  const body = page.locator('body');
  await expect(body).not.toHaveClass(/md-cmt-mode/);

  await page.keyboard.press('c');
  await expect(body).toHaveClass(/md-cmt-mode/);
  // モードに入ると、見えているユニットにキーボード・カーソルが置かれる。
  await expect(page.locator('.md-cmt-kbcursor')).toHaveCount(1);

  await page.keyboard.press('Escape');
  await expect(body).not.toHaveClass(/md-cmt-mode/);
  await expect(page.locator('.md-cmt-kbcursor')).toHaveCount(0);
});

test('Esc は最前面のオーバーレイを 1 つずつ閉じる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  // コメントモード → Enter で入力ポップオーバー、の 2 段重ね。
  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  await page.keyboard.press('Enter');
  await expect(page.locator('#md-cmt-popover')).toBeVisible();

  // 1 回目の Esc は前面のポップオーバーだけを閉じ、モードは残る
  // （まとめて閉じると、書きかけを捨てたうえにモードからも出てしまう）。
  await page.keyboard.press('Escape');
  await expect(page.locator('#md-cmt-popover')).toHaveCount(0);
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);

  // 2 回目でモードを抜ける。
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
});

test('⌘P はフォルダモードでだけ開く', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('Meta+p');
  const palette = page.locator('#md-pal-backdrop');
  await expect(palette).toBeVisible();
  // 一覧にフィクスチャのファイルが出ること（サーバの走査結果が届いている）。
  await expect(page.locator('.md-pal-row').first()).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(palette).toHaveCount(0);

  // 単一ファイルモードには「別のファイルを開く」入口が無いので初期化されない。
  await open(page, SINGLE_URL);
  await page.keyboard.press('Meta+p');
  await expect(page.locator('#md-pal-backdrop')).toHaveCount(0);
});

test('] で次のファイルへ移動する', async ({ page }) => {
  await openFolder(page);
  const bodyText = page.locator('#preview-pane .markdown-body');

  // 未選択の状態から ] を押すと、一覧の先頭（a.md）から入る。
  await page.keyboard.press(']');
  await expect(bodyText).toContainText('見出し A');
  await expect(page.locator('.tree-item.active')).toHaveText(/a\.md/);

  // もう一度で次のファイル（b.md）へ。
  await page.keyboard.press(']');
  await expect(bodyText).toContainText('見出し B');
  await expect(page.locator('.tree-item.active')).toHaveText(/b\.md/);

  // [ で戻る。
  await page.keyboard.press('[');
  await expect(bodyText).toContainText('見出し A');
});

test('⌘R と ⌘D は排他で、非 md では raw が無効になる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md を開く
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  const raw = page.locator('.md-raw-toggle');
  const diff = page.locator('.md-diff-toggle');

  // raw ON → ソースが出る（レンダリング前の `# 見出し A` が見える）。
  await page.keyboard.press('Meta+r');
  await expect(raw).toHaveClass(/active/);
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(page.locator('#preview-pane')).toContainText('# 見出し A');

  // diff ON → raw は自動で畳まれる（同時に 2 つ active にならない）。
  await page.keyboard.press('Meta+d');
  await expect(diff).toHaveClass(/active/);
  await expect(raw).not.toHaveClass(/active/);

  // もう一度 ⌘D で通常表示へ戻る。
  await page.keyboard.press('Meta+d');
  await expect(diff).not.toHaveClass(/active/);
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  // 非 md（notes.txt）は通常表示が既にソースなので raw トグルを隠す。
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('#preview-pane')).toContainText('md ではないテキストファイル');
  await expect(raw).toBeHidden();
});

test('? のヘルプが開き、Esc で閉じる（キー一覧は keymap から作られる）', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('?');
  const help = page.locator('#md-help-backdrop');
  await expect(help).toBeVisible();
  // フォルダモード限定のキー（⌘P）が一覧に出ていること。
  await expect(help).toContainText('⌘P');
  await page.keyboard.press('Escape');
  await expect(help).toHaveCount(0);

  // 単一ファイルモードでは ⌘P は一覧に出ない（scope: folder）。
  await open(page, SINGLE_URL);
  await page.keyboard.press('?');
  await expect(page.locator('#md-help-backdrop')).toBeVisible();
  await expect(page.locator('#md-help-backdrop')).not.toContainText('⌘P');
});

test('Tab でツリーへ移り、j/k がツリー操作になる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md を開く（本文にフォーカスがある状態から始める）
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  await page.keyboard.press('Tab');
  await expect(page.locator('body')).toHaveClass(/nav-tree/);
  // ツリーにフォーカスがあるので j/k は本文スクロールではなくカーソル移動になる。
  const cursor = page.locator('.tree-item.cursor');
  await expect(cursor).toHaveCount(1);
  const first = await cursor.textContent();
  await page.keyboard.press('j');
  expect(await cursor.textContent()).not.toBe(first);

  // Tab で本文へ戻る。
  await page.keyboard.press('Tab');
  await expect(page.locator('body')).not.toHaveClass(/nav-tree/);
});

test('⌘F は検索の入力欄にフォーカスがあっても閉じられる', async ({ page }) => {
  await openFolder(page);
  const bar = page.locator('#md-search-bar');

  await page.keyboard.press('Meta+f');
  await expect(bar).not.toHaveClass(/hidden/);
  await expect(page.locator('.md-search-input')).toBeFocused();
  await page.locator('.md-search-input').fill('見出し');

  // 入力欄にフォーカスがある状態からのトグル。ここで閉じられないと、
  // オーバーレイに閉じ込められて素キーが全部死ぬ。
  await page.keyboard.press('Meta+f');
  await expect(bar).toHaveClass(/hidden/);

  // 直前の語は消さないので、開き直すとそのまま検索を続けられる。
  await page.keyboard.press('Meta+f');
  await expect(bar).not.toHaveClass(/hidden/);
  await expect(page.locator('.md-search-input')).toHaveValue('見出し');
});

test('j / k で本文がスクロールする', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md を開く
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  const top = () => page.evaluate(() => document.getElementById('preview-pane').scrollTop);
  // 本文が 1 画面に収まると動かないので、確実にスクロールできる高さを作る。
  await page.evaluate(() => {
    var pad = document.createElement('div');
    pad.style.height = '3000px';
    document.querySelector('#preview-pane .markdown-body').appendChild(pad);
  });
  expect(await top()).toBe(0);
  await page.keyboard.press('j');
  expect(await top()).toBeGreaterThan(0);
  await page.keyboard.press('k');
  expect(await top()).toBe(0);
});

test('html プレビュー(iframe)内のクリックで右クリックメニューが閉じる', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');

  // 先に iframe 内を一度クリックしてフォーカスを移しておく。実機では右クリック自体が
  // フォーカスを移すため、メニューが開いた後のクリックでは親 window の blur が
  // 発火しない。この状態を作らないと、blur 経由で閉じてしまいバグを検出できない。
  await frame.locator('h1').click();

  await frame.locator('h1').click({ button: 'right' });
  const menu = page.locator('#md-context-menu');
  await expect(menu).toBeVisible();

  // メニュー外＝iframe 内の別の場所をクリック。親 document にはこの mousedown が
  // 届かないので、bindFrame の橋渡しが無いとメニューが開いたまま残る。
  await frame.locator('p').last().click();
  await expect(menu).toHaveCount(0);
});

test('コメントモードでサイドバーが Comment に置き換わり、抜けると元へ戻る', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  const body = page.locator('body');
  const panel = page.locator('.md-toc-panel');
  const cmLabel = page.locator('.md-toc-header [data-tab="comments"]');
  const outlineLabel = page.locator('.md-toc-header [data-tab="outline"]');
  // 入る前のサイドバー開閉状態（見出し数と幅次第）。抜けた後の復元を照合する。
  const wasOpen = !(await panel.evaluate((el) => el.classList.contains('hidden')));
  // モード外に Comment は存在しない（置き換えモデル。入口はピル/c/バッジ）。
  await expect(cmLabel).toBeHidden();

  // c でモードに入るとサイドバーがまるごと Comment に置き換わる（Outline のタイトルごと）。
  await page.keyboard.press('c');
  await expect(body).toHaveClass(/md-cmt-mode/);
  await expect(panel).not.toHaveClass(/hidden/);
  await expect(cmLabel).toBeVisible();
  await expect(outlineLabel).toBeHidden();
  await expect(page.locator('.md-cmt-side')).toBeVisible();

  // Esc で抜けると、入る前の状態（開いていれば Outline / 閉じていれば閉じる）に復元。
  await page.keyboard.press('Escape');
  await expect(body).not.toHaveClass(/md-cmt-mode/);
  await expect(cmLabel).toBeHidden();
  if (wasOpen) {
    await expect(panel).not.toHaveClass(/hidden/);
    await expect(outlineLabel).toBeVisible();
  } else {
    await expect(panel).toHaveClass(/hidden/);
  }

  // Comment 表示中の ⌘T は「Outline へ切替」＝モード終了（明示操作は復元より優先）。
  await page.keyboard.press('c');
  await expect(cmLabel).toBeVisible();
  await page.keyboard.press('Meta+t');
  await expect(body).not.toHaveClass(/md-cmt-mode/);
  await expect(outlineLabel).toBeVisible();
  await expect(panel).not.toHaveClass(/hidden/);
});
