// コメント（comment.js）。付ける・出す・巡回するの 3 つ。
//
// プレビューと raw / ソース表示で行の数え方が違う（フロントマターぶんずれる、
// 非 md はソースしか無い）ので、どの表示で付けたかを覚えて戻れるかが要。
const fs = require('fs');
const path = require('path');
const { test, expect } = require('@playwright/test');
const { id, openFolder, nextFrames, treeItem } = require('./helpers');

/// 追跡済みのフィクスチャは常にクリーンで差分が空になり、行数も閾値に遠い。
/// 錨れない表示（git 差分 / 巨大ソース）を作るには、その場でファイルを置くしかない。
/// 未追跡ファイルは差分の相手がいないので「全行が追加」になる。
function withFixture(name, body, fn) {
  const file = path.join(__dirname, '../ui-fixtures', name);
  fs.writeFileSync(file, body);
  return fn(name).finally(() => fs.rmSync(file, { force: true }));
}

/// ソース表示の 💬 バッジ。行ではなく、その行の番号のセル（横スクローラの外の
/// ガター）に載る——行の中に置くとコードの先頭文字に重なるため。
function srcBadge(page, line) {
  return page.locator('#preview-pane .source-gutter-rows .src-num')
    .nth(line - 1).locator('.md-cmt-badge');
}

/// #preview-pane が確実にスクロールできる高さを作る（本文が 1 画面に収まると
/// j を押しても動かず、スクロールへ戻ったことを見分けられないため）。
async function padPane(page) {
  await page.evaluate(() => {
    const pad = document.createElement('div');
    pad.style.height = '3000px';
    document.querySelector('#preview-pane .markdown-body').appendChild(pad);
  });
}

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
  // パネルは root 相対で出すが、クリップボードは絶対パス（貼り先は root を知らない）。
  const notes = id(page, 'notes.txt');
  expect(copied).toContain(`- ${notes}:3`);
  expect(copied).toContain('> 行コメント（.md-src-rows）の位置合わせを測るための行。');
  expect(copied).toContain('ここに質問');
  // レンジは空行も 1 行として引用に並ぶ（5 行目は空行）。
  expect(copied).toContain(`- ${notes}:5-6\n>\n> 空行のあとの行。`);

  // モードを抜けても 💬 バッジは押せる／ホバーで内容が出る（行レイヤ自体は通り抜ける）。
  // バッジが載るのは行ではなく行番号のセル（横スクローラの外のガター）。
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
  await srcBadge(page, 3).hover();
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

test('X は全ファイルのコメントを一度に消し、消した件数を返す', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']'); // a.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.keyboard.press('c');

  // a.md に 2 件、b.md に 1 件（一覧はファイルを跨いで溜まる）。
  for (const [line, body] of [['3', 'ひとつめ'], ['7', 'ふたつめ']]) {
    await page.locator(`#preview-pane p[data-src-line="${line}"]`).click();
    await page.locator('.md-cmt-textarea').fill(body);
    await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  }
  await page.keyboard.press(']'); // b.md
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し B');
  await page.locator('#preview-pane p[data-src-line="3"]').click();
  await page.locator('.md-cmt-textarea').fill('b.md にも');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.md-cmt-item')).toHaveCount(3);

  // X は対象を選ばない（カーソルはコメントの無い行に置いておく）。
  await page.keyboard.press('k');
  await page.keyboard.press('Shift+X');
  await expect(page.locator('.md-cmt-item')).toHaveCount(0);
  await expect(page.locator('.md-toast')).toHaveText('3 件を全消去しました');
  // 本文の印（マーカー・💬・埋め込み）も残らない。
  await expect(page.locator('.md-cmt-marked')).toHaveCount(0);
  await expect(page.locator('.md-cmt-badge')).toHaveCount(0);
  await expect(page.locator('.md-cmt-embed')).toHaveCount(0);

  // 空で押しても壊れない（件数ではなく「まだありません」を返す）。
  await page.keyboard.press('Shift+X');
  await expect(page.locator('.md-toast')).toHaveText('コメントはまだありません');
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
  await expect(srcBadge(page, 13)).toHaveCount(1);
  await expect(srcBadge(page, 13)).toHaveText('💬2');
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

test('html 表示（iframe）の中の行にコメントを付けられる', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  // ツリークリックでフォーカスがサイドバーへ移っているので、素キーが本文へ届くよう戻す。
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  // 配信時にブロック要素へ行が刻まれている（request.rs の inject_src_lines）ので、
  // 「付けられない」案内ではなく件数ベースのキー案内が出る。
  await expect(page.locator('.md-cmt-hint')).toContainText('j / k で移動');

  // 枠は親のレイヤに浮く。他人のページなので DOM には触らず、位置だけを重ねる。
  const cursor = page.locator('.md-cmt-frame-mark[data-state="kbcursor"]');
  await expect(cursor).toBeVisible();
  // 本文側の class は付かない（付けたら他人の DOM を書き換えたことになる）。
  await expect(frame.locator('.md-cmt-kbcursor')).toHaveCount(0);

  // 枠は iframe の中の見出しに重なっている（親座標へ換算できている）。
  const h1Box = await frame.locator('h1').boundingBox();
  const cursorBox = await cursor.boundingBox();
  expect(Math.abs(cursorBox.y - h1Box.y)).toBeLessThan(8);

  // j で次のユニット（2 つめの段落）へ。iframe の中をスクロールするのではなく、
  // カーソルが動く。
  await page.keyboard.press('j');
  const pBox = await frame.locator('p').first().boundingBox();
  await expect.poll(async () => {
    const b = await cursor.boundingBox();
    return Math.abs(b.y - pBox.y);
  }).toBeLessThan(8);

  // Enter でその行にコメント。錨は html ファイルの 6 行目（フィクスチャの最初の <p>）。
  await page.keyboard.press('Enter');
  await page.locator('.md-cmt-textarea').fill('iframe の中の段落へ');
  await page.keyboard.press('Meta+Enter');
  await expect(page.locator('.md-cmt-loc').first()).toContainText('page.html:6');

  // コメント済みの塗りは iframe の中の ::highlight() に入る（DOM は変えない）。
  await expect.poll(() => page.evaluate(() => {
    const w = document.querySelector('iframe.html-frame').contentWindow;
    return !!(w.CSS && w.CSS.highlights && w.CSS.highlights.has('md-cmt-marked'));
  })).toBe(true);
  await expect(frame.locator('.md-cmt-marked')).toHaveCount(0);

  // モードを抜ければ枠は消える。
  await page.keyboard.press('Escape');
  await expect(cursor).toBeHidden();
});

test('html 表示から md へ戻ると、iframe 用の枠が残らない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).click();
  await expect(page.frameLocator('iframe.html-frame').locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  const cursor = page.locator('.md-cmt-frame-mark[data-state="kbcursor"]');
  await expect(cursor).toBeVisible();

  // md へ移ると本文の相手が変わる。枠は親のレイヤに浮いているので、渡す前に消さないと
  // md の上へ取り残される。
  await page.locator('.tree-item', { hasText: 'a.md' }).first().click();
  await expect(page.locator('#preview-pane .markdown-body')).toBeVisible();
  await expect(cursor).toBeHidden();
  // md 側は今までどおり本文の class で指す。
  await expect(page.locator('.md-cmt-kbcursor')).toHaveCount(1);
});

test('iframe にフォーカスがあってもコメントのキーが効く', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');

  // html を読んでいる間はフォーカスが iframe の中にあるのが普通。ここから始める。
  await frame.locator('h1').click();

  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  const cursor = page.locator('.md-cmt-frame-mark[data-state="kbcursor"]');
  await expect(cursor).toBeVisible();

  // j はカーソル移動として親へ渡る。iframe の中をスクロールしてしまうと、枠だけ置いて
  // 動かせなくなる。
  const top = () => page.evaluate(() => {
    const d = document.querySelector('iframe.html-frame').contentDocument;
    return (d.scrollingElement || d.documentElement).scrollTop;
  });
  const before = (await cursor.boundingBox()).y;
  await page.keyboard.press('j');
  await expect.poll(async () => (await cursor.boundingBox()).y).toBeGreaterThan(before);
  expect(await top()).toBe(0);

  // d（ページ送り）は譲らない。モードは本文の素キーを止めない。
  await page.keyboard.press('d');
  await expect.poll(top).toBeGreaterThan(0);

  // 入口（c）だけ渡して出口を渡さないと、中から入ったきり出られない。モードは画面を
  // 覆わないので、親の「オーバーレイが開いているか」には数えられない。
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).not.toHaveClass(/md-cmt-mode/);
});

test('iframe の中をクリック / ドラッグしてコメントを付けられる', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);

  // クリック 1 発で、その行に付く。
  await frame.locator('p').first().click();
  await page.locator('.md-cmt-textarea').fill('クリックで付けた');
  await page.keyboard.press('Meta+Enter');
  await expect(page.locator('.md-cmt-loc').first()).toContainText('page.html:6');

  // ドラッグで見出しから段落までのレンジ。iframe の中の座標も親の座標系で拾える。
  const h1 = await frame.locator('h1').boundingBox();
  const p1 = await frame.locator('p').first().boundingBox();
  await page.mouse.move(h1.x + 5, h1.y + h1.height / 2);
  await page.mouse.down();
  await page.mouse.move(p1.x + 5, p1.y + p1.height / 2, { steps: 5 });
  await page.mouse.up();
  await page.locator('.md-cmt-textarea').fill('見出しから段落まで');
  await page.keyboard.press('Meta+Enter');
  await expect(page.locator('.md-cmt-loc').first()).toContainText('page.html:5-6');
});

test('モード中は iframe の中のリンクで遷移しない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await frame.locator('p').first().evaluate((p) => {
    const a = p.ownerDocument.createElement('a');
    a.href = '#anchor';
    a.textContent = 'リンク';
    p.appendChild(a);
  });
  const url = () => page.evaluate(() =>
    document.querySelector('iframe.html-frame').contentDocument.URL);
  const before = await url();

  await page.keyboard.press('c');
  await frame.locator('p a').click();
  // 遷移せず、代わりにその行のコメント入力が開く。
  await expect(page.locator('.md-cmt-textarea')).toBeVisible();
  expect(await url()).toBe(before);
});

test('html から別の html へ移ると、カーソルが前のページを指したままにならない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  const cursor = page.locator('.md-cmt-frame-mark[data-state="kbcursor"]');
  await expect(cursor).toBeVisible();
  await page.keyboard.press('j');
  const onFirstPage = (await cursor.boundingBox()).y;

  // 別の html へ。iframe の中の要素は iframe ごと外れても isConnected を返し続けるので、
  // それで「宙に浮いたか」を見ていると、前のページの位置に枠が残ったままになる。
  await treeItem(page, 'sub').click();
  await treeItem(page, 'sub/page.html').click();
  await expect(frame.locator('body')).toContainText('サブフォルダ');

  // 新しいページの先頭ユニットへ置き直される（前の位置ではない）。
  await expect(cursor).toBeVisible();
  await expect.poll(async () => (await cursor.boundingBox()).y).not.toBe(onFirstPage);
  const h1 = await frame.locator('h1').boundingBox();
  expect(Math.abs((await cursor.boundingBox()).y - h1.y)).toBeLessThan(8);
});

test('html 表示でも、入力欄は iframe の中のスクロールに付いてくる', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  await frame.locator('p').first().click();
  const pop = page.locator('#md-cmt-popover');
  await expect(pop).toBeVisible();
  const before = (await pop.boundingBox()).y;

  // iframe の中のスクロールは親の document には届かない。host 経由で伝えないと、
  // 本文だけ動いて入力欄が画面に貼り付く。
  await page.evaluate(() => {
    const d = document.querySelector('iframe.html-frame').contentDocument;
    (d.scrollingElement || d.documentElement).scrollTop = 300;
  });
  await expect.poll(async () => (await pop.boundingBox()).y).toBeLessThan(before - 200);
});

test('コメントの無いユニットへホバーを移すとプレビューが閉じる', async ({ page }) => {
  await openFolder(page);
  await page.keyboard.press(']');
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');

  await page.keyboard.press('c');
  await page.keyboard.press('Enter');
  await page.locator('.md-cmt-textarea').fill('見出しに一言');
  await page.keyboard.press('Meta+Enter');
  await page.keyboard.press('Escape');

  const marked = page.locator('#preview-pane .md-cmt-marked').first();
  await marked.hover();
  await expect(page.locator('.md-cmt-preview')).toBeVisible();

  // 隙間なく並ぶユニットを渡り歩くとき、mouseout は「ユニットの外へ出たか」しか
  // 見ていない。コメントの無いユニットへ入った側で閉じないと出たままになる。
  const bare = page.locator('#preview-pane [data-src-line]:not(.md-cmt-marked)').first();
  await bare.hover();
  await expect(page.locator('.md-cmt-preview')).toHaveCount(0);
});

test('html 表示の 💬 バッジは本文ではなく枠と同じレイヤに出る', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  await page.keyboard.press('Enter');
  await page.locator('.md-cmt-textarea').fill('見出しへ');
  await page.keyboard.press('Meta+Enter');

  // 埋め込みカードを出せない相手なので、モード中もバッジが唯一の目印として残る。
  const badge = page.locator('.md-cmt-frame-overlay .md-cmt-badge');
  await expect(badge).toHaveCount(1);
  // 他人の DOM には挿さない。
  await expect(frame.locator('.md-cmt-badge')).toHaveCount(0);
  await expect(frame.locator('.md-cmt-embed')).toHaveCount(0);

  // 錨の右上に乗る。
  const h1 = await frame.locator('h1').boundingBox();
  const b = await badge.boundingBox();
  expect(Math.abs(b.y - h1.y)).toBeLessThan(12);
  expect(b.x).toBeGreaterThan(h1.x + h1.width / 2);

  // オーバーレイはクリックを透かすが、バッジだけは受け取る。
  await badge.click();
  await expect(page.locator('.md-cmt-textarea')).toHaveValue('見出しへ');
});

test('html のコメントは raw とレンダリングを行き来して着地する', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  // ① レンダリング表示で見出し（5 行目）へ。
  await page.keyboard.press('c');
  await page.keyboard.press('Enter');
  await page.locator('.md-cmt-textarea').fill('レンダリング側');
  await page.keyboard.press('Meta+Enter');
  await expect(page.locator('.md-cmt-side')).toContainText('page.html:5');

  // ② ⌘R で raw。html も raw トグルを持つので、同じファイルの第 2 の表示になる。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.locator('.md-src-row[data-src-line="9"]').click();
  await page.locator('.md-cmt-textarea').fill('raw 側');
  await page.keyboard.press('Meta+Enter');
  await expect(page.locator('.md-cmt-side')).toContainText('page.html:9');
  // raw で付けた印は raw の行に付く。
  await expect(page.locator('.md-src-row[data-src-line="9"]')).toHaveClass(/md-cmt-marked/);

  // ③ n で巡回すると、付けた表示へ戻してから着地する。5 行目はレンダリング側なので
  //    iframe が戻り、枠がその見出しに乗る。
  await page.keyboard.press('n');
  await expect(page.locator('iframe.html-frame')).toBeVisible();
  const cursor = page.locator('.md-cmt-frame-mark[data-state="kbcursor"]');
  await expect(cursor).toBeVisible();
  const h1 = await frame.locator('h1').boundingBox();
  await expect.poll(async () => {
    const b = await cursor.boundingBox();
    return Math.abs(b.y - h1.y);
  }).toBeLessThan(12);

  // ④ もう一度 n で raw 側のコメントへ。raw に切り替わって行に着地する。
  await page.keyboard.press('n');
  await expect(page.locator('.source-view')).toBeVisible();
  await expect(page.locator('.md-src-row[data-src-line="9"]')).toHaveClass(/md-cmt-kbcursor/);
});

test('html から md へ戻ると、iframe 用の 💬 バッジも残らない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  await expect(page.frameLocator('iframe.html-frame').locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  await page.keyboard.press('Enter');
  await page.locator('.md-cmt-textarea').fill('置き去りにされないこと');
  await page.keyboard.press('Meta+Enter');
  const badge = page.locator('.md-cmt-frame-overlay .md-cmt-badge');
  await expect(badge).toHaveCount(1);

  // バッジも枠と同じで親のレイヤに浮いている。渡す前に剥がさないと md の上に残り、
  // 押すと外れた文書の要素を錨に編集欄が開く（矩形が全部 0 なので左上へ飛ぶ）。
  await page.locator('.tree-item', { hasText: 'a.md' }).first().click();
  await expect(page.locator('#preview-pane .markdown-body')).toBeVisible();
  await expect(badge).toHaveCount(0);
});

test('html 表示でも 💬 バッジのホバーでコメントが読める', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  await expect(page.frameLocator('iframe.html-frame').locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  await page.keyboard.press('c');
  await page.keyboard.press('Enter');
  await page.locator('.md-cmt-textarea').fill('バッジから読める');
  await page.keyboard.press('Meta+Enter');
  await page.keyboard.press('Escape');

  // バッジは親のオーバーレイに居るので「本文の上か」では拾えない。指している行を
  // 自分で持っているので、そちらから引く。
  await page.locator('.md-cmt-frame-overlay .md-cmt-badge').hover();
  await expect(page.locator('.md-cmt-preview')).toContainText('バッジから読める');
});

test('iframe の中で履歴だけ書き換えたページには、今までどおり付けられる', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  // pushState は Document を作り直さない。刻んだ行はそのまま生きているので、
  // これを「リンク先へ移った」と数えると SPA で錨れなくなる。
  await page.evaluate(() => {
    document.querySelector('iframe.html-frame').contentWindow.history.replaceState(null, '', '?tab=2');
  });

  await page.keyboard.press('c');
  await expect(page.locator('.md-cmt-hint')).toContainText('j / k で移動');
  await expect(page.locator('.md-cmt-frame-mark[data-state="kbcursor"]')).toBeVisible();
});

test('git 差分ではコメントモード中でも j / k がスクロールし、付けられない旨が出る', async ({ page }) => {
  await withFixture('zz-untracked.md', '# 未追跡\n\n差分では全行が追加になる。\n', async (name) => {
    await openFolder(page);
    await page.locator('.tree-item', { hasText: name }).click();
    await expect(page.locator('#preview-pane')).toContainText('未追跡');
    // ツリークリックでフォーカスがサイドバーへ移っているので、素キーが本文へ届くよう戻す。
    await page.evaluate(() => document.activeElement && document.activeElement.blur());
    await page.keyboard.press('Meta+d');
    await expect(page.locator('.diff-source')).toBeVisible();

    await page.keyboard.press('c');
    await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
    await expect(page.locator('.md-cmt-hint')).toContainText('git 差分にコメントはできません');
    await expect(page.locator('.md-cmt-kbcursor')).toHaveCount(0);

    await padPane(page);
    const top = () => page.evaluate(() => document.getElementById('preview-pane').scrollTop);
    expect(await top()).toBe(0);
    await page.keyboard.press('j');
    await expect.poll(top).toBeGreaterThan(0);
    await page.keyboard.press('k');
    await expect.poll(top).toBe(0);
  });
});

test('1 万行超のソースではコメントモード中でも j / k がスクロールし、付けられない旨が出る', async ({ page }) => {
  const big = Array.from({ length: 10500 }, (_, i) => 'line ' + (i + 1)).join('\n') + '\n';
  await withFixture('zz-big.txt', big, async (name) => {
    await openFolder(page);
    await page.locator('.tree-item', { hasText: name }).click();
    await expect(page.locator('.source-main')).toBeVisible();
    await page.evaluate(() => document.activeElement && document.activeElement.blur());
    // 閾値超えなので wrapSourceLines が行を包まない＝錨る行ユニットが 1 個も無い。
    expect(await page.locator('#preview-pane [data-src-line]').count()).toBe(0);

    await page.keyboard.press('c');
    await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
    await expect(page.locator('.md-cmt-hint')).toContainText('大きなファイルなので行コメントはできません');
    // 案内はサイドバーの常設ヒントに一本化した（トーストは出さない）。
    await expect(page.locator('.md-toast')).toHaveCount(0);

    const top = () => page.evaluate(() => document.getElementById('preview-pane').scrollTop);
    expect(await top()).toBe(0);
    await page.keyboard.press('j');
    await expect.poll(top).toBeGreaterThan(0);
  });
});

// 差し替え直後の読み位置の追随（common.js の restoreAnchor）と、コメントのジャンプの
// 取り合い。landOn は scrollIntoView なので wheel も pointerdown も keydown も出さず、
// 追随の解放条件では気づけない。しかも表示を切り替えてから飛ぶ経路
// （applyView → 差し替え → landWhenReady）では追随を張った後にジャンプが来るので、
// 明示的に手を離させないと、高さが落ち着いた時点で元の位置へ引き戻される。
//
// scroll anchoring はここでは切らない。mermaid の伸びを補うのはエンジンの仕事で、
// 切ると「追随が引き戻した」のか「伸びに流された」のか区別できなくなる。
test('表示を切り替えて飛ぶコメントジャンプが、読み位置の追随に引き戻されない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'zz-mermaid.md' }).click();
  await expect(page.locator('#preview-pane .mermaid svg').first()).toBeVisible();
  await page.keyboard.press('c');

  // プレビュー側で、mermaid よりずっと下の項目にコメントを付ける。
  const target = page.locator('#preview-pane li[data-src-line]').last();
  const line = await target.getAttribute('data-src-line');
  await target.scrollIntoViewIfNeeded();
  await target.click();
  await page.locator('.md-cmt-textarea').fill('下の方');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();

  // raw へ切り替えて、いちばん上まで戻る（飛ぶ先が画面外になる位置）。
  await page.keyboard.press('Meta+r');
  await expect(page.locator('.source-view')).toBeVisible();
  await page.evaluate(() => { document.getElementById('preview-pane').scrollTop = 0; });

  // n で巡回 → 付けた表示（プレビュー）へ戻して着地する。
  await page.keyboard.press('n');
  await expect(page.locator('.source-view')).toHaveCount(0);
  await expect(page.locator('.md-cmt-flash')).toHaveCount(1);

  // スムーススクロールが終わるまで待ってから着地位置を測る。
  const stable = () => page.waitForFunction(() => {
    const p = document.getElementById('preview-pane');
    if (window.__t === p.scrollTop) return (window.__n = (window.__n || 0) + 1) >= 3;
    window.__t = p.scrollTop; window.__n = 0; return false;
  }, null, { polling: 50, timeout: 5000 });
  const top = (l) => page.evaluate((x) => {
    const p = document.getElementById('preview-pane');
    const u = p.querySelector('[data-src-line="' + x + '"]');
    return Math.round(u.getBoundingClientRect().top - p.getBoundingClientRect().top);
  }, l);
  await stable();
  const landed = await top(line);

  // 着地してから本文の高さを動かす。追随は ResizeObserver で起きるので、これが
  // 「mermaid が着地より後に描き終わった」と同じ条件になる（Playwright では lib が
  // すでに載っているぶん描画が速く、着地より前に済んでしまって再現しない）。
  await page.evaluate(() => {
    const body = document.querySelector('#preview-pane .markdown-body');
    const pad = document.createElement('div');
    pad.style.height = '500px';
    body.insertBefore(pad, body.firstChild);
  });
  await page.waitForTimeout(400);

  // 着地したところに留まっていること（引き戻されると 1 画面以上ずれる）。
  const after = await top(line);
  const paneH = await page.evaluate(() => document.getElementById('preview-pane').clientHeight);
  expect(Math.abs(after - landed),
    `着地 ${landed}px → ${after}px（ペイン高 ${paneH}px）`).toBeLessThan(paneH / 2);
});

test('iframe がリンク先へ移ったら、元のファイル名で付けてしまわない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  // ページ自身の JS が別の html へ移った状態。タブは page.html を指したままなので、
  // ここで錨ると sub/page.html の行番号を page.html の識別子で保存してしまう。
  await page.evaluate(() => {
    document.querySelector('iframe.html-frame').contentWindow.location.href = './sub/page.html';
  });
  await expect(frame.locator('body')).toContainText('サブフォルダ');

  await page.keyboard.press('c');
  await expect(page.locator('body')).toHaveClass(/md-cmt-mode/);
  await expect(page.locator('.md-cmt-hint')).toContainText('リンク先へ移っています');
  await expect(page.locator('.md-cmt-frame-mark[data-state="kbcursor"]')).toBeHidden();
});

test('html 表示でもモード中は掴む合図（十字カーソル）が出る', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'page.html' }).first().click();
  const frame = page.frameLocator('iframe.html-frame');
  await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
  await page.evaluate(() => document.activeElement && document.activeElement.blur());

  const cursorOf = () => page.evaluate(() => {
    const d = document.querySelector('iframe.html-frame').contentDocument;
    const cs = d.defaultView.getComputedStyle(d.querySelector('h1'));
    return { cursor: cs.cursor, select: cs.webkitUserSelect || cs.userSelect };
  });
  expect((await cursorOf()).cursor).not.toBe('crosshair');

  // 親の CSS は iframe の中まで届かない。届けないと html 表示だけ、掴む画面に入った
  // 合図が出ないまま普段どおりの I ビームで指すことになる。
  await page.keyboard.press('c');
  await expect.poll(async () => (await cursorOf()).cursor).toBe('crosshair');
  expect((await cursorOf()).select).toBe('none');

  await page.keyboard.press('Escape');
  await expect.poll(async () => (await cursorOf()).cursor).not.toBe('crosshair');
});
