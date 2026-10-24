// 本文まわりの土台（common.js / keyscroll.js / help.js / contextmenu.js）。
//
// オーバーレイの譲り合い（Esc が 1 回で 1 つだけ閉じる）、素キーのスクロール、
// ヘルプが keymap から作られていること、iframe 越しのクリックでメニューが閉じること。
// どれも「本文にフォーカスがある」という前提が崩れると一斉に死ぬ部分。
const { test, expect, openFolder } = require('./helpers');

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

test('? のヘルプが開き、Esc で閉じる（キー一覧は keymap から作られる）', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('?');
  const help = page.locator('#md-help-backdrop');
  await expect(help).toBeVisible();
  // 表の行がそのまま並ぶ（出し分けは無い）。
  await expect(help).toContainText('⌘P');
  await expect(help).toContainText('⌘B');
  await page.keyboard.press('Escape');
  await expect(help).toHaveCount(0);
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

// 本文はアプリの UI と同じ document にある。本文にタブ・ツリーの行・ヘッダに似せた
// 要素を書かれても、右クリックの対象は「今開いているファイル」のまま変わらないこと。
test('本文にタブやツリーの行に似せた要素があっても、右クリックの対象は開いているファイル', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md を開く
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  const current = await page.evaluate(() => window.MD_ROOT_DIR + '/a.md');

  await page.evaluate(() => {
    const body = document.querySelector('#preview-pane .markdown-body');
    body.insertAdjacentHTML('beforeend',
      '<div class="md-tab" data-path="/etc/fake-tab" id="fake-tab">タブもどき</div>' +
      '<div id="sidebar-header"><span id="fake-header">ヘッダもどき</span></div>' +
      '<div id="sidebar"><div class="tree-item" data-path="/etc" data-kind="dir" id="fake-tree">行もどき</div></div>');
  });

  for (const sel of ['#fake-tab', '#fake-header', '#fake-tree']) {
    await page.locator('#preview-pane ' + sel).click({ button: 'right' });
    const menu = page.locator('#md-context-menu');
    await expect(menu).toBeVisible();
    // タブ・フォルダのときにだけ出る行が無いこと。
    await expect(menu).not.toContainText('閉じる (⌘W)');
    await expect(menu).not.toContainText('ここを root にする');
    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await menu.locator('.md-context-menu-item', { hasText: '絶対パスをコピー' }).click();
    expect(await page.evaluate(() => window.__mdIpc.filter((m) => m.startsWith('menu:')))).toEqual(['menu:abs:' + current]);
  }
});
