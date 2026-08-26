// コメント（comment.js）。付ける・出す・巡回するの 3 つ。
//
// プレビューと raw / ソース表示で行の数え方が違う（フロントマターぶんずれる、
// 非 md はソースしか無い）ので、どの表示で付けたかを覚えて戻れるかが要。
const { test, expect } = require('@playwright/test');
const { SINGLE_URL, open, openFolder, nextFrames } = require('./helpers');

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

test('raw / ソース表示でも行にコメントできる', async ({ page }) => {
  // 「全部コピー」の中身を見たいので、クリップボード書き込みを捕まえておく
  // （WebKit ではクリップボードの読み出しができないため）。
  await page.addInitScript(() => {
    window.__copied = null;
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: (t) => { window.__copied = t; return Promise.resolve(); } },
    });
  });
  await openFolder(page);
  // 非 md（notes.txt）は通常表示がソースビュー。md の raw トグルと同じ DOM を通る。
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('.source-view')).toBeVisible();
  // ソースは表示した時点で 1 行 1 要素に包まれている（common.js の wrapSourceLines）。
  // かつてはコメントモード中だけ本文に重ねる別レイヤ（.md-src-rows）だったが、行その
  // ものを要素にしたので、モードに入る前から行ユニットが揃っている。
  await expect(page.locator('.md-src-row')).toHaveCount(7);

  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);

  // Copy ボタンはモード中も押せる（行ユニットやその装飾の下に隠れない）。
  const copyHit = await page.evaluate(() => {
    const btn = document.querySelector('.source-main .copy-btn');
    const r = btn.getBoundingClientRect();
    const el = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
    return el && el.className;
  });
  expect(copyHit).toContain('copy-btn');

  // 行ユニットが行の中身をそのまま持っていること（重ねるレイヤだった頃は座標の一致を
  // 測っていたが、いまは行の実テキストが要素の中身なので、中身で確かめる）。
  await expect(page.locator('.md-src-row[data-src-line="3"]'))
    .toContainText('位置合わせを測るための行');

  // 3 行目をクリックしてコメントを保存 → 行に色帯が付き、一覧に file:line で載る。
  await page.locator('.md-src-row[data-src-line="3"]').click();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('ここに質問');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.md-src-row[data-src-line="3"]')).toHaveClass(/md-cmt-marked/);
  await expect(page.locator('.md-cmt-side')).toContainText('notes.txt:3');

  // ドラッグで複数行をまとめて 1 コメントにできる（5-6 行目のレンジ）。
  const row5 = await page.locator('.md-src-row[data-src-line="5"]').boundingBox();
  const row6 = await page.locator('.md-src-row[data-src-line="6"]').boundingBox();
  await page.mouse.move(row5.x + 40, row5.y + row5.height / 2);
  await page.mouse.down();
  await page.mouse.move(row6.x + 40, row6.y + row6.height / 2);
  await page.mouse.up();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('この 2 行');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.md-cmt-side')).toContainText('notes.txt:5-6');

  // 引用は行レイヤ（空要素）ではなくソースの行そのものが入る。
  await page.locator('.md-cmt-side .md-cmt-btn-primary').click();
  const copied = await page.evaluate(() => window.__copied);
  expect(copied).toContain('- notes.txt:3');
  expect(copied).toContain('> 行コメント（.md-src-rows）の位置合わせを測るための行。');
  expect(copied).toContain('ここに質問');
  // レンジは空行も 1 行として引用に並ぶ（5 行目は空行）。
  expect(copied).toContain('- notes.txt:5-6\n>\n> 空行のあとの行。');

  // モードを抜けても 💬 バッジは押せる／ホバーで内容が出る（行レイヤ自体は通り抜ける）。
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
  await page.locator('.md-src-row[data-src-line="3"] .md-cmt-badge').hover();
  await expect(page.locator('.md-cmt-preview')).toContainText('ここに質問');
});

test('md の raw で付けたコメントは、プレビューでは本文に出ず一覧に残る', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  // raw（⌘R）は 1 行 1 ユニット。段落の途中の行（14 行目）を掴む。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.keyboard.press('c');
  await page.locator('.md-src-row[data-src-line="14"]').click();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('段落の 2 行目に質問');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.md-src-row[data-src-line="14"]')).toHaveClass(/md-cmt-marked/);

  // プレビューへ戻す。raw の行コメントをプレビュー側のブロック全体（段落・コード・
  // mermaid）へ落とすと錨として粗いので、本文には描かない（付けた表示にだけ出す）。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await expect(page.locator('p[data-src-line="13"]')).not.toHaveClass(/md-cmt-marked/);
  await expect(page.locator('#preview-pane .md-cmt-badge')).toHaveCount(0);
  // 一覧は横断インデックスなので消えない。行番号はソースの行のまま。
  await expect(page.locator('.md-cmt-side')).toContainText('a.md:14');

  // raw へ戻せば印も戻る（消えたのではなく、その表示に出していないだけ）。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.md-src-row[data-src-line="14"]')).toHaveClass(/md-cmt-marked/);
});

test('単一ファイルモードの raw でも行にコメントできる', async ({ page }) => {
  await open(page, SINGLE_URL);
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();

  await page.keyboard.press('c');
  await page.locator('.md-src-row[data-src-line="1"]').click();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('見出しの表記');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();

  // 単一ファイルモードのパスは MD_FILE_REL（cwd の外なので basename）。
  await expect(page.locator('.md-cmt-side')).toContainText('a.md:1');
  await expect(page.locator('.md-src-row[data-src-line="1"]')).toHaveClass(/md-cmt-marked/);
});

test('フロントマター付き md でも raw とプレビューの行番号が一致する', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'zz-frontmatter.md' }).click();
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('フロントマターの見出し');
  // レンダリング済みの行番号は「ファイルの行」（フロントマターの 4 行ぶんを含む）。
  await expect(page.locator('#preview-pane h1[data-src-line="6"]')).toBeVisible();

  // raw で 6 行目（見出し）にコメント → プレビューへ戻して同じ見出しに印が付くこと。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.keyboard.press('c');
  await page.locator('.md-src-row[data-src-line="6"]').click();
  await page.locator('.md-cmt-textarea').fill('見出しの表記ゆれ');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.md-cmt-side')).toContainText('zz-frontmatter.md:6');

  // プレビューへ戻す。印は付けた表示（raw）にだけ出るので見出しには付かないが、
  // ここで見たいのは行番号の一致——一覧の :6 と、プレビュー側の見出しの
  // data-src-line が同じ 6 を指していること（フロントマターの 4 行ぶんを含む）。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('フロントマターの見出し');
  await expect(page.locator('#preview-pane h1[data-src-line="6"]')).toBeVisible();
  await expect(page.locator('h1[data-src-line="6"]')).not.toHaveClass(/md-cmt-marked/);
});

test('n / p は付けたときの表示（raw / プレビュー）へ切り替えて飛ぶ', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.keyboard.press('c');

  // プレビューで 3 行目の段落にコメント。
  await page.locator('#preview-pane p[data-src-line="3"]').click();
  await page.locator('.md-cmt-textarea').fill('プレビュー側');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();

  // raw に切り替えて 14 行目（段落の 2 行目）にコメント。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.locator('.md-src-row[data-src-line="14"]').click();
  await page.locator('.md-cmt-textarea').fill('raw 側');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();

  // 一覧で raw 由来の 1 件だけに印が付く。
  await expect(page.locator('.md-cmt-item .md-cmt-tag')).toHaveCount(1);

  // プレビューへ戻してから巡回する。
  const raw = page.locator('.md-raw-toggle');
  await page.keyboard.press('Meta+r');
  await expect(raw).not.toHaveClass(/active/);

  // 1 件目（プレビュー由来）は raw を畳んだまま。
  await page.keyboard.press('n');
  await expect(raw).not.toHaveClass(/active/);
  await expect(page.locator('#preview-pane p[data-src-line="3"]')).toBeVisible();

  // 2 件目（raw 由来）へ進むと raw 表示へ切り替わって着地する。
  await page.keyboard.press('n');
  await expect(raw).toHaveClass(/active/);
  await expect(page.locator('.md-src-row[data-src-line="14"]')).toHaveClass(/md-cmt-marked/);

  // 1 件目へ戻ると raw は畳まれる。
  await page.keyboard.press('p');
  await expect(raw).not.toHaveClass(/active/);
  await expect(page.locator('#preview-pane p[data-src-line="3"]')).toHaveClass(/md-cmt-marked/);
});

test('本文が動いただけの mousemove では n / p の巡回対象を奪われない', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.keyboard.press('c');

  // 2 件付ける（巡回できる状態にする）。
  for (const [line, body] of [['3', 'ひとつめ'], ['7', 'ふたつめ']]) {
    await page.locator(`#preview-pane p[data-src-line="${line}"]`).click();
    await page.locator('.md-cmt-textarea').fill(body);
    await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  }

  // マウスを 3 行目の段落の上に置いてから、キーボードで巡回する。
  const box = await page.locator('#preview-pane p[data-src-line="3"]').boundingBox();
  const mx = Math.round(box.x + 20);
  const my = Math.round(box.y + box.height / 2);
  await page.mouse.move(mx, my);

  const review = page.locator('.md-cmt-item.review');
  await page.keyboard.press('n');
  await expect(review).toContainText('a.md:3');
  await page.keyboard.press('n');
  await expect(review).toContainText('a.md:7');

  // ジャンプで本文がスクロールすると、マウスが止まっていても mousemove が飛んでくる
  // （カーソルの下の要素が変わるため）。これで巡回対象を奪われないこと。
  await page.evaluate(([x, y]) => {
    const el = document.querySelector('#preview-pane p[data-src-line="3"]');
    el.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y, bubbles: true }));
  }, [mx, my]);
  // 一覧のハイライト更新は rAF スロットルなので、2 フレーム待ってから見る
  // （待たずに見ると「まだ変わっていないだけ」を通してしまう）。
  await nextFrames(page);
  await expect(review).toContainText('a.md:7');

  // 同じ座標の mousemove で「+」ハンドルを消してはいけない。消すとカーソル下の要素が
  // 変わってまた同じ座標の mousemove が来る——の繰り返しでハンドルがちらつく。
  await page.mouse.move(mx, my);
  await expect(page.locator('.md-cmt-handle')).toBeVisible();
  await page.evaluate(([x, y]) => {
    const el = document.querySelector('#preview-pane p[data-src-line="3"]');
    el.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y, bubbles: true }));
  }, [mx, my]);
  await nextFrames(page);
  await expect(page.locator('.md-cmt-handle')).toBeVisible();

  // 本当にマウスを動かしたときは今までどおりホバー先へ持ち替える（巡回対象は解除）。
  await page.evaluate(([x, y]) => {
    const el = document.querySelector('#preview-pane p[data-src-line="3"]');
    el.dispatchEvent(new MouseEvent('mousemove', { clientX: x + 7, clientY: y + 3, bubbles: true }));
  }, [mx, my]);
  await nextFrames(page);
  await expect(page.locator('.md-cmt-item.review')).toHaveCount(0);
});

test('錨が同じユニットに重なった 💬 バッジは 1 個にまとまる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.keyboard.press('c');
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();

  // 13 行目そのものへ 1 件と、13-14 行のレンジで 1 件。レンジの錨は範囲の先頭
  // ユニットなので、どちらも 13 行目の行ユニットに載る。
  const row13 = page.locator('.md-src-row[data-src-line="13"]');
  await row13.click();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('13 行目だけ');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();

  const b13 = await row13.boundingBox();
  const b14 = await page.locator('.md-src-row[data-src-line="14"]').boundingBox();
  await page.mouse.move(b13.x + 40, b13.y + b13.height / 2);
  await page.mouse.down();
  await page.mouse.move(b14.x + 40, b14.y + b14.height / 2);
  await page.mouse.up();
  await expect(page.locator('#md-cmt-popover')).toBeVisible();
  await page.locator('.md-cmt-textarea').fill('13-14 のレンジ');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();

  // 💬 バッジはモード外だけに出る（モード中は同じ場所にインライン埋め込みが出るので、
  // 中に入れると二重になる）。抜けてから数える。
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
  await expect(row13).toHaveClass(/md-cmt-marked/);
  // 錨が同じ要素なので、バッジは 2 個並ばず「💬2」の 1 個になる。
  await expect(row13.locator('.md-cmt-badge')).toHaveCount(1);
  await expect(row13.locator('.md-cmt-badge')).toHaveText('💬2');
});

test('別ファイルの raw コメントへ飛ぶと、raw のまま着地する', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md
  await page.keyboard.press(']'); // b.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し B');

  // b.md の raw で 1 行目にコメントしてから、プレビューへ戻して a.md へ移る。
  await page.keyboard.press('c');
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.locator('.md-src-row[data-src-line="1"]').click();
  await page.locator('.md-cmt-textarea').fill('b.md の raw から');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await page.keyboard.press('Meta+r');
  await page.keyboard.press('['); // a.md へ
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  // n で b.md の raw コメントへ。ファイルを開くのと raw の切り替えが二重フェッチに
  // ならず、raw の本文の上に着地すること（通常レンダリングに上書きされない）。
  await page.keyboard.press('n');
  await expect(page.locator('.md-raw-toggle')).toHaveClass(/active/);
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(page.locator('.md-src-row[data-src-line="1"]')).toHaveClass(/md-cmt-kbcursor/);
});
