// 本文スクロールの素キー（vim/less 流）を全モード（stdin / single / folder）で有効化する。
// スクロール主体は MdCommon.getScroller()（folder=#preview-pane / 単一ファイル=document）。
//
// folder モードのツリー操作（folder.js）とは「サイドバーにフォーカスがあるか」で排他する。
// ここではサイドバーにフォーカスがある間は一切動かないので、j/k 等はツリー側が担当する。
// FOLDER_JS は document-start 注入でこのスクリプトより先に評価されるが、両者はイベント順に
// 依存せず「フォーカスの所在」で排他しているので、stopPropagation には頼らない。
(function() {
  function scroller() {
    if (window.MdCommon && MdCommon.getScroller) return MdCommon.getScroller();
    return document.scrollingElement || document.documentElement;
  }

  // html を iframe 描画している時、スクロールすべき対象は iframe 内の文書である。
  // 親（body / #preview-pane）は html-frame が丁度 1 画面ぶんなので動かず、フォーカスが
  // まだ親にあるうち（開いた直後・iframe 外をクリックした後）は素キーが死んでしまう。
  // フォーカスが iframe 内にある場合はこのハンドラ自体が呼ばれず、iframe 側の
  // bindFrame が同じ applyScrollKey を呼ぶ（結果はどちらでも同じ）。
  // html-frame は html ファイル表示のときだけ存在し、md 本文には現れない。
  function frameScroller() {
    var f = document.querySelector('iframe.html-frame');
    if (!f) return null;
    try {
      var d = f.contentDocument;
      var sc = d && (d.scrollingElement || d.documentElement);
      // スクロールできない（短い）中身なら親に任せる。
      return sc && sc.scrollHeight > sc.clientHeight ? sc : null;
    } catch (e) { return null; } // 外部サイトへ遷移した等で cross-origin
  }

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

    if (e.key === '/') {
      if (window.MdSearch && MdSearch.open) MdSearch.open();
      e.preventDefault();
      return;
    }
    // スクロール素キーの実体は MdCommon.applyScrollKey（html iframe 側と共用）。
    // 未対応キーは preventDefault されず通常動作に任される。
    if (window.MdCommon && MdCommon.applyScrollKey) {
      MdCommon.applyScrollKey(e, frameScroller() || scroller());
    }
  });
})();
