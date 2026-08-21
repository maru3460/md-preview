# raw / ソースビューのインライン埋め込み（handoff）

コメントのインライン埋め込み（`docs/comment-sidebar.md` 参照）を raw・ソースビューでも
出せるようにするための調査メモと設計。**実装済み**（案 A・常時 1 行 1 要素。
`common.js` の `wrapSourceLines` が本文差し替え時に包み直し、行レイヤ
（`ensureSourceRows` 一族）は削除した。以下は当時の調査記録）。

## 問題

raw（⌘R）と非 md ファイルのソースビューは、本文全体が **1 個の `<pre><code>`** で、
行の単位になる要素が DOM に無い。コメント用の行ユニットは、透明な行レイヤ
（`.md-src-rows`、`comment.js` の `ensureSourceRows`）を上に重ね、**空の行ボックスを
積み上げるだけ**で下のコードと行を揃えている（座標計算なし）。

この構造では行の間に埋め込みブロックを挟めない——レイヤ側の行だけ下へズレて、
動かないコード本体と揃わなくなる。だから現状は錨が `.md-src-row` のとき埋め込みを
スキップし、raw のコメントはサイドバー一覧で読む割り切りにしている。

対応するなら**ソースビューのレンダリング構造ごと変える**必要がある。

## 参考: 他プロダクトの実装

### difit（yoshiko-pg/difit）— このコメント機能の元イメージ

ローカル git diff のビューア。「差分にコメント → AI に渡す」の本家。

- React 18 + TypeScript + Prism.js（言語は動的ロード）
- **行単位のコンポーネント**: `DiffLineRow` / `DiffCodeLine`（`src/client/components/`）
- コメントは `CommentThreadCard` を**行コンポーネントの下に挿入**する。行が最初から
  1 行 1 要素なので、挿入で位置合わせが壊れる問題自体が存在しない
- ハイライトも行単位（`PrismSyntaxHighlighter`）。diff は行が主役なので、
  複数行にまたがる構文の扱いは diff 文脈では割り切っている模様

### GitHub

- blob / diff とも `<table>` で **1 行 = 1 `<tr>`**。インラインコメントは
  colspan した `<tr>` をその行の直後に挿入する
- ハイライトはサーバ側で行ごとに済ませた HTML を返す（複数行構文は
  ステートマシンを行またぎで持ち越す方式）

### エディタ系（追加調査候補）

- **Monaco (VSCode)**: view zones——行間に任意高さのブロックを挟む公式機構。
  VSCode のレビューコメント UI はこれ
- **CodeMirror 6**: block widget / decoration で同等のことができる

## 実装するなら: 選択肢

### 案 A: ソースビューを 1 行 1 要素にする（difit / GitHub 方式）

hljs 適用**後**の HTML を行境界で分割して、1 行ずつ要素に包み直す。

- hljs は複数行にまたがる `<span>`（文字列・コメント）を出すので、単純に `\n` で
  split すると壊れる。**行末で開いているタグを閉じ、次の行頭で開き直す**変換が要る
  （highlightjs-line-numbers 等の先行例がある既知テク）
- これができると行レイヤ（`.md-src-rows`）自体が不要になり、既存のユニット処理が
  そのまま効く。埋め込みも兄弟挿入でそのまま動く
- 影響範囲: `ensureSourceRows` / `unitQuote`（`layer.mdLines` から引用を取る仕組み）/
  `base.css` の行レイヤ契約 / 行番号ガター（`addLineNumbers`）との整合
- **パフォーマンス注意**: 1 行 1 要素はコメントモード中の行レイヤで既にやっている
  規模（`SRC_ROWS_MAX` = 10,000 行で打ち切り）なので DOM 量は既知の範囲。ただし
  常時この構造になるため「素の閲覧を軽いままにする」という現在の方針
  （必要時のみ行レイヤ生成）とトレードオフになる。巨大ファイルの hljs は
  `request.rs` の閾値（1MB / 10,000 行で `nohighlight`）で既にスキップ済みなので、
  閾値設計はそこへの統合として考える

### 案 B: 固定フロート（中間案）

DOM に挟まず、n/p の巡回対象やカーソル行のコメントを行の近く（右余白など）に
浮かせる。位置合わせは壊れないが、スクロール追従の再配置が必要で、
「+」ハンドル（`moveHandle`）と同じ fixed 追従系の実装になる。

### 案 C: 現状維持

raw は色帯（行レイヤの `.md-cmt-marked`）＋サイドバー一覧で読む。
raw / プレビューの表示分離後は役割分担として一応成立している。

## 調査結果（2026-08-20）

上の「次にやること」1〜3 を実施した。結論: **案 A は技術的に成立する**。
ハイライトは「全文を hljs にかけてから HTML を行境界で分割する」方式でよい
（プロトタイプ検証済み、下記）。

### GitHub の実 DOM（実ページを取得して確認）

- **blob ビュー**は React 化されていて全部 div。ただし行番号と本文が**別カラム**
  （`.react-line-numbers` と `.react-code-lines`）で、`data-line-number` と
  line-height の一致だけで揃えている。可変高のブロックを行間に挟むと番号カラムが
  ズレる構造なので、インライン埋め込みの参考にはならない。
  ハイライト層は `aria-hidden` で、選択・検索は透明 `<textarea>` が担う二重化。
  大ファイルはビューポート近傍 ~75 行だけ DOM 化する仮想スクロール。
- **PR diff ビュー**は今も `<table>` で 1 行 = 1 `<tr>`。インラインコメントは
  対象行の直後に `colspan="3"` の `<tr class="inline-comments">` を挿入する。
  行番号セルはテキストを持たず `data-line-number` + CSS `::before` で表示
  （コピーに行番号が混入しない定番テク）。
- **複数行構文の持ち越し**: GitHub はハイライト結果を HTML ではなく
  `stylingDirectives`（行ごとの `[開始桁, 終了桁, クラス]` 配列）で受け取る。
  **スパンは絶対に行境界をまたがない**——7 行のブロックコメントは「各行が独立に
  `pl-c`」として 7 回出る。行またぎ問題はハイライタ側が行単位にクラスを
  再発行することで消している。

### difit の実装（ソースを読んで確認）

- 1 行 = 1 `<tr>`（`DiffLineRow`、React.memo）。コメントスレッドは行の子ではなく、
  `chunk.lines.map()` 内で Fragment を使い**行 `<tr>` の直後に `colSpan={3}` の
  別 `<tr>` を兄弟挿入**。範囲コメントは**末尾行の直下**に出す
  （この repo の埋め込みと同じ判断）。
- ハイライトはハイブリッド: デフォルトは**行ごと完全独立**に Prism へかけるだけで、
  複数行文字列の途中行が壊れるのは割り切り。Vue/Svelte/Astro だけ
  **全文を 1 回 tokenize → 行別トークン配列に割って各行に配る**
  （`precomputedTokens`）。**2000 行超は全文 tokenize をスキップ**して行単位に
  フォールバック。トークナイザ状態のストリーム持ち越しはしていない。
- 行単位の仮想スクロールは不採用。React.memo + ファイル単位の遅延レンダリング
  （IntersectionObserver, 初期 8 ファイル）+ 2000 行上限だけ。

### hljs 行分割プロトタイプ（検証済み）

同梱の `src/highlight.min.js` で「全文ハイライト → 行境界で分割、行またぎの
`<span>` は行末で閉じて次の行頭で開き直す」変換を検証した。hljs の出力は
`<span class="...">` / `</span>` / エスケープ済みテキストだけの安全なサブセット
なので、正規表現トークナイザ + スタックで足りる。

```js
function splitHighlightedLines(html) {
  var lines = [], stack = [], buf = '';
  var re = /(<span[^>]*>)|(<\/span>)|(\n)|([^<\n]+)|(<)/g, m;
  while ((m = re.exec(html)) !== null) {
    if (m[1]) { stack.push(m[1]); buf += m[1]; }
    else if (m[2]) { stack.pop(); buf += m[2]; }
    else if (m[3]) {
      buf += '</span>'.repeat(stack.length);  // 行末で全部閉じる
      lines.push(buf);
      buf = stack.join('');                    // 次の行頭で開き直す
    }
    else buf += m[0];
  }
  buf += '</span>'.repeat(stack.length);
  lines.push(buf);
  return lines;
}
```

検証内容: `main.rs` / `comment.js` / `base.css` / `sample.md` / `request.rs` +
複数行テンプレートリテラル・ブロックコメントの合成ケースで、
「各行のタグが行内で閉じている」「各行のテキストが元ソースの行と完全一致」を
全ケース確認。行またぎ（持ち越し）は CSS コメント・markdown で実際に発生し、
正しく処理された。**コストは 11,176 行の JS で hljs 本体 77ms に対し分割 7ms**——
分割がボトルネックになることはない。

### 案 A の設計メモ（実装時の入口）

- ソースビューの生成は `request.rs` の `source-view`（全文 1 個の `<pre><code>`）。
  1 行 1 要素化はクライアント側（hljs 適用後に分割して包み直す）でも、
  サーバ側でも選べる。hljs はクライアントで動いているので、
  `highlightIn`（`common.js`）の直後に分割を挟むのが素直。
- 行番号は GitHub diff の `data-line-number` + `::before` 方式にすると、
  ガター別カラム（`addLineNumbers`）を置き換えつつコピー安全にできる。
- 埋め込みスキップは `comment.js` の `md-src-row` 判定 1 箇所。行レイヤが
  消えればこの分岐ごと消せる。`unitQuote` の `layer.mdLines` 特例も、
  各行要素の `textContent` から取れるようになり不要になる。
- 閾値設計: difit の「2000 行超は落とす」と同型。閾値の本家は既に
  `request.rs` にある（`HIGHLIGHT_MAX_BYTES` = 1MB / `HIGHLIGHT_MAX_LINES` =
  10,000 で `nohighlight`）ので、`SRC_ROWS_MAX` を新設閾値と揃えるのではなく
  サーバの 1 判定に統合する。巨大ファイルは「ハイライトなし + 1 行 1 要素」
  または「案 C 相当」に落とす。

## セカンドオピニオン（2026-08-20、別セッションの Fable に相談）

推奨は「**案 A・常時 1 行 1 要素・閾値はサーバ 1 本**」。要点:

- **案 A を推す**。行レイヤは wheel 手動転送 / `layer.mdLines` / `unitQuote` 特例 /
  埋め込みスキップと補修が既に 4 箇所あり、これ以上足すより構造を正す方が総量が減る。
  案 B は fixed 追従系をもう 1 系統増やすだけ。
- **常時包む**（hydrate の `highlightIn` 直後、閾値超過ファイルは包まず案 C 挙動へ
  フォールバック）。遅延包み直しは、モード突入時の `<code>` 再構築で検索の
  `<mark>` や選択が孤児になる副作用がある。1 万 div は WKWebView で問題にならない
  規模（ハイライト済みファイルは既に数万 span を持っている）。
- **閾値は `request.rs` の `HIGHLIGHT_MAX_LINES`（10,000 行）1 本**に統合し、
  `SRC_ROWS_MAX` は行レイヤごと削除。バイト閾値（1MB）はハイライト専用に残す——
  1MB 超・少行数のファイル（ログ等）は色なしで包めば行コメントできるので、
  包み判定にバイトを混ぜると退行する。

実装時の具体的な壊れ所（コード読みで発見）:

1. `unitQuote` の isCode 判定（comment.js:181）——新しい行要素は `code-wrapper` でも
   `PRE` でもないので散文ブランチに落ち、**引用のインデントが潰れる**。行クラスを
   isCode に足すのが必須
2. Copy ボタン（`addCopyButtons`）が `code.innerText` を送るので、埋め込みが
   `<code>` 内に入る新構造では**コメント本文が混入**する。clone → `.md-cmt-*`
   除去 → textContent 方式に変える
3. 選択コピーの改行・空行保持のため、**各行末に実際の `\n` テキストノードを残す**
   （shiki 方式）。`code.textContent === 元ソース` の不変条件も維持される
4. 横スクロール: 行 div は `width: max-content; min-width: 100%`（diff の内側
   トラックと同じ手）、埋め込みは `position: sticky; left: 0`。これで wheel
   手動転送ハックも消える
5. 行番号はガター別カラムのままだと埋め込みで**番号がズレる**（GitHub blob を
   「参考にならない」と切ったのと同じ問題）。行要素の `::before`
   （`content: attr(data-src-line)`）+ sticky left に移す。**pseudo 要素の
   sticky が WKWebView で効くかだけ未検証**——最初に実機で潰す。ダメなら
   実 span + `user-select: none` に倒す
6. redraw の `allUnits` が常時 1 万要素走査になるので、コメント 0 件なら
   early return を足す
7. クラス名 `md-src-row` は意図的に継続する（viewSettled / computeTarget /
   redraw がこの名前で書かれていて、保てば無改修で動く）

推奨の実装順: (a) 実機スパイクで「::before + sticky 行番号」「空行 div の高さ」
「複数行選択コピー」の 3 点を先に検証 → (b) `common.js` に行包み実装
（`highlightIn` 直後・冪等・行末 `\n` 保持）→ (c) `base.css` の行レイヤ契約を
削除して新スタイル → (d) `comment.js` の行レイヤ一族を削除 + `unitQuote` 修正 →
(e) `addCopyButtons` の clone 化 → (f) 回帰確認（検索・ドラッグ選択・n/p・
ホットリロード・1 万行・1MB 少行数）。

## 次にやること

1. 案 A（常時 1 行 1 要素・shiki 型）で行くか、ユーザーが決める
2. 行くなら実機スパイク（上記 (a) の 3 点）から着手し、`ensureSourceRows` 一族の
   置き換え設計を `docs/comment-sidebar.md` と同じ形式で残す
