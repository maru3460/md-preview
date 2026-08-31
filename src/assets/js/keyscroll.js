// 本文スクロールの素キー（vim/less 流）の実処理。キーの割り当てと「どの文脈で効くか」は
// keymap.js の表が持つので、ここは「どこをスクロールするか」だけを決める。
//
// スクロール主体は MdCommon.getScroller()（シェル=#preview-pane / 1 枚もの=document）。
// ツリーにフォーカスがある間と、錨れる行があるコメントモード中の j/k は keymap.js 側の
// when で除外済み（錨れない表示——html の iframe / git 差分 / 巨大ソース——では戻ってくる）。
(function() {
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

  if (window.MdKeymap) {
    // スクロール素キーの実体は MdCommon.applyScrollKey（html iframe 側と共用）。
    MdKeymap.on('scroll', function(e) {
      MdCommon.applyScrollKey(e, frameScroller() || MdCommon.getScroller());
    });
    MdKeymap.on('search-open', function() {
      if (window.MdSearch && MdSearch.open) MdSearch.open();
    });
  }
})();
