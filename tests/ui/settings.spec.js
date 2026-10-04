// 設定タブ（settings.js / #38）。
//
// 設定はファイルではないタブとして本文ペインに描く。設定を持っているのは Rust 側
// なので、`MdSettings.push` を直接叩いて「渡された」状態を作り、操作の結果は
// **IPC に何が出たか**で確かめる。
const { test, expect } = require('@playwright/test');
const { openFolder, open, ONE_FILE_URL } = require('./helpers');

const SW = {
  minimal: ['#ffffff', '#37352f', '#2e7cd6', '#2f9e44', '#e03131'],
  paper: ['#faf6ee', '#33302a', '#1f6f6b', '#2a6f8c', '#9b3b2c'],
  nord: ['#2e3440', '#d8dee9', '#88c0d0', '#a3be8c', '#bf616a'],
  default: ['#ffffff', '#1f2328', '#3b82f6', '#1a7f37', '#cf222e'],
};
const THEMES = [
  { name: 'minimal', group: 'light', swatch: SW.minimal },
  { name: 'paper', group: 'light', swatch: SW.paper },
  { name: 'nord', group: 'dark', swatch: SW.nord },
  { name: 'default', group: 'auto', swatch: SW.default },
  { name: 'mine', group: 'user', swatch: null },
];

const ipc = (page) => page.evaluate(() => window.__mdIpc.filter((m) => m.startsWith('settings:')));
const settingsTab = (page) => page.locator('.md-tab[data-path="md:settings"]');
const card = (page, name) => page.locator(`.st-card[data-theme="${name}"]`);
const checked = (page) => page.locator('.st-card.is-on');
const navTo = (page, key) => page.locator(`.st-nav button[data-pane="${key}"]`).click();

/// Rust から状態が渡ってきた、という状態を作る。
async function push(page, over) {
  await page.evaluate((s) => window.MdSettings.push(s), Object.assign({
    theme: 'paper',
    defaultTheme: 'default',
    autoDarkBg: '#0d1117',
    themes: THEMES,
    windowSize: null,
    defaultWindowSize: { w: 1280, h: 700 },
  }, over || {}));
}

/// ⌘, で設定タブを開き、状態が届いたところまで進める。
async function openSettings(page, over) {
  await openFolder(page);
  await page.keyboard.press('Meta+Comma');
  await expect(page.locator('.st-layout')).toBeVisible();
  await push(page, over);
}

// ── タブとしての出入り ───────────────────────────────────────

test('⌘, と歯車で「設定」タブが開き、2 回目は増えずにそこへ移る', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('Meta+Comma');
  await expect(settingsTab(page)).toHaveClass(/active/);
  await expect(settingsTab(page).locator('.md-tab-name')).toHaveText('設定');
  // 前に出すたびに写しを取り直す。
  expect(await ipc(page)).toEqual(['settings:get']);

  await page.locator('#tabbar-gear').click();
  await expect(settingsTab(page)).toHaveCount(1);
  // 設定は画面を覆わないので、本文の素キーは止まらない。
  expect(await page.evaluate(() => window.MdCommon.isOverlayOpen())).toBe(false);
});

test('ファイルのタブと行き来でき、⌘W で閉じられる', async ({ page }) => {
  await open(page, ONE_FILE_URL);
  await page.keyboard.press('Meta+Comma');
  await expect(page.locator('.st-layout')).toBeVisible();

  // ファイルのタブへ戻ると本文が出る。
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('#preview-pane .markdown-body')).toBeVisible();
  await expect(page.locator('.st-layout')).toHaveCount(0);

  // 設定へ戻って ⌘W で閉じる。
  await settingsTab(page).click();
  await expect(page.locator('.st-layout')).toBeVisible();
  await page.keyboard.press('Meta+w');
  await expect(settingsTab(page)).toHaveCount(0);
  await expect(page.locator('#preview-pane .markdown-body')).toBeVisible();
  // 一時ファイルを持たないので、閉じたことを Rust へ知らせない。
  expect(await page.evaluate(() => window.__mdIpc.filter((m) => m.startsWith('closed:md:')))).toEqual([]);
});

test('⌘P を開いている間は ⌘, で裏だけが設定に切り替わらない', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press('Meta+p');
  await expect(page.locator('#md-pal-backdrop')).toBeVisible();
  await page.keyboard.press('Meta+Comma');
  await expect(settingsTab(page)).toHaveCount(0);
});

test('設定の項目名は ⌘T のアウトラインに見出しとして並ばない', async ({ page }) => {
  await openSettings(page);
  await expect(page.locator('#preview-pane h1, #preview-pane h2, #preview-pane h3')).toHaveCount(0);
});

test('設定タブではファイル向けの機能が黙る（raw / diff のボタン・右クリックのパス系）', async ({ page }) => {
  await open(page, ONE_FILE_URL);
  await expect(page.locator('.md-raw-toggle')).toBeVisible();
  await page.keyboard.press('Meta+Comma');
  await expect(page.locator('.st-layout')).toBeVisible();
  await expect(page.locator('.md-raw-toggle')).toBeHidden();
  await expect(page.locator('.md-diff-toggle')).toBeHidden();
  // ⌘R / ⌘D を押しても設定画面のまま（取りに行く対象が無い）。
  await page.keyboard.press('Meta+r');
  await page.keyboard.press('Meta+d');
  await expect(page.locator('.st-layout')).toBeVisible();

  await settingsTab(page).click({ button: 'right' });
  const menu = page.locator('#md-context-menu');
  await expect(menu.getByText('閉じる (⌘W)')).toBeVisible();
  await expect(menu.getByText('絶対パスをコピー')).toHaveClass(/disabled/);
  await page.keyboard.press('Escape');

  // ファイルのタブへ戻ればボタンも戻る。
  await page.keyboard.press('Shift+Tab');
  await expect(page.locator('.md-raw-toggle')).toBeVisible();
  await expect(page.locator('.md-diff-toggle')).toBeVisible();
});

// ── テーマ ───────────────────────────────────────────────────

test('テーマは組ごとのカードで並び、いまのテーマに印が付く', async ({ page }) => {
  await openSettings(page);
  await expect(page.locator('.st-group')).toHaveText(['ライト', 'ダーク', 'OS の設定に追従', 'ユーザー']);
  await expect(page.locator('.st-card')).toHaveCount(5);
  await expect(checked(page)).toHaveAttribute('data-theme', 'paper');
  // 見本は theme.rs の配色そのまま。地が bg。
  await expect(card(page, 'nord').locator('.st-sw')).toHaveCSS('background-color', 'rgb(46, 52, 64)');
  // 配色を持たないユーザーテーマは「見本なし」。
  await expect(card(page, 'mine').locator('.st-sw')).toHaveText('見本なし');
});

test('カードを押すと印が移り、切り替えを頼む', async ({ page }) => {
  await openSettings(page);
  await card(page, 'nord').click();
  await expect(checked(page)).toHaveAttribute('data-theme', 'nord');
  await card(page, 'minimal').click();
  await expect(checked(page)).toHaveAttribute('data-theme', 'minimal');
  // いまのテーマをもう一度押しても頼まない。
  await card(page, 'minimal').click();
  expect(await ipc(page)).toEqual(['settings:get', 'settings:theme:nord', 'settings:theme:minimal']);
});

test('テーマには既定に戻すを出さない（カードを押せば戻せる）', async ({ page }) => {
  await openSettings(page);
  await expect(page.locator('.st-reset')).toHaveCount(0);
});

test('設定のテーマが一覧に無ければそう添えて、default に印を付ける', async ({ page }) => {
  await openSettings(page, { theme: 'gone' });
  await expect(page.locator('.st-note')).toContainText('「gone」が見つからない');
  await expect(checked(page)).toHaveAttribute('data-theme', 'default');
});

test('Rust から届いたテーマの層でページの見た目が変わる', async ({ page }) => {
  await openFolder(page);
  await page.evaluate(() => window.MdSettings.applyTheme('body { background: rgb(1, 2, 3) !important; }', 'dark'));
  await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(1, 2, 3)');
  expect(await page.evaluate(() => window.MD_APPEARANCE)).toBe('dark');
});

// ── ウィンドウ ────────────────────────────────────────────────

test('初期サイズは保存してある値を出し、無ければ既定を出す', async ({ page }) => {
  await openSettings(page);
  await navTo(page, 'window');
  await expect(page.locator('#st-width')).toHaveValue('1280');
  await expect(page.locator('#st-height')).toHaveValue('700');
  // 既定のままでも「既定に戻す」は出ている。
  await expect(page.locator('.st-reset button')).toHaveAttribute('title', '既定は 1280 × 700');

  await push(page, { windowSize: { w: 1440, h: 900 } });
  await expect(page.locator('#st-width')).toHaveValue('1440');
});

test('打ち込んだ大きさは確定したときに 1 回だけ頼み、数字以外は打てない', async ({ page }) => {
  await openSettings(page);
  await navTo(page, 'window');
  const w = page.locator('#st-width');
  await w.fill('1600');
  // 打っている途中では送らない。
  expect(await ipc(page)).toEqual(['settings:get']);
  await w.press('Enter');
  expect(await ipc(page)).toEqual(['settings:get', 'settings:window-size:1600x700']);

  // 読めるかどうかは Rust の `parse_window_size` だけが決める。ここで数字に直して
  // 送ると `12abc` が 12 に化けて保存される。読めなければ Rust が今の状態を
  // 送り返し（main.rs の SettingsRequest::parse）、欄は保存してある値へ戻る。
  // 数字以外は打てない（`12e80` は `1280` になる）。5 桁で切れる。
  await w.fill('');
  await w.type('12e80');
  await expect(w).toHaveValue('1280');
  await w.fill('');
  await w.type('9999999');
  await expect(w).toHaveValue('99999');
  // 上限を超える値は Rust が断って今の状態を送り返す（main.rs の SettingsRequest::parse）。
  // 確定した欄はフォーカスを外すので、送り返された値で上書きされる。
  await w.press('Enter');
  expect(await ipc(page)).toEqual([
    'settings:get', 'settings:window-size:1600x700', 'settings:window-size:99999x700',
  ]);
  await expect(w).not.toBeFocused();
  await push(page, { windowSize: { w: 1440, h: 900 } });
  await expect(w).toHaveValue('1440');
});

test('入力欄では素キーが本文の操作に化けない', async ({ page }) => {
  await openSettings(page);
  await navTo(page, 'window');
  await page.locator('#st-width').fill('');
  await page.locator('#st-width').type('12');
  // `c`（コメントモード）にならない。数字以外なので欄にも入らない。
  await page.locator('#st-width').type('c');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
  await expect(page.locator('#st-width')).toHaveValue('12');
});

test('打ちかけの入力は、その間に届いた写しで消えない', async ({ page }) => {
  await openSettings(page);
  await navTo(page, 'window');
  const w = page.locator('#st-width');
  await w.fill('16');
  // 「いまの大きさに設定する」の返事や `md theme` の知らせで写しが届くことがある。
  await push(page, { windowSize: { w: 1440, h: 900 } });
  await expect(w).toHaveValue('16');
  await expect(w).toBeFocused();
  // 打っていない欄は届いた値に変わる。
  await expect(page.locator('#st-height')).toHaveValue('900');
});

test('打ちかけのままタブを移っても、打った値は保存を頼む', async ({ page }) => {
  await open(page, ONE_FILE_URL);
  await page.keyboard.press('Meta+Comma');
  await push(page);
  await navTo(page, 'window');
  await page.locator('#st-width').fill('1500');
  // タブは mousedown で preventDefault するので、欄のフォーカスが外れず change も起きない。
  await page.locator('.md-tab').first().click();
  await expect(page.locator('.st-layout')).toHaveCount(0);
  expect(await ipc(page)).toEqual(['settings:get', 'settings:window-size:1500x700']);
});

test('全角の数字は半角に直り、途中に打った文字を消してもカーソルは飛ばない', async ({ page }) => {
  await openSettings(page);
  await navTo(page, 'window');
  const w = page.locator('#st-width');
  await w.fill('');
  await w.focus();
  await page.keyboard.insertText('１６００');
  await expect(w).toHaveValue('1600');

  await w.evaluate((el) => el.setSelectionRange(1, 1));
  await page.keyboard.type('a');
  await expect(w).toHaveValue('1600');
  expect(await w.evaluate((el) => el.selectionStart)).toBe(1);
});

test('Esc で打ちかけを捨てて、保存してある値へ戻す', async ({ page }) => {
  await openSettings(page, { windowSize: { w: 1440, h: 900 } });
  await navTo(page, 'window');
  const w = page.locator('#st-width');
  await w.fill('2000');
  await w.press('Escape');
  await expect(w).toHaveValue('1440');
  await expect(w).not.toBeFocused();
  expect(await ipc(page)).toEqual(['settings:get']);
});

test('写しが届いても、打っている欄のカーソル位置は動かない', async ({ page }) => {
  await openSettings(page);
  await navTo(page, 'window');
  const w = page.locator('#st-width');
  await w.fill('1600');
  await w.evaluate((el) => el.setSelectionRange(2, 2));
  await push(page, { windowSize: { w: 1440, h: 900 } });
  await page.keyboard.type('7');
  await expect(w).toHaveValue('16700');
});

test('いまの大きさに設定する・既定に戻すは、それぞれ Rust に頼む', async ({ page }) => {
  await openSettings(page, { windowSize: { w: 1440, h: 900 } });
  await navTo(page, 'window');
  await page.getByRole('button', { name: 'いまの大きさに設定する' }).click();
  await page.locator('.st-reset button').click();
  // 大きさはページが測って送る（Rust の inner_size は起動時の値のまま動かない）。
  const vp = page.viewportSize();
  expect(await ipc(page)).toEqual([
    'settings:get', `settings:window-size:${vp.width}x${vp.height}`, 'settings:window-size:reset',
  ]);
});
