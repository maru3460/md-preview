# HTML iframe: 高さ自動フィット化ガイド（将来メモ）

`.html` の描画は現在 **ミニブラウザ方式**（iframe をビューポート高 `calc(100vh - 96px)` に固定し、
中身は iframe 内でスクロール）で実装している。これはシンプルで、`position: fixed` や
`height: 100vh` を使う実 HTML でも崩れないのが利点。

将来「Markdown 本文と同じく、外側と一体でスクロール（＝中身の高さに iframe を自動フィット）」に
したくなった場合の実装メモを残す。**現状はやらない判断**（下記の落とし穴が理由）。

## 見た目の違い

| | ミニブラウザ（現状） | 高さ自動フィット |
|---|---|---|
| スクロール | iframe 内で完結（窓内スクロール） | 外側ペインと一体でスクロール |
| スクロールバー | iframe のもの | 外側（`#preview-pane` / window）のもの |
| `md` との一貫性 | 別物（ミニブラウザ感） | md 本文と同じ挙動 |

## 実装スケッチ

前提: iframe は現状どおり sandbox 無し（同一オリジン）。同一オリジンなので親から
`contentDocument` を直接測れる。

1. iframe の CSS 固定高さ（`calc(100vh - 96px)`）をやめ、JS で実測値を書き込む。

   ```css
   .html-frame { display:block; width:100%; border:0; background:#fff; /* height は JS が設定 */ }
   ```

2. `common.js` の `bindFrame()` に高さ同期を足す（`onload` 後に実行）。

   ```js
   function syncHeight(frame) {
     var doc;
     try { doc = frame.contentDocument; } catch (e) { return; }
     if (!doc) return;
     var h = Math.max(doc.documentElement.scrollHeight, doc.body ? doc.body.scrollHeight : 0);
     h = Math.max(0, Math.min(h, 200000)); // 巨大/負値クランプ（レイアウト DoS 防止）
     frame.style.height = h + 'px';
   }
   ```

3. 後から高さが変わるケース（遅延画像・Web フォント・JS で DOM が伸びる）に追従するため
   `ResizeObserver` を張る。**iframe が差し替わるたびに `disconnect()` してリークを防ぐ**。

   ```js
   var ro = new ResizeObserver(function() { syncHeight(frame); });
   try { ro.observe(frame.contentDocument.documentElement); } catch (e) {}
   frame.__mdResizeObserver = ro; // 差し替え時に disconnect する用
   ```

4. スクロール保持は外側（`#preview-pane` / `scrollingElement`）の `scrollTop` を使う
   ＝既存のホットリロード時の保存/復元がそのまま効く（iframe 内部 scroll は使わない）。

## 落とし穴（これが「やらない」理由）

- **`height: 100vh` を使うページ**: vh = iframe 高さ = コンテンツ高さ、で無限に伸び続ける
  フィードバックループの定番事故。クランプしても振動しうる。
- **`position: fixed` / `sticky` ヘッダー**: iframe のビューポートがコンテンツ全高になるので、
  固定要素が文書先頭に貼り付いたまま流れて壊れる。
- **タイミング**: `onload` 前は subresource 未確定で過小、遅延読み込みで後からズレる。
  ResizeObserver 必須で、その寿命管理（差し替え時 disconnect）を誤るとリーク。
- **世代管理**: raw ⇄ 通常を連打すると古い iframe の `onload` が遅れて発火し、古い高さを
  書き得る。現 iframe との同一性チェックが要る。

要するに「素の文書」なら自動フィットが自然だが、HTML は「独自レイアウトを持つページ」で
あることが多く、ミニブラウザ方式の方が事故が少ない。切り替えるならページ種別を見て
出し分ける（例: 単純な文書は自動フィット、レイアウト持ちはミニブラウザ）まで踏み込む価値がある。
