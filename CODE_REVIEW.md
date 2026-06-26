# md-preview コードレビュー & リファクタ TODO

> 調査日: 2026-06-26 / 対象: `src/` 全体（Rust + ES5 JS）
> 本ドキュメントは **リファクタリングを本筋**とした調査結果。パフォーマンスは実測で結論を出し、
> バグ観点は「リファクタ中に当たったら先に潰す候補」として付録に記録する。

---

## 0. TL;DR

- **パフォーマンス**: 急いで直すべき問題は無し（実測で確認済み）。
- **リファクタ**: ✅ **完了**。R1〜R8 をすべて実施。`cargo build` / `cargo test`（9 passed）/
  `cargo clippy` green、headless スクショ（dark/light）でリファクタ前と一致、実アプリで
  mermaid lazy-load 描画も確認済み。新規 `src/common.js` に JS 共通ヘルパを集約。
- **バグ修正（先行実施・済）**: 付録の実在バグ（C1 / High-XSS / L1 / M3 / M1）はリファクタ前に
  **修正済み**（commit `3c72ffd`）。詳細と検証結果は付録 §3 を参照。M2・L2 は誤検知/過大評価で未対応。

---

## 1. パフォーマンス調査（結論: 対応不要）

### 1.1 フォルダの `has_md` 走査は問題なし（実測済み）
`has_md_descendant`（`request.rs:93`）は最初の `.md` で early-return する。`find ... -print -quit` を代理に実測:

| シナリオ | 結果 |
|---|---|
| `md /` の spawn スレッド数（`/`直下のdir数）| 16 |
| 最悪の単一dir（`/private`, `/System`）| 約 2.0s |
| 実プロジェクトの `node_modules`（README多数）| **0.000s（即時）** |

- async（`main.rs:178` のスレッド spawn）なので **UI をブロックしない**。サイドバーのドットが遅れて点くだけ。
- `mdDotCache`（`folder.js:22`）でパスごと1回限り。スレッドは人間ペースの展開でしか増えない。
- 唯一遅いのは「巨大かつ .md 皆無」のシステムツリー（`/System` 等）だが実運用では稀。
- → **設計（async化）は妥当。対応不要。**

### 1.2 静的アセットの `to_vec()` コピー（効果限定的）
- `serve_builtin_lib`（`request.rs:206`）が mermaid(3MB)/drawio(3.7MB) を毎回 `to_vec()`。
- ただし `MdLibs.load()`（`init.js:54` / `folder.js:5`）が URL 単位でメモ化 → **コピーはプロセス当たり最大1回**＝起動時の一過性コスト。
- `Cow::Borrowed(MERMAID_JS.as_bytes())` 化は安全・正しいが**性能効果はほぼ無い**。やるなら「クリーンさ」目的で `serve_builtin_lib` だけ。
- `/` 応答の `html_bytes.to_vec()`（`request.rs:282`）は `Vec<u8>` をクロージャが所有しており `&'static` ではないため **Cow::Borrowed 化は不可（据え置きが正解）**。

### 1.3 リロード時の `hljs.highlightAll()`（実害なし）
- バンドルは highlight.js **v11.11.1**。`highlightElement` は `if (e.dataset.highlighted) return` で **既ハイライト要素を再パースせずスキップ**（バンドル内で確認）。
- 「全体を再ハイライト」という前提は誤り。実コストは querySelectorAll 走査 + スキップ要素の `console.log` のみ。
- → scope 限定化のメリットはコンソールログ抑制程度。**性能目的では不要。**

---

## 2. リファクタリング TODO（本筋）

### 2.1 優先度一覧（すべて ✅ 実施済み）

| # | 箇所 | 内容 | 優先 | 状態 |
|---|---|---|---|---|
| R1 | `init.js` / `folder.js` | `MdLibs`（遅延ロード IIFE）がほぼ完全コピー | 高 | ✅ `common.js` の `MdCommon.loadLib` に集約 |
| R2 | `init.js` / `folder.js` | `addCopyButtons`・`runMermaid(In)`・`runDrawio(In)` が重複。命名揺れ自体がコピーの証拠 | 高 | ✅ `MdCommon.addCopyButtons`/`runMermaid`/`runDrawio`（scope引数で統一） |
| R3 | `init.js` / `folder.js` / `toc.js` | 見出しスラグ化が3箇所重複、かつ実装が非一貫 | 高 | ✅ `MdCommon.slugify`/`ensureHeadingId(s)` に統一 |
| R4 | `html.rs` `build_html`/`build_folder_html` | `<head>` テンプレートが同一 | 中 | ✅ `head(title, theme_css, custom_css, extra_head)` を抽出 |
| R5 | `html.rs` mermaid/drawio/filename | fence本文を集める同型ループが3回コピペ | 中 | ✅ `collect_code_text()` に抽出 |
| R6 | `request.rs` | `serve_md_fragment` と `serve_single_file_body` がほぼ同一 | 中 | ✅ `render_md_file()` + 薄いラッパに分離 |
| R7 | `main.rs` | 7要素タプルを返す巨大 if/else | 中 | ✅ `AppConfig` struct + `build_stdin_config`/`build_path_config` |
| R8 | `request.rs` | `decode` が `percent_decode` の別名（デッドコード）| 低 | ✅ 削除し `percent_decode` 直呼び |

おまけ: `render_full_document()` を `html.rs` に新設し、stdin / 単一ファイル / `handle_asset` /
`run_html_dump` の本文生成重複を一掃。`handle_asset` の md 直開きでも frontmatter が描画される
ようになった（本来のページ表示との整合改善）。

### 2.2 着手順（推奨3ステップ + 仕上げ）

**ステップ1 — JS 共通化（R1・R2・R3・R4 をまとめて）** ← インパクト最大
- 新規 `src/common.js` に集約: `MdLibs` / `copyButtons(scope)` / `mermaid(scope)` / `drawio(scope)` / `slugify(text)` / `ensureHeadingIds(scope)`。
- `html.rs` に `head(title, theme_css, custom_css, extra_head)` ヘルパを抽出し、`build_html`/`build_folder_html` 双方の `<head>` を共通化。common.js は `search.js`/`toc.js` と同様 `<head>` に inline（init/folder より**前**に置く）。
- グローバル公開（`window.MdLibs` / `window.MdRender` 等）で ES5 方針（var/function/IIFE）のまま参照可能。
- 引数を `scope = scope || document` で吸収すれば、init（引数なし）/ folder（scope渡し）両方を1関数で賄える。
- **注意（挙動変化）**: R3 のスラグ一意化を統一すると、init/folder 側でも同名見出しに `-2` が付くようになる。これは実質バグ修正寄り（→ 付録 M1）。アンカー整合は toc 方式（一意化）に揃えるのが正しい。「リファクタで挙動を変えない」原則からは外れる点だけ意識する。

**ステップ2 — `html.rs` `collect_code_text` 抽出（R5）** ← 最小リスク
- `fn collect_code_text<'a, I>(iter: &mut Peekable<I>) -> String` を切り出し、mermaid/drawio/filename の3分岐で呼ぶ。
- 各分岐は「content をどう HTML 化するか」だけ残る。既存テスト（`mermaid_block_still_works` 等）が回帰を守る。完全に安全。

**ステップ3 — `request.rs` の掃除（R6・R8）** ← C1 を塞ぐ回
- `decode` を削除して `percent_decode` 直呼びに（R8）。
- `serve_md_fragment`/`serve_single_file_body` を `render_md_file(path) -> Option<String>`（fm+body、ラップなし）+ 薄いラッパに分離（R6）。
- **この回で付録 C1（パストラバーサル）を必ず修正**: `handle_asset` を `safe_join` 経由に統一する。

**仕上げ — `main.rs` タプル→struct 化（R7）** ← 影響範囲広め、最後に
- `struct AppConfig { title, init_script, html_bytes, window_width, root_dir, single_file_path, watch_enabled }` を導入。
- 各分岐を `build_*_config(...)` に切り出す。`render_full_document(markdown, title, theme_css, custom_css)` を `html.rs` に作れば stdin/single-file/`handle_asset` の本文生成重複も一掃できる。
- struct 化 → 分岐抽出の2段に分けて進めると安全。

---

## 3. 付録: レビュー中に見つかったバグ（リファクタの本筋外・記録用）

> 「リファクタを進める上でバグを踏んだら先に直す」方針の参照リスト。実在確認済みのみ掲載。
>
> **対応状況（2026-06-26, commit `3c72ffd`）**: サブエージェントで多角的に再検証のうえ、実在バグ
> （C1 / High-XSS / L1 / M3 / M1）を**修正済み**。M2 は過大評価、L2 は対策済み（誤検知）と判明し未対応。
> `cargo build` / `cargo test`（9 passed）green。

| # | 深刻度 | 状態 | 場所 | 概要 |
|---|--------|------|------|------|
| C1 | **Critical** | ✅ **修正済み** | `request.rs:235-247` | `handle_asset` が `safe_join` を通さず `..`/`%2e%2e` で **root 外の任意ファイルを読める**。`url_path` は `main.rs:172` で percent_decode 済みのため `![](%2e%2e/%2e%2e/etc/passwd)` 等が通る。→ `handle_asset` を `safe_join` 経由に統一して修正。**訂正**: 当初記載の「絶対パスで root 外」は不正確。`strip_prefix('/')` で相対化されるため**絶対パス単独では成立せず、`..`/`%2e%2e` 経由のみ**。 |
| **XSS** | **High** | ✅ **修正済み** | `html.rs:149`(`build_html`) / `:246`(`build_folder_html`) | **再検証で新規発見**。`<title>{title}</title>` にファイル名/ディレクトリ名を**無エスケープ**で埋め込み。macOS のファイル名は `< > " &` 可のため、`</title><script>...</script>.md` という名前を開くだけで任意 JS 実行（C1 と同じ「信頼できない md を開く」脅威モデル）。→ `<title>` 出力前に `html_escape(title)` で修正。 |
| M1 | Medium | ✅ **修正済み** | `init.js:1` / `folder.js:50` / `toc.js:27` | 見出しスラグ: init/folder は重複・空IDを一意化しない、toc のみ一意化 → 同名/記号のみ見出しでアンカー破壊、TOC 開閉で挙動変化。→ init/folder の `addHeadingIds` にも一意化（`-2`/`-3`）＋空ID `'h'` フォールバックを追加し toc と揃えて修正。共通化（R3）は別途リファクタで実施予定。 |
| M3 | Medium | ✅ **修正済み** | `folder.js:194-212` | `loadPreview` が `r.ok` 未チェックで `"Not Found"` を本文に描画（init.js の `MdReload` は弾いている）。→ `r.ok ? r.text() : null` ＋ `if (html == null) return;` を移植。あわせて `MdSearch.reset()` を成功ブランチ内へ移動（読込失敗時に表示中ドキュメントの検索状態を消さない）。 |
| L1 | Low | ✅ **修正済み** | `request.rs:145` | `safe_join` の `rel.contains("..")` が `my..file.md` 等の正当パスを誤拒否。→ `Path::components()` で `Component::ParentDir` のみ弾く方式に変更。後段の canonicalize + starts_with が escape を別途捕捉するため安全。 |
| M2 | Medium | ⚠️ **未対応（過大評価）** | `main.rs:352-387` | single_file 監視が atomic save / canonicalize 失敗 / symlink でリロード取りこぼし、と当初記載。→ **再検証の結論: ほぼ誤り**。single_file モードは**親ディレクトリを NonRecursive 監視**しており、これは atomic save（temp→rename）への標準的対策そのもの。`single_file` は `main.rs:80` で canonicalize 済みのため symlink も解決済み、canonicalize 失敗時の fallback も watch 親パスが canonical なので通常一致する。残るのは「rename 中の一瞬 ＋ FSEvents の `/tmp`↔`/private/tmp` 差」の極端ケースのみ。実用上の取りこぼしはほぼ無く未対応。 |
| L2 | Low | ❌ **未対応（誤検知）** | `search.js:82-174` | 「リロード後に古い `matches[]` 残留」と当初記載。→ **再検証の結論: 対策済み**。`loadPreview`（folder.js）/ `MdReload`（init.js）の双方が本文差替**前**に `MdSearch.reset()` を呼び `matches[]` を空化しているため残留は起きない。下記「誤検知」へ移動。 |

### 誤検知だったもの（検証で否定・修正不要）
- **L2**: リロード時に `loadPreview` / `MdReload` が `MdSearch.reset()` を呼ぶため `matches[]` は空化済み。残留・外れ要素への `scrollIntoView` は発生しない。
- `percent_decode` の境界 `i + 2 < bytes.len()`（`request.rs:11`）は**正しい**。末尾 `%XX` も取りこぼさない。
- frontmatter パーサ（`html.rs:191`）は短い入力でも**パニックしない**。`&s[5..]` 等も ASCII ガード下で UTF-8 境界安全。

---

## 4. メモ
- JS は既存方針（ES5: `var` / `function` / IIFE）を尊重する。
- ステップ1〜3で地ならししてから R7（struct化）へ。
- C1 は信頼できない md を開く前提だと実害があるため、ステップ3を後回しにしすぎない。
