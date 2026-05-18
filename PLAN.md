# md-preview ビルトインテーマ機能 実装計画書

## 1. 概要

### 何を作るか
md-preview に**ビルトインテーマ機能**を導入し、`--theme=<name>` または `md config theme <name>` で見た目を切り替えられるようにする。CSS は `src/themes/*.css` として複数同梱し、`include_str!` でバイナリに埋め込む。

### ゴール
- CLI フラグでの**一時適用**と、設定ファイルでの**永続化**を両立
- 既存ユーザーの `~/.config/md-preview/style.css` を**壊さない**（互換性最優先）
- 「テーマ」は CSS 変数のオーバーライド方式を主軸にして**保守コストを抑える**

### ユーザー体験 before / after

**Before（現状）**
```sh
md README.md                         # GitHub 風固定
# 見た目を変えたい → ~/.config/md-preview/style.css を自分で書く必要あり
```

**After（提案）**
```sh
md --list-themes                     # 利用可能テーマ一覧
md --theme=dracula README.md         # 今だけ Dracula
md config theme dracula              # 以後ずっと Dracula
md README.md                         # → Dracula で開く
md --theme=default README.md         # config を無視して default で開く（CLI > config）
md config get theme                  # 現在の設定確認
```

---

## 2. コマンド設計（最終形）

### 既存（維持）
- `md <file.md>` / `md <dir>` / `md .` / `md --sample`

### 新規フラグ
- `md --theme=<name>` / `md --theme <name>` — 一回限り適用（config より優先）
- `md --list-themes` — ビルトインテーマ一覧
- `md --help` / `-h`、`md --version` / `-V` — clap で自動

### 新規サブコマンド `md config`
- `md config theme <name>` — 糖衣構文（最頻ユースケース）
- `md config get <key>`
- `md config set <key> <value>`
- `md config unset <key>`
- `md config list`
- `md config path`

key は初期は `theme` のみ。将来 `window.width` 等を追加する余地を残す。

### 終了コード
- 正常: `0`
- 引数エラー / 不明テーマ（CLI 指定時）: `2`
- ファイル/ディレクトリ I/O エラー: `1`

---

## 3. 設定ファイル

### パス
- 第一候補: `$XDG_CONFIG_HOME/md-preview/config.toml`
- フォールバック: `~/.config/md-preview/config.toml`

既存の `~/.config/md-preview/style.css` と同じディレクトリに置く（発見性◎）。

### 形式（TOML）
```toml
# ~/.config/md-preview/config.toml
theme = "dracula"

# 将来用
# [window]
# width = 1200
```

### 失敗時の挙動
| ケース | 挙動 |
|---|---|
| ファイル無し | 静かにスキップ |
| パース失敗 | stderr に warning → defaults で起動継続 |
| 未知のキー | 警告して無視 |
| 不明テーマ名 (config) | warning + default フォールバック |
| 不明テーマ名 (CLI) | exit 2（厳格） |

### 書き込み（`md config set`）
- ディレクトリは `create_dir_all`
- 読み → パース → 差し替え → 書き戻し
- 初期実装ではコメントは保持しない（必要になったら `toml_edit` 検討）

---

## 4. 適用優先順位

```
[1] デフォルト style.css                          ← 基盤（常に最初）
        ↓
[2] ビルトインテーマ CSS（解決ルール: 下記）       ← 変数・色の上書き
        ↓
[3] ~/.config/md-preview/style.css （ユーザー手書き） ← 最強（既存互換）
```

### テーマ解決ルール
```
if --theme=NAME 指定:
    → NAME を使う（不明なら警告 + exit 2）
elif config.theme:
    → config の値を使う（不明なら警告 + default フォールバック）
else:
    → default
```

`CLI > config > なし`（git/cargo/npm 慣例）。

### HTML 内 `<style>` 順序
```html
<style>{CSS}</style>                <!-- 基盤 -->
<style>{HLJS_LIGHT_CSS}</style>
<style>@media(prefers-color-scheme:dark){HLJS_DARK_CSS}</style>
<style>{THEME_CSS}</style>          <!-- 新規 -->
<style>{custom_css}</style>         <!-- ユーザーCSS は最強のまま -->
```

ユーザーCSSが**テーマより後ろ**にあることが既存互換の鍵。

---

## 5. ファイル構成と変更箇所

### 新規ファイル
| ファイル | 役割 |
|---|---|
| `src/cli.rs` | clap derive で `Cli` struct |
| `src/config.rs` | `Config` struct と `load/save/path/get/set/unset` |
| `src/themes.rs` | `available_themes()`, `get_theme_css(name)`, `resolve_theme()` |
| `src/themes/default.css` | 空（フォールバック用） |
| `src/themes/soft-paper.css` | 紙風・indigo |
| `src/themes/github-dark.css` | GitHub Dark |
| `src/themes/dracula.css` | Dracula |
| `src/themes/nord.css` | Nord |
| `src/themes/solarized-light.css` | Solarized Light |

### 既存ファイル変更（行番号付き）
- `Cargo.toml:10-15` — `clap`, `toml` 追加
- `src/main.rs:11-15` — `mod cli; mod config; mod themes;`
- `src/main.rs:27-50` — 引数パース全面差し替え + テーマ解決
- `src/main.rs:67` — `build_folder_html(&title, theme_css, &custom_css, None)`
- `src/main.rs:80` — 同上
- `src/main.rs:97` — `build_html(..., theme_css, &custom_css)`
- `src/main.rs:131-153` — クロージャに `theme_css` キャプチャ
- `src/main.rs:151` — `handle_request(..., theme_css, &custom_css, ...)`
- `src/html.rs:128` — `build_html` シグネチャ + 本文
- `src/html.rs:138付近` — `<style>{custom_css}</style>` の直前に `<style>{theme_css}</style>` 挿入
- `src/html.rs:220` — `build_folder_html` シグネチャ + 本文
- `src/html.rs:253付近` — 同上
- `src/request.rs:226-238` — `handle_asset` に `theme_css` 引数
- `src/request.rs:248-272` — `handle_request` シグネチャに `theme_css`

---

## 6. 実装ステップ（PR 分割可能な単位）

1. **CSS 変数の導入（破壊的変更なし）** — `src/style.css` の色を `var(--md-*)` に置換。挙動同一を確認
2. **テーマモジュール骨組み** — `src/themes.rs` + `default.css`（空）。ユニットテストだけ
3. **`build_html` / `build_folder_html` に `theme_css` 引数追加** — 全呼び出し更新、現状は `""` を渡す
4. **引数パーサ刷新（CLI モジュール）** — clap で `--theme`, `--list-themes`, `config <sub>`
5. **設定ファイル read-only** — `Config::load()` / `path()`。**ここで MVP 完成**（手書き config.toml で動く）
6. **ビルトインテーマ 1 個目（soft-paper）** — `md --theme=soft-paper sample.md` 動作確認
7. **`md config` 書き込み実装** — `save/set/unset/list`
8. **残りのテーマ追加** — github-dark, dracula, nord, solarized-light
9. **エラーハンドリング・UX 仕上げ** — 不明テーマの suggest（編集距離≤2）、help 文言
10. **ドキュメント・テスト** — README 更新、ユニットテスト、`Cargo.toml` を `0.2.0` に bump

---

## 7. 依存追加の判断

### clap：**採用推奨**
- Pros: サブコマンド + 複数フラグの整理、`--help` / `--version` 無料、git/cargo 風 UX
- Cons: バイナリサイズ +200KB〜500KB（未確認）、コンパイル時間増
- 判断: UX 価値が大きい。`features = ["derive"]`

### toml：**採用推奨**
- Pros: 標準形式、配列・テーブルで将来拡張に強い
- Cons: 依存 1 個増、コメント保持不可
- 判断: `toml = { version = "0.8", default-features = false, features = ["parse", "display"] }`

### dirs：**不要**
- XDG 解決は 5 行で書ける

### `Cargo.toml` 変更分
```toml
[dependencies]
# ...既存...
clap = { version = "4", features = ["derive"] }
toml = { version = "0.8", default-features = false, features = ["parse", "display"] }
```

---

## 8. ビルトインテーマ案

| 名前 | 方向性 | 元ネタ |
|---|---|---|
| `default` | 現状の GitHub 風（OS 追従） | GitHub Primer |
| `soft-paper` | 温かい紙風 + indigo | オリジナル |
| `github-dark` | GitHub Dark Default | GitHub Primer Dark |
| `dracula` | 紫系定番ダーク | draculatheme.com |
| `nord` | 北欧ブルーのダーク | nordtheme.com |
| `solarized-light` | 黄褐色ライト | ethanschoonover.com/solarized |

### テーマ CSS の書き方：CSS 変数オーバーライド方式
```css
/* src/themes/dracula.css */
:root {
    --md-bg: #282a36;
    --md-fg: #f8f8f2;
    --md-link: #8be9fd;
    --md-code-bg: #44475a;
    --md-blockquote-fg: #bd93f9;
    /* ...etc */
    color-scheme: dark;
}
```
各テーマ概ね 30〜80 行で済む。

### `prefers-color-scheme: dark` との関係
- `default` テーマ = 空 CSS → 既存の OS 追従ロジックがそのまま生きる
- 固定ダーク系（dracula 等）= `color-scheme: dark` 宣言で OS 設定無視

---

## 9. エッジケース・エラーハンドリング

| ケース | 挙動 |
|---|---|
| `--theme=知らない名前` (CLI) | exit 2 + Available 一覧表示 |
| `config.theme = "知らない名前"` | warning + default フォールバック（起動続行） |
| `config.toml` パース失敗 | warning + defaults |
| `config.toml` 無し | 静かにスキップ |
| ディレクトリ無い状態で `set` | `create_dir_all`、失敗時 exit 1 |
| 書き込み権限なし | exit 1 + エラー表示 |
| `config get <未定義key>` | exit 1（git 慣例） |
| ユーザー CSS とテーマ競合 | ユーザー CSS が後勝ち（仕様、README に明記） |
| `--list-themes` と `--theme` 同時 | `--list-themes` 優先 |
| `--sample` と `--theme` 同時 | `--sample` 優先（テーマ無関係） |

### 候補 suggest（任意・小さな実装で UX 大）
編集距離 ≤ 2 で最大 3 件:
```
error: unknown theme 'drakula'

Did you mean:
  - dracula

Run 'md --list-themes' to see all available themes.
```

---

## 10. テスト戦略

### ユニットテスト
- `themes`: `available_themes()`, `get_theme_css()`, `resolve_theme()` の優先順位
- `config`: TOML パース、空ファイル、ラウンドトリップ (load→set→save→load)
- `cli`: 各引数パターンが期待モードに変換される
- `html`: `<style>theme_css</style>` が `<style>user_css</style>` より前にある

### 手動確認（macOS 実機）
全テーマで `md --sample > /tmp/sample.md && md --theme=NAME /tmp/sample.md` を実行し、以下が崩れないこと:
- 見出し H1-H4、段落、リンク
- インラインコード、コードブロック（hljs）
- アラート全 5 種、テーブル、blockquote
- フォルダモードのサイドバー
- Mermaid、Cmd+F、ファイル名付きコードブロック

加えて：
- OS のライト/ダーク切替で `default` が追従、`dracula` 等は固定
- ユーザー CSS との共存（`body { padding: 100px; }` がテーマ適用時にも効く）
- ホットリロードがテーマ適用後も動作

### 回帰確認
Step 1 の変数リファクタ後、テーマなしで `md sample.md` のスクリーンショット pixel diff（または目視）。

---

## 11. 将来の拡張余地

| アイデア | 概要 |
|---|---|
| ユーザー定義テーマ | `~/.config/md-preview/themes/<name>.css` を `--theme=<name>` で参照 |
| `--theme=auto` | ライト/ダーク自動切替（ペア設定） |
| テーマ切替ホットキー | Cmd+T 等で実行中ローテーション（`evaluate_script` で `<style>` 差し替え） |
| hljs テーマ連動 | テーマと syntax highlight を一致 |
| `[window]` 設定 | `width`, `height` を config から |
| フォント設定 | `font_family`, `font_size` 変数化 |
| Mermaid テーマ連動 | `themeVariables` をテーマと一致 |
| `md config edit` | `$EDITOR` で `config.toml` を開く |

---

## 12. ユーザー向けドキュメント変更

### README.md への追記

**使い方サンプル**（`--sample` の下）:
```
md --theme=dracula README.md  # 一回限り Dracula で表示
md --list-themes              # 利用可能テーマ一覧
md config theme dracula       # 以後ずっと Dracula
```

**「カスタムCSS」セクションの直前**に新規セクション「テーマ」:

````markdown
## テーマ

ビルトインテーマを選べます。

```sh
md --list-themes              # 一覧表示
md --theme=soft-paper file.md # 一回だけ適用
md config theme dracula       # 永続化
md config theme default       # デフォルトに戻す
```

利用可能なテーマ:
- `default` — GitHub 風（OS のダークモード設定に追従）
- `soft-paper` — 温かみのある紙風、indigo アクセント
- `github-dark` — GitHub Dark Default
- `dracula` — 紫系の定番ダーク
- `nord` — 北欧ブルーのダーク
- `solarized-light` — 黄褐色系のライト

設定は `~/.config/md-preview/config.toml` に保存されます。`md config path` でパスを表示できます。

### 適用優先順位
1. `--theme=NAME` (コマンドライン)
2. `config.toml` の `theme`
3. デフォルト

ユーザーCSS (`~/.config/md-preview/style.css`) はテーマよりも後に適用されるので、テーマを使いつつ部分的な上書きも可能です。
````

---

## 不確実な点（未確認）

- clap 追加によるバイナリサイズ・コンパイル時間の具体的なインパクト（未計測）
- `src/style.css` 645 行全体の変数化の完全性（SVG mask URL `style.css:392-403` 付近は色値含まないので影響なしと判断、ただし全行レビュー未完）
- Mermaid 図のテーマ連動は今回スコープ外。Phase 2 推奨
- `wry` ランタイムでの CSS 変数再評価は MVP では関係なし（起動時固定）。ホットキー切替実装時に検証必要
- ユーザーが既に `--md-*` プレフィックスを使っていた場合の衝突（現実的にはほぼ無し、`--md-*` を予約と明記）
