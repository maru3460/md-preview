---
name: md-preview
description: マークダウンを書く時、ユーザーに見せるときに使う
---

# md-preview チートシート

`md` コマンドで起動する macOS 向けの Markdown ビューア。GitHub 風スタイルで WebKit ウィンドウに描画する。人に Markdown を見せたいとき・自分でレンダリング結果を確認したいときに使う。

主な機能: 検索（⌘F）・アウトライン（⌘T）・git 差分（⌘D）・raw ソース表示（⌘R）・本文へのコメント（`c`）・Mermaid / draw.io 図・テーマ・`.html` の iframe 描画・非 md ファイルのソースビュー。図のライブラリは図がある時だけ遅延ロードされるので通常の Markdown は軽いまま開ける。**キーボードだけで読み歩け**、フォルダを開けばファイルツリーから複数ファイルを行き来できる。`?` でショートカット一覧が出る。

## 開き方

`md` は GUI ウィンドウを開き、閉じるまでブロックする。必ずバックグラウンドで起動する。

```bash
md path/to/document.md &
```

表示モードは開くパスで決まる:

- カレントディレクトリ内のファイル → フォルダモード（ディレクトリツリーのサイドバー付き）
- カレントディレクトリ外のファイル（一時ディレクトリなど） → 単一ファイルモード（サイドバーなし）
- ディレクトリ（`md .` / `md docs/`） → そこをルートにしたフォルダモード

## 主なコマンド

```bash
md <file.md|ディレクトリ>   # ファイルかディレクトリを開く
cat file.md | md            # 標準入力（パイプ）から読む
md theme [<名前>]           # テーマ一覧の表示 / 切り替え
```

その他のオプション（`--sample`、`--version` など）は `md --help` を参照。

## 記法

GFM（テーブル・タスクリスト・打ち消し線・脚注・コードのシンタックスハイライト）に対応。加えて、リンク（`<url>` / WikiLink）、GFM アラート（`> [!NOTE]` など）、Mermaid / draw.io 図、`<details>` の折りたたみ、ファイル名つきコードブロック（```` ```rust:src/main.rs ````）、frontmatter を描画する。書き方の詳細は [references/notation.md](references/notation.md)。

## もっと詳しく（references/）

- [references/navigation.md](references/navigation.md) — キーボード操作（スクロール素キー・フォルダ内のファイル移動とツリー操作）、検索 / アウトライン / diff / raw の各機能、`.html` の iframe 描画と非 md のソースビュー
- [references/notation.md](references/notation.md) — 記法リファレンス（リンク・表・アラート・Mermaid・draw.io・折りたたみ・ファイル名つきコード・frontmatter）
- [references/comment.md](references/comment.md) — 本文へのコメント機能（`c`）の詳しい操作
