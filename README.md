# md-preview

MarkdownファイルをmacOSのネイティブウィンドウで表示するだけのツール。

```sh
md README.md
```

macOS WebKit（[wry](https://github.com/nicobao/nicobao-fork-tauri-apps-wry)）を使ったレンダリングで、軽量・高速。テーブル、タスクリスト、取り消し線、脚注、ダークモードに対応。

## インストール

[Rust](https://rustup.rs/) が必要です。

```sh
cargo install --path .
```

`~/.cargo/bin/md` としてインストールされます。`PATH` に含まれていることを確認してください。

## アンインストール

```sh
cargo uninstall md-preview
```

## 使い方

```sh
md path/to/file.md   # 単ファイルモード（900px幅）
md .                 # フォルダモード（カレントディレクトリをサイドバー表示）
md docs/             # フォルダモード（指定ディレクトリをサイドバー表示）
```

カレントディレクトリ内のファイルを指定した場合はフォルダモード（1200px幅）で開き、そのファイルを初期表示します。

```sh
cd ~/docs
md intro.md   # フォルダモードで開き、intro.md をプレビュー
```

GitHub風のスタイルでネイティブウィンドウに表示されます。

## フォルダモード

サイドバーにディレクトリツリーが表示されます。ファイルをクリックするとプレビューペインに表示されます。Markdownファイル内の相対リンクもプレビューペイン内で遷移します。

## カスタムCSS

`~/.config/md-preview/style.css` にCSSファイルを置くと、デフォルトのスタイルを上書き・拡張できます。

```css
/* 例: コンテンツ幅を広げる */
.markdown-body {
    max-width: 1000px;
}
```

## 対応するMarkdown機能

- 見出し、段落、リンク、画像（相対パス・data URI）
- **太字**、*斜体*、~~取り消し線~~
- コードブロック・インラインコード（シンタックスハイライト）
- テーブル
- タスクリスト
- 引用
- 脚注
- YAML front matter
- 水平線
- ダークモード（システム設定に追従）

## 制限事項

- macOS専用
- 外部リンク（http/https）はクリックするとデフォルトブラウザで開く
- 読み取り専用（ファイル変更時の自動リロードなし）

## ライセンス

MIT
