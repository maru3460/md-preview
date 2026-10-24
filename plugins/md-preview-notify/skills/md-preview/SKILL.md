---
name: md-preview
description: md-preview(mdコマンド)の使い方
argument-hint: "[--stdin | -s] [--direct | -d]"
---

# md-preview

`md` コマンドで起動する macOS 用 Markdown ビューア。
Markdown, HTML をユーザーに見せる際に使う。

## 基本的な使い方

```bash
md --notify --title '<何の文書か>' --description '<どの作業の何か>' path/to/document.md
md --notify --title '<何の文書か>' --description '<どの作業の何か>' a.md a.html
```

上記によりmdに通知が送られ、通知を見たユーザーが非同期でファイルを閲覧する。
複数ファイル作成した場合は一括で通知可能。

### 名乗る

`--notify` には `--title` と `--description` を付けて、非同期でファイルを閲覧するユーザーに情報を渡す。
開いたあとに読んでほしい補足があれば、`--note` も付ける。

```bash
md --notify plan.md --title '<何の文書か>' --description '<どの作業の何か>' --note '<補足>'
```

- `--title`：何の文書か。目安は10 字。
- `--description`：どの作業の何か。目安は20 字。
- `--note`：開いた人が読む補足。目安は50 字。

## パイプ

スキルが `--stdin` / `-s` 付きで呼ばれたら、標準入力で渡す

```bash
md --notify --title '<何の文書か>' --description '<どの作業の何か>' <<'EOF'
# 見出し
本文
EOF
```

## 直接開く

スキルが `--direct` / `-d` 付きで呼ばれたら、`--notify`をつけない。
この時フォーカスがmd-previewのウィンドウに移る。

```bash
md path/to/document.md
```

## 文書の JavaScript とネットの読み込み

md は設定「文書を信頼する」が OFF だと、文書の JavaScript とネットからの読み込みを止める。
文書を書く前に設定を確かめ、OFF なら [references/restrictions.md](references/restrictions.md) を読む。

```bash
grep -s '^trust-documents' ~/.config/md-preview/settings   # `trust-documents=on` が出なければ OFF
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
- [references/restrictions.md](references/restrictions.md)
設定「文書を信頼する」が OFF の窓に出す文書を書くときに読む
