// keymap.js の表と実処理の対応。
//
// ディスパッチは `handlers[b.run]` が無ければ黙って読み飛ばす。つまり run 名を
// 打ち間違えても例外にはならず、そのキーが無反応になるだけで気付けない。
// window.MdX 越しのモジュール間契約は型が無いので、ここだけは実際に起動した
// ページへ訊きに行って照合する。
const { test, expect } = require('@playwright/test');
const { SINGLE_URL, open, openFolder } = require('./helpers');

/// そのモードで出るはずの run のうち、担当モジュールが居ないもの。
function orphanRuns(page) {
  return page.evaluate(() =>
    window.MdKeymap.binds
      .filter((b) => b.run && window.MdKeymap.inScope(b.scope))
      .map((b) => b.run)
      .filter((run) => !window.MdKeymap.has(run)));
}

test('フォルダモードのキーが全部ハンドラを持っている', async ({ page }) => {
  await openFolder(page);
  // タブ・ツリー・パレットまで初期化された状態で照合する。
  expect(await orphanRuns(page)).toEqual([]);
});

test('単一ファイルモードのキーが全部ハンドラを持っている', async ({ page }) => {
  await open(page, SINGLE_URL);
  // folder スコープのキー（⌘P / ⌘B / タブ）はそもそも出ないので対象外になる。
  expect(await orphanRuns(page)).toEqual([]);
});

test('表示専用の行（run 無し）は JS を通らないキーだけ', async ({ page }) => {
  await openFolder(page);
  // run が無い行はヘルプに出るだけで keydown では拾わない。macOS のメニューが
  // 処理するキーと、common.js のレジストリが一括で受ける Esc がこれに当たる。
  // ここが増えていたら「表に足したのに動かないキー」を作った可能性がある。
  const displayOnly = await page.evaluate(() =>
    window.MdKeymap.binds.filter((b) => !b.run).map((b) => b.keys));
  expect(displayOnly).toEqual(['⌃⌘F', '⌘Q', '右クリック', 'Esc']);
});
