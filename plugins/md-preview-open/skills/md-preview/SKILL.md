---
name: md-preview
description: md-preview(mdコマンド)の使い方
argument-hint: "[--stdin | -s] [--notify | -n]"
---

# md-preview

`md` コマンドで起動する macOS 用 Markdown ビューア。
Markdown, HTML をユーザーに見せる際に使う。

## 基本的な使い方

```bash
md path/to/document.md
md a.md a.html
```

上記によりmdの窓が前に出て、ファイルがタブで開く。
窓が既に開いていれば、新しい窓は増えずにその窓のタブとして届く。
複数ファイル作成した場合は、1 回の `md` にまとめて渡す。

## パイプ

スキルが `--stdin` / `-s` 付きで呼ばれたら、標準入力で渡す

```bash
md --title '<何の文書か>' <<'EOF'
# 見出し
本文
EOF
```

## 通知

スキルが `--notify` / `-n` 付きで呼ばれたら、`--notify`をつける
`notify`をつける場合ユーザーがファイルを見るのは非同期なのでtitleとdescriptionをつける

```bash
md --notify --title '<何の文書か>' --description '<どの作業の何か>' path/to/document.md
```

## 注意事項

- 一時的な指示ではなく永続的に、開かずに通知だけしてほしいと依頼された場合、
プラグインを`md-preview-open`から`md-preview-notify`に切り替えるよう提案する。


## リファレンス

- [references/syntax.md](references/syntax.md)
md に渡す文書を書く前に読む。
- [references/keymap.md](references/keymap.md)
ユーザーに md の操作を聞かれたときに読む
- [references/theme.md](references/theme.md)
テーマや見た目を変えるよう頼まれたときに読む
