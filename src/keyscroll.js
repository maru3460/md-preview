// 本文スクロールの素キー（vim/less 流）を全モード（stdin / single / folder）で有効化する。
// スクロール主体は MdCommon.getScroller()（folder=#preview-pane / 単一ファイル=document）。
//
// folder モードのツリー操作（folder.js）とは「サイドバーにフォーカスがあるか」で排他する。
// ここではサイドバーにフォーカスがある間は一切動かないので、j/k 等はツリー側が担当する。
// FOLDER_JS は document-start 注入でこのスクリプトより先に評価されるが、両者はイベント順に
// 依存せず「フォーカスの所在」で排他しているので、stopPropagation には頼らない。
(function() {
  var LINE = 40; // 1 行ぶんのスクロール量(px)

  function scroller() {
    if (window.MdCommon && MdCommon.getScroller) return MdCommon.getScroller();
    return document.scrollingElement || document.documentElement;
  }
  function by(dy) { var s = scroller(); if (s) s.scrollTop += dy; }
  function to(top) { var s = scroller(); if (s) s.scrollTop = top; }
  function page() { var s = scroller(); return s ? s.clientHeight : 600; }

  document.addEventListener('keydown', function(e) {
    // ⌘/Ctrl/Alt 併用は既存ショートカット等に委ねる（Shift は Space/G で使うので許可）。
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (window.MdCommon) {
      // ツリーにフォーカスがある間はツリー操作（folder.js）の担当。
      if (MdCommon.isSidebarFocused()) return;
      // 検索バー / ヘルプ / 右クリックメニュー表示中は背後を動かさない。
      if (MdCommon.isOverlayOpen()) return;
      // 入力欄・ボタン・リンクにフォーカス中は素キーをそちらへ委ねる。
      if (MdCommon.isInteractiveFocus()) return;
    }
    // コメントモード中の j/k はユニット・カーソル移動（comment.js）に譲る。
    // d/u/Space/g/G のページ送りはモード中もそのまま効かせる。
    if (window.MdComment && MdComment.isMode && MdComment.isMode() && (e.key === 'j' || e.key === 'k')) return;

    switch (e.key) {
      case 'j': by(LINE); break;
      case 'k': by(-LINE); break;
      case 'd': by(page() / 2); break;
      case 'u': by(-page() / 2); break;
      // 1 ページ送りは 0.9 ページぶん。1 割を残して直前の文脈を画面に留め、読み位置を見失わせない。
      case ' ': by((e.shiftKey ? -1 : 1) * page() * 0.9); break;
      case 'g': to(0); break;
      case 'G': { var s = scroller(); if (s) s.scrollTop = s.scrollHeight; break; } // 末尾へ（clampで下端に収まる）
      case '/':
        if (window.MdSearch && MdSearch.open) MdSearch.open();
        break;
      default:
        return; // 未対応キーは preventDefault せず通常動作に任せる
    }
    e.preventDefault();
  });
})();
