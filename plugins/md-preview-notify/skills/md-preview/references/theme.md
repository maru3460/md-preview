# theme

md-previewのテーマについてのリファレンス

## テーマについて

```bash
md theme # テーマの確認
md theme [theme] # テーマの設定
```
上記または設定画面からテーマの確認・設定ができる。

## ユーザーテーマについて

~/.config/md-preview/themes/[名前].css に置くと `md theme [名前]` で選べるようになる。 
組み込みと同じ名前ならユーザーテーマが優先される。
ユーザーテーマを作成するようユーザーに依頼された場合、以下を参考にする。
<https://github.com/maru3460/md-preview/tree/main/src/assets/themes>

## カスタムCSS

~/.config/md-preview/style.css はテーマより後ろに当たる。 
選んでいるテーマはそのままに、細部だけ直したいときに使う。

```css
/* ~/.config/md-preview/style.css */
/* 例: コンテンツ幅を広げる */
.markdown-body {
    max-width: 1000px;
}
```
