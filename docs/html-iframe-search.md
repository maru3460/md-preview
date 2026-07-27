# html（iframe）の中身を検索する（設計検討）

## 目的

`.html` は iframe（ミニブラウザ方式）で描画しているため、⌘F / `/` の検索が html の
**中身**に効かない。親 DOM しか走査しないので、転送しても 0/0 の空振りバーが出るだけである。
そのため `bindFrame`（`src/common.js`）は検索キーを意図的に転送していない。
これを「iframe の中身も検索できる」状態にするための検討。

現状のワークアラウンドは ⌘R で生ソース表示に切り替えてから検索すること。

前提として、スクロール素キー・`?`・`]` / `[` / `Tab`・⌘R / ⌘D / ⌘T / ⌘W は
iframe にフォーカスがあっても効くようになっている（検索だけが取り残されている）。

## 現状の `search.js` の構造

1. `collect()` — `TreeWalker` で本文の Text ノードを集める（SCRIPT / STYLE / SVG /
   BUTTON / `aria-hidden` を除外）
2. マッチ箇所を `<mark class="md-search-hit">` として **DOM に差し込む**
3. `jumpTo(i)` で `scrollIntoView` し、カレントヒットに `.current` を付ける
4. 閉じるときにマークを剥がして `normalize()` で Text ノードを戻す

つまり**検索対象の DOM を書き換える**方式。これが iframe 相手だと問題になる。

## 案A: iframe の DOM に直接マークを差す

`search.js` の `document` 依存を「検索対象ドキュメント」引数に一般化し、対象が
`.html-frame` のときは `frame.contentDocument` を渡す。同一オリジン（sandbox 無し）で
配信しているので、技術的にはすべて可能。

必要な作業:

- スコープの一般化（`collect` / TreeWalker / `getSelection` を対象 doc 側から取る）
- **ハイライト用 CSS の注入** — 親の `base.css` は iframe に効かないので `<style>` を 1 枚差す
- `scrollIntoView` は iframe 内で完結するのでそのまま効く
- 閉じるとき・ホットリロード時のマーク除去

問題:

- `syncFrameBackground` のコメントにある「**中身は読むだけで書き換えない（忠実性を保つ）**」
  方針に反する。`<mark>` を挟むとインライン要素が増えるので、`p > :first-child` 系の CSS や
  `childNodes` を触るページ自身の JS が壊れうる
- ページ自身の MutationObserver / フレームワークの再描画があると、マークが消えたり暴れたりする
- 外部サイトへ遷移して cross-origin になると読めない（静かに無効化するしかない）

## 案B: CSS Custom Highlight API（`::highlight()`）— 本命

`Highlight` + `CSS.highlightRegistry` を使うと、**DOM を一切書き換えずに** Range だけで
ハイライトできる。

```js
const h = new Highlight(...ranges);
frame.contentWindow.CSS.highlightRegistry.set('md-search', h);
// 対象 doc 側の <style> に ::highlight(md-search) { background: ... }
```

利点:

- 忠実性の問題が消える。Text ノードを分割しないのでページの DOM も JS も無傷
- 後始末は registry から消すだけ。`normalize()` が要らない

注意点:

- ジャンプは `range.getBoundingClientRect()` から自前でスクロールする
  （`scrollIntoView` が使えないので、そこだけ手作業）
- `CSS.highlightRegistry` は**ドキュメントごと**なので、iframe 側の
  `frame.contentWindow.CSS` を使う必要がある
- WebKit は Safari 17.2+ で対応。このアプリは wry / WKWebView なので **OS の Safari
  バージョン依存**になる。`if (!window.Highlight)` のフォールバックは必須

## 着地案

**iframe だけ案B、親の md 本文は既存の `<mark>` 方式のまま**が最も安全。

- 親は既に動いていて壊す理由がない
- iframe は忠実性が最優先
- 共通化するのは「Range の集め方（TreeWalker）」と「検索バーの UI / ヒットカウント」だけに
  留め、**描画方式だけ差し替える**

### 作業量の見積り

| 作業 | 見積り |
| --- | --- |
| `search.js` を「対象 doc を受け取る」形にリファクタ（既存の親モードを壊さないのが肝） | 1〜2h |
| iframe 用の Highlight レンダラ＋自前スクロール | 1h |
| `/` と ⌘F の転送を `bindFrame` で解禁、ヒット 0 件時の見せ方 | 30m |
| ホットリロード・iframe 内遷移でのハイライト破棄、cross-origin フォールバック | 30m |

計 **3〜4 時間**程度。案B ならページを壊すリスクは無いので、手間の大半は既存
`search.js` のリファクタになる。

### 着手前に確かめること

本実装の前に、**`Highlight` が実機の WKWebView で本当に効くか**を捨てコードで確認する
（15 分程度）。ダメなら案A に落とすか、「html は ⌘R でソース表示してから検索」で据え置く。

## やらないこと

コメント機能（`c`）の iframe 対応。`comment.js` は Rust 側レンダラが吐く
`[data-src-line]` 属性にぶら下がっているが、任意の html にはソース行との対応が無い。
DOM ノードと html ソース行を後付けで対応付けるのは別プロジェクト級のため、html 表示中は
非対応と割り切る。
