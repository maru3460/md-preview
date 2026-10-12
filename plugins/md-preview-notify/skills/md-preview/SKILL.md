---
name: md-preview
description: md-preview(mdコマンド)の使い方
argument-hint: [--pipe | -p]
---

# md-preview

`md` コマンドで起動する macOS 用 Markdown ビューア。
Markdown, HTML をユーザーに見せる際に使う。

## 基本的な使い方

```bash
md --notify path/to/document.md
md --notify a.md a.html
```

上記によりmdに通知が送られ、通知を見たユーザーが非同期でファイルを閲覧する。
複数ファイル作成した場合は一括で通知可能。

## パイプ

スキルが `--pipe` / `-p` 付きで呼ばれたら、標準入力で渡す

適当な場所(scratchpad等)にマークダウンを作成した上で

```bash
cat /path/to/file | md --notify
```

もしくはマークダウンを作成せずに

```bash
md --notify <<'EOF'
# 見出し
本文
EOF
```

## 注意事項

- `--notify`オプションは必須。
これがない場合画面遷移が発生し、ユーザーの邪魔となる。
- 一時的な指示ではなく永続的に`--notify`をつけないよう依頼された場合、
プラグインを`md-preview-notify`から`md-preview-open`に切り替えるよう提案する。


## リファレンス

- [references/syntax.md](references/syntax.md)
md に渡す文書を書く前に読む。
- [references/keymap.md](references/keymap.md)
ユーザーに md の操作を聞かれたときに読む
- [references/theme.md](references/theme.md)
テーマや見た目を変えるよう頼まれたときに読む
