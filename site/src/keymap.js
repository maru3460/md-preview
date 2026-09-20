import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { runInNewContext } from 'node:vm';

// キーバインドの本家は src/assets/js/keymap.js の BINDS。サイトはそこから「表を
// 生成」はしない（`?` 一覧は列に流すので文言が短く、サイトは初見向けに開きたい。
// 刻みも語彙も別物になる）。代わりに、ここで本家を読んで、サイトの表が全行を
// 拾えているかだけを突き合わせる。文言はサイト側の自由。
//
// keymap.js は IIFE で window.MdKeymap に生やすだけなので import はできないが、
// トップレベルで触るのは window と document.addEventListener の 2 つだけ。
// スタブを差して評価すれば binds がそのまま取れる（正規表現で抜くより、desc に
// 引用符が混ざっても壊れない）。
// keymap.js のトップレベルが他の DOM API を触り始めたらここが落ちる。そのときは
// スタブを足すこと。
const SRC = resolve(process.cwd(), '../src/assets/js/keymap.js');

function loadKeymap() {
  const sandbox = { window: {}, document: { addEventListener() {} } };
  runInNewContext(readFileSync(SRC, 'utf8'), sandbox);
  const km = sandbox.window.MdKeymap;
  if (!km || !Array.isArray(km.binds) || km.binds.length === 0) {
    throw new Error('keymap.js: MdKeymap.binds を取り出せませんでした（keymap.js の構造が変わっていませんか）');
  }
  return km;
}

const km = loadKeymap();

// match / when は関数なので落とし、表示に要る 4 つだけを持つ。
export const KEY_BINDS = km.binds.map((b) => ({ cat: b.cat, keys: b.keys, desc: b.desc, run: b.run }));
export const KEY_CATEGORIES = km.categories;

// サイトの表が本家の全行を拾えているかを確かめる。refs は各行が指す本家の keys 文字列。
// 1 行を複数行に開いてよい（ツリー内 → j/k・g/G・Enter/l・h）し、畳んでもよい。
// 見るのは「どの行が触れられていないか」「存在しない行を指していないか」だけ。
export function assertCovers(refs, where) {
  const known = new Set(KEY_BINDS.map((b) => b.keys));
  const seen = new Set(refs);

  const ghosts = refs.filter((r) => !known.has(r));
  const missing = KEY_BINDS.filter((b) => !seen.has(b.keys)).map((b) => `${b.keys}（${b.desc}）`);

  // サイトだけ古いまま公開されるより、ビルドが赤くなる方がよい。
  if (ghosts.length) {
    throw new Error(`${where}: keymap.js に無いキーを指しています: ${ghosts.join(', ')}`);
  }
  if (missing.length) {
    throw new Error(
      `${where}: keymap.js にあるのにサイトに出ていないキーがあります:\n  ${missing.join('\n  ')}\n` +
        '行を足すか、既にどこかで説明しているなら、その行の参照をそのキーにしてください。'
    );
  }
}
