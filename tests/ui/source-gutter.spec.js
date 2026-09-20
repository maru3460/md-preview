// ソースビューの行番号ガター（common.js の buildSrcGutter / syncSrcGutter）。
//
// 番号は横スクローラ（code）の外の 1 カラムに並ぶ。中に置くと、横スクロールで流れない
// ための sticky が行数ぶん要って縦スクロールが重くなるため。外に出したぶん縦位置は
// 自分で合わせるので、ここで見るのは「ずれていないこと」が中心になる。
//   ・3000 行級でも、先頭から末尾まで番号と行がそろう（丸め誤差が積もらない）
//   ・横に送っても番号は動かない（スクローラの外にいる）
//   ・行間にコメントカードが挟まると、その番号だけカードのぶん下がる（レンジなら末尾行）
//   ・カードが消えれば隙間も畳まれる
//   ・はみ出していないソースに、ガター幅ぶんの横スクロールを作らない
//   ・表示を往復してもガターは 1 本のままで、隙間が持ち越されない
const { test, expect } = require('@playwright/test');
const fs = require('fs');
const path = require('path');
const { openFolder } = require('./helpers');

const LINES = 600;
// 画面より十分に短い行だけのソース（横スクロールが出ないことを見るため）。
const NARROW = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join('\n') + '\n';
// 240 桁 ＝ どの窓幅でも必ず横にあふれる。空行も混ぜる（高さを持つか見るため）。
const WIDE = Array.from({ length: LINES }, (_, i) =>
  (i % 50 === 0 ? '' : `line ${i + 1}: ` + 'x'.repeat(240))).join('\n') + '\n';

function withFixture(name, body, fn) {
  const file = path.join(__dirname, '../ui-fixtures', name);
  fs.writeFileSync(file, body);
  return fn(name).finally(() => fs.rmSync(file, { force: true }));
}

/// rAF スロットルの測り直しが済むまで待つ。
const settle = (page) =>
  page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

/// 番号のセルと行の縦ずれ（px）。指定した行だけを見る。
const drift = (page, lines) => page.evaluate((lines) => {
  const cells = [...document.querySelectorAll('#preview-pane .source-gutter-rows .src-num')];
  const rows = [...document.querySelectorAll('#preview-pane .md-src-row')];
  return lines.map((n) => {
    const i = n === 'last' ? rows.length - 1 : n - 1;
    return Math.round(cells[i].getBoundingClientRect().top - rows[i].getBoundingClientRect().top);
  });
}, lines);

test('3000 行級でも番号と行が末尾までそろい、横に送っても番号は動かない', async ({ page }) => {
  await withFixture('zz-gutter.txt', WIDE, async (name) => {
    await openFolder(page);
    await page.locator('.tree-item', { hasText: name }).click();
    await expect(page.locator('.source-main')).toBeVisible();
    await expect(page.locator('#preview-pane .md-src-row')).toHaveCount(LINES);
    await expect(page.locator('#preview-pane .src-num')).toHaveCount(LINES);
    await settle(page);

    // 丸め誤差が積もると末尾ほどずれるので、先頭・中ほど・末尾を見る。
    expect(await drift(page, [1, 2, LINES / 2, 'last'])).toEqual([0, 0, 0, 0]);

    // 番号は横スクローラの外にいるので、横に送っても 1px も動かない。
    const cellLeft = () => page.locator('#preview-pane .src-num').first()
      .evaluate((el) => Math.round(el.getBoundingClientRect().left));
    const before = await cellLeft();
    await page.locator('#preview-pane .source-main pre code').evaluate((el) => { el.scrollLeft = 300; });
    await settle(page);
    expect(await cellLeft()).toBe(before);
    expect(await drift(page, [1, LINES / 2, 'last'])).toEqual([0, 0, 0]);
  });
});

test('コメントカードが挟まると、その行の番号だけカードのぶん下がる', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('.source-main')).toBeVisible();
  await settle(page);
  expect(await drift(page, [1, 3, 4, 'last'])).toEqual([0, 0, 0, 0]);

  // 3 行目にコメントを付ける（モード中はカードが行の直後に挟まる）。
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
  await page.keyboard.press('c');
  await page.locator('.md-src-row[data-src-line="3"]').click();
  await page.locator('.md-cmt-textarea').fill('ここに質問');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.source-main .md-cmt-embed')).toHaveCount(1);
  await settle(page);

  // カードの上下で番号がずれていない ＝ 3 行目の番号だけがカードのぶん押し下がった。
  expect(await drift(page, [1, 3, 4, 'last'])).toEqual([0, 0, 0, 0]);
  const gap = await page.locator('#preview-pane .src-num').nth(2)
    .evaluate((el) => parseFloat(el.style.marginBottom) || 0);
  const card = await page.locator('.source-main .md-cmt-embed')
    .evaluate((el) => el.getBoundingClientRect().height);
  expect(gap).toBeGreaterThan(card - 1);

  // 💬 バッジはモード外の目印で、行ではなく番号のセルに載る（コードに重ねない）。
  await page.keyboard.press('Escape');
  await expect(page.locator('.src-num > .md-cmt-badge')).toHaveCount(1);

  // カードが消えれば隙間も畳まれる。
  await expect(page.locator('.source-main .md-cmt-embed')).toHaveCount(0);
  await settle(page);
  expect(await page.locator('#preview-pane .src-num').nth(2)
    .evaluate((el) => el.style.marginBottom)).toBe('');
  expect(await drift(page, [1, 3, 4, 'last'])).toEqual([0, 0, 0, 0]);
});

test('はみ出していないソースには横スクロールが出ない', async ({ page }) => {
  await withFixture('zz-gutter.txt', NARROW, async (name) => {
    await openFolder(page);
    await page.locator('.tree-item', { hasText: name }).click();
    await expect(page.locator('.source-main')).toBeVisible();
    await settle(page);

    // ガターを入れる前の可視幅で行の min-width を焼くと、最長行が短くても
    // ガター幅ぶん横に伸びる（番号を行の外へ出したときに踏んだ穴）。
    expect(await page.locator('#preview-pane .source-main pre code').evaluate(
      (el) => el.scrollWidth - el.clientWidth)).toBe(0);
    expect(await drift(page, [1, 2, 'last'])).toEqual([0, 0, 0]);
  });
});

test('中身の無いファイルでも 1 行目が高さを持つ', async ({ page }) => {
  await withFixture('zz-gutter.txt', '', async (name) => {
    await openFolder(page);
    await page.locator('.tree-item', { hasText: name }).click();
    await expect(page.locator('.source-main')).toBeVisible();
    await settle(page);

    // 番号を行の外へ出す前は、行番号の ::before が空行の高さを保証していた。
    const h = await page.evaluate(() => {
      const row = document.querySelector('#preview-pane .md-src-row');
      const cell = document.querySelector('#preview-pane .src-num');
      return { row: row.getBoundingClientRect().height, cell: cell.getBoundingClientRect().height };
    });
    expect(h.row).toBeGreaterThan(0);
    expect(Math.round(h.row - h.cell)).toBe(0);
  });
});

test('レンジコメントの隙間は末尾行の番号に入る', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('.source-main')).toBeVisible();
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
  await page.keyboard.press('c');

  // 5-6 行目をドラッグして 1 件にする。カードは範囲の末尾（6 行目）の直後に出る。
  const row5 = await page.locator('.md-src-row[data-src-line="5"]').boundingBox();
  const row6 = await page.locator('.md-src-row[data-src-line="6"]').boundingBox();
  await page.mouse.move(row5.x + 40, row5.y + row5.height / 2);
  await page.mouse.down();
  await page.mouse.move(row6.x + 40, row6.y + row6.height / 2);
  await page.mouse.up();
  await page.locator('.md-cmt-textarea').fill('この 2 行');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.source-main .md-cmt-embed')).toHaveCount(1);
  await settle(page);

  const margins = await page.evaluate(() => [...document.querySelectorAll('#preview-pane .src-num')]
    .map((c, i) => [i + 1, parseFloat(c.style.marginBottom) || 0]).filter((m) => m[1] > 0));
  expect(margins.map((m) => m[0])).toEqual([6]);
  expect(await drift(page, [1, 5, 6, 7, 'last'])).toEqual([0, 0, 0, 0, 0]);
});

test('表示を往復してもガターは 1 本で、隙間が持ち越されない', async ({ page }) => {
  await openFolder(page);
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('.source-main')).toBeVisible();

  // コメントを 1 件付けて隙間を作る。
  await page.evaluate(() => document.activeElement && document.activeElement.blur());
  await page.keyboard.press('c');
  await page.locator('.md-src-row[data-src-line="3"]').click();
  await page.locator('.md-cmt-textarea').fill('往復のあいだ残らないこと');
  await page.locator('#md-cmt-popover .md-cmt-btn-primary').click();
  await expect(page.locator('.source-main .md-cmt-embed')).toHaveCount(1);
  await page.keyboard.press('Escape');

  // 別ファイルへ行って戻る（本文の差し替えでガターも作り直される）。
  await page.locator('.tree-item', { hasText: 'a.md' }).click();
  await expect(page.locator('#preview-pane .markdown-body')).toContainText('見出し A');
  await page.locator('.tree-item', { hasText: 'notes.txt' }).click();
  await expect(page.locator('.source-main')).toBeVisible();
  await settle(page);

  await expect(page.locator('#preview-pane .source-gutter')).toHaveCount(1);
  // モード外＝カードは出ていないので、隙間は 1 つも残っていない。
  expect(await page.evaluate(() => [...document.querySelectorAll('#preview-pane .src-num')]
    .filter((c) => c.style.marginBottom).length)).toBe(0);
  expect(await drift(page, [1, 3, 4, 'last'])).toEqual([0, 0, 0, 0]);
});
