---
title: 全体リファクタリング（2026-08-05 実施）
date: 2026-08-05
status: 完了
scope: src/*.rs, src/*.js（同梱ライブラリの min.js を除く）
---

# 全体リファクタリング

機能追加を優先して進めた結果溜まっていた重複を、10 項目にまとめて解消した記録。

問題は「壊れている」ことではなく、**同じ判断が複数箇所にコピーされていて、次に何かを
足すと全部を直して回る必要がある**ことだった。以下は何をどこへ寄せたかと、
今後どこを触ればよいかの入口。

## 変更後の構造 ── 何を足すときどこを触るか

| やりたいこと | 触る場所 |
|---|---|
| 新しい表示対象を足す（画像 / `.csv` / `.ipynb` …） | `request.rs` の `ViewKind` と `render_file`。拡張子表は `RENDERABLE_EXT` |
| エンドポイントを足す | `request.rs` の `Route` と `parse_route` |
| 本文差し替え後の後処理を足す | `common.js` の `hydrate()` |
| 表示モードを足す（blame / プレゼン …） | `viewmode.js` の `SPECS` ＋ `keymap.js` に `view-<id>` 行 |
| ショートカットを足す・変える | `keymap.js` の `BINDS`（表示と実処理の両方がここから出る） |
| オーバーレイを足す | `MdCommon.registerOverlay()` で登録するだけ |
| ページに JS モジュールを足す | `html.rs` の `PAGE_SCRIPTS` に 1 行 |
| 起動モードの設定を変える | `app_config.rs` |
| 端末サブコマンドを足す | `cli.rs` ＋ `main.rs` の `run_terminal_command` |

## やったこと

### 1. ファイル種別の判定を `ViewKind` に集約した

「`.md` はレンダリング / `.html` は iframe / それ以外はソースビュー / 非 UTF-8 は通知」
という 1 つの決定が **7 箇所**（main.rs 3・request.rs 3・folder.js 1）に独立して実装され、
コメントの「揃える」で人力保証していた。実際 `.markdown` がウォッチャから漏れる回帰が
起きていた。

```rust
pub enum ViewKind { Markdown, HtmlPage, Source, Binary }
pub const RENDERABLE_EXT: &[&str] = &["md", "markdown", "html", "htm"];
pub fn render_file(path: &Path, rel: &str, mode: ViewMode) -> Option<RenderedFile>
```

初期ページ・`?file=`・`?body=1`・`?raw=`・`--html` ダンプの**すべてが `render_file` を通る**。
JS 側の `isRenderablePath` も `window.MD_RENDERABLE_EXT`（Rust から注入）を読むので、
拡張子の定義元は 1 箇所になった。

これに伴って直った食い違い 2 つ:

- **フォルダモードの frontmatter が全幅で出ていた**。単一ファイル表示では元から
  `.markdown-body`（720px）の内側だったが、フォルダの `?file=` 経路だけ外側に置いていた。
- **通知の見た目が経路で違った**。`.binary-msg`（テーマが色だけ指定）と `.diff-msg`
  （base.css が中央寄せだけ指定）に割れていたのを `.md-notice` 1 つに統一。

### 2. `handle_request` を `parse_route` に置き換えた

生クエリを `strip_prefix` で順番に舐めており、`file=` が `files=1` を拾わないことや
`diff=1` を `diff=<rel>` より先に見ることが**判定の順序**で保たれていた（そのための
テストまであった）。キーで厳密に分けたので、エンドポイントを足しても前方一致の衝突は
起きえない。

```rust
enum Target { Single, Rel(String) }   // `?raw=1`（単一） と `?raw=<rel>`（フォルダ）を吸収
enum Route<'a> { BuiltinLib, Dir, HasMd, Files, Changed, View(Target), Raw(Target), … }
```

`Target` が「ラッパ（`.markdown-body`）を付けるか」まで持つので、
単一用とフォルダ用に 2 本ずつあった配信関数（diff / raw / diffstat / body）が各 1 本になった。

### 3. カスタムプロトコルをスレッド化した

macOS では WKWebView がハンドラをメインスレッドで呼ぶのに、別スレッドへ逃がしていたのは
`has_md=` だけだった。`files=1`（最大 5 万ディレクトリ走査）・`changed=1`（git 子プロセス）・
`diff`（最大 400 万セルの LCS DP）は同期実行でウィンドウを止めうる状態だった。

入口で一律 `thread::spawn` にしたので、以後どんな重いエンドポイントを足しても固まらない。
`has_md` の個別対応も消えて `handle_request` に載った。

### 4. 本文差し替え後の後処理を `MdCommon.hydrate()` に集約した

`ensureHeadingIds → highlight → copyButtons → lineNumbers → mermaid → drawio →
frames → search 再init → toc → comment.reanchor` の並びが 5 箇所（init.js 2・folder.js・
raw.js・diff.js）に、微妙に食い違った形でコピーされていた。

中身の種類で分岐していないのは、各処理が「対象が無ければ何もしない」ように書けているため
（差分に mermaid は無いし、ソースビューに見出しは無い）。分岐を増やすより無いものを黙って
飛ばす方が、経路ごとの差を生まない。

副産物として `hljs.highlightAll()`（毎回 document 全体を走査）を、差し替えた範囲だけの
`highlightElement` に統一した。

### 5. `diff.js` / `raw.js` を `viewmode.js` に統合した

337 行のうち 240 行ほどが変数名以外同一だった。排他も互いの `isActive()` /
`deactivate()` を名指しで呼び合う形で、3 つ目を足すと組み合わせが増える構造だった。

`SPECS` に 1 行足せばボタン・排他・再取得・バッジが揃う。同時に active になれるのは
1 つだけで、それは `viewmode.js` の `activeId` が保証する。**337 行 → 209 行。**

### 6. オーバーレイをレジストリ化した

`isOverlayOpen()` が id のハードコード列で、Esc は各モジュールが自前で拾って
「他が開いていたら譲る」を個別に書いていた。6 つ目を足すには 3 箇所を揃える必要があった。

```js
MdCommon.registerOverlay({ id, isOpen, close, priority, blocksKeys });
```

Esc は `common.js` が 1 箇所で受け、**priority 最大の 1 つだけ**を閉じる。
コメントモードは `blocksKeys: false` で登録してあるので、モード中も `d`/`u`/`Space` の
ページ送りは効いたまま、Esc の順番（入力を取消 → モードを抜ける）だけに参加する。

### 7. `keymap.js` を表示専用から実処理の駆動元にした

表は表示専用で、実処理は 8 ファイルに散った `keydown` リスナが持っていた。
つまり**キーを変えるには表とハンドラの両方を直す必要があった**。

いまは 1 つの表に「キー・説明・カテゴリ・表示範囲・効く文脈（`when`）・ハンドラ名（`run`）」
が載り、`keymap.js` の唯一の `keydown` リスナがディスパッチする。各モジュールは
`MdKeymap.on('<run>', fn)` で実処理を差し込むだけ。

- 同じキーを複数の行が持つ場合（`j`/`k` は 本文 / ツリー / コメント中 の 3 つ）は
  `when` が互いに排他になるよう書いてあるので、表の順序に依存しない。
- 内側のハンドラが処理済みのキー（パレット入力欄の `⌃p` など）は、ディスパッチャが
  `defaultPrevented` を見て譲る。
- 入力欄でも奪う必要がある開閉トグル（`⌘F` / `⌘T` / `⌘P`）は `cmdAnywhere`、
  `⌃W` を巻き込みたくない `⌘W` は `metaOnly` と、述語を分けてある。
- JS を通らないキー（`⌃⌘F` / `⌘Q` は macOS のメニュー項目が処理）は `run` 無しの
  表示専用行として同じ表に並ぶ。

### 8. `main.rs` を分割した

795 行に CLI 引数処理・テーマ一覧表示・自己デタッチ・ウィンドウ起動・イベントループが
同居していた。

- `cli.rs`（ライブラリ側）… `--help` / `--sample` / `md theme` / `--html` ダンプ。
  GUI 非依存なのでユニットテストが書ける
- `app_config.rs`（ライブラリ側）… 起動モードごとの設定組み立て、ページへ注入する
  グローバル（`page_globals`）
- `main.rs`（412 行）… ウィンドウとイベントループと監視だけ

### 9. 小さな重複を掃除した

| 内容 | 変更 |
|---|---|
| 入力欄判定のインライン展開 4 箇所 | `MdCommon.isFieldEl` / keymap の述語へ |
| `⌘W` ハンドラ 2 コピー | `common.js` の 1 箇所へ |
| アンカースクロール 2 コピー | `MdCommon.scrollToAnchor` へ |
| `AppConfig` 組み立ての重複・ウィンドウ幅のマジックナンバー | `AppConfig::folder` / `single_page` と定数へ |
| `head()` の script 手作業列挙 | `PAGE_SCRIPTS` 配列へ（追加は 1 行） |
| バイナリ通知の文言 4 箇所 | `request::notice_html` へ |
| `style.css` 読み込み 2 箇所 | `md_preview::user_style_css()` へ |
| `scrollingElement || documentElement` の書き写し | `MdCommon.getScroller()` へ |

### 10. テストを足した

**Rust（`tests/render.rs`）** — 描画結果のスナップショットテスト 19 本。
`tests/fixtures/` を描画した結果を `tests/snapshots/` と丸ごと突き合わせる。
inline される CSS / JS の中身は `…` に畳み、nonce は固定値に潰して比較する。

```sh
cargo test                                        # 全部
UPDATE_SNAPSHOTS=1 cargo test --test render       # 期待値を更新（差分は必ず目で見る）
```

このリファクタ中、意図しない構造変化はすべてここが先に捕まえた（frontmatter の位置と
通知クラスの 2 件）。

**UI（`tests/ui/smoke.spec.js`）** — Playwright スモークテスト 10 本。
Rust 側では一切触れない「フォーカス排他とオーバーレイの譲り合い」を押さえる。

```sh
npm install && npx playwright install webkit   # 初回だけ
npx playwright test
```

- ブラウザは **WebKit**。本番が WKWebView なので、キーイベントの扱いや
  CSS Custom Highlight API の有無を一番忠実に再現できる
- ページは `examples/serve.rs`（`handle_request` を HTTP で公開する開発用サーバ）越しに開く。
  製品バイナリには含まれない（`cargo install` は `[[bin]] md` だけを入れる）
- フォルダモードと単一ファイルモードでショートカットの有無が違うので、サーバは 2 つ立てる。
  単一ファイルモードは「cwd の外のファイル」を開いたときのモードなので、
  cwd を一時ディレクトリにして起動する

## 残っている割り切り

直さなかったもの。困ってから直せばよい判断。

- **`comment.js` が 982 行**で最大。コメントの CRUD・マーカー描画・ポップオーバー・
  パネル・キーボード操作・ドラッグ選択が 1 ファイルにある。分けるほどの実害はまだ無い
  （相互参照が多く、分けると往復が増える）
- **`search.js` が 640 行**。親の mark 方式と iframe の Highlight API 方式の 2 実装を
  持つのは意図的（`docs/html-iframe-search.md` の判断）
- **`base.css` が 1580 行の 1 ファイル**。構造レイヤーとして一貫しているので分割の
  必然性は薄い。新しい UI をシステムカラー（Canvas/CanvasText/color-mix）で組む規約は
  維持——これのおかげで UI を足しても 13 個のテーマ CSS に触らなくて済む
- **UI テストはフォルダモード中心**。stdin モードは対象外（サーバ経由で再現しにくい）

## 壊さない方がいいもの

- `lib.rs` によるコア / GUI 分離。fuzz・統合テスト・`examples/serve.rs` が同じコードを叩ける
- CSP + nonce 設計。untrusted Markdown 前提の防御が一貫している
- `safe_join` の二段構え（`..` 成分の拒否 ＋ canonicalize 後の prefix 検査）
- テーマの appearance 固定テスト（「本文はライトなのにコードだけダーク」を構造的に防ぐ）
- 上限とその報告（`Truncation` enum・`MAX_MATCHES`・`CHANGED_MAX`・`HIGHLIGHT_MAX_BYTES`）。
  黙って切らずに理由と実際の上限値を UI へ返す
- `docs/` に判断の経緯を残すこと
