function article() {
    return document.querySelector('.markdown-body');
}
// スクロール主体。単一ファイル / stdin では document（#preview-pane が無いので
// MdCommon.getScroller はそこへ落ちる）。判定を書き写さず共有ヘルパに委ねる。
function pageScroller() {
    return MdCommon.getScroller();
}

document.addEventListener('DOMContentLoaded', function() {
    // 検索とアウトラインは、本文の後処理（hydrate）より先に init しておく必要がある
    // （hydrate は「作り直させる」だけで、作るのは各モジュールの init）。
    if (window.MdSearch) {
        window.MdSearch.init(article() || document.body);
    }
    if (window.MdToc) {
        window.MdToc.init(pageScroller());
    }
    // stdin モードにはファイルが無いので diff / raw トグルは出さない。
    // stdin にはファイルが無いので、ファイル前提のモード（diff / raw）は作らない。
    if (window.MD_MENU_MODE !== 'stdin' && window.MdViewModes) {
        window.MdViewModes.initAll({
            getContainer: article,
            getScroller: pageScroller,
            url: function(id) { return '/?' + id + '=1'; },
            getStatUrl: function() { return '/?diffstat=1'; },
            reloadNormal: loadNormalBody
        });
        // 単一ファイルが .md 以外なら通常表示が既にソースなので raw は無効。
        // 非md表示は .markdown-body に source-page が付く（build_html 側）ことで判別する。
        var body = article();
        window.MdRaw.setAvailable(!(body && body.classList.contains('source-page')));
        window.MdDiff.refreshStat();
    }
    // コメント機能: 本文と現在ファイルの相対パスを渡して初期化する。stdin はファイルが
    // 無いので file 部は空（file:line を付けずファイル名だけで出す挙動になる）。
    if (window.MdComment) {
        window.MdComment.init({
            getContainer: article,
            getFile: function() { return window.MD_FILE_REL || ''; }
        });
    }
    // 初回描画の後処理。単一ファイルモードは onLinkClick を渡さない
    // ＝ iframe 内の相対遷移はそのまま iframe に任せる。
    MdCommon.hydrate(article() || document.body);
    setTimeout(function() { window.ipc.postMessage('ready'); }, 0);
});

function loadNormalBody() {
    var el = article();
    if (!el) return;
    var scroller = pageScroller();
    var savedScroll = scroller.scrollTop;
    fetch('/?body=1', { cache: 'no-store' })
        .then(function(r) { return r.ok ? r.text() : null; })
        .then(function(html) {
            if (html == null) return;
            el.innerHTML = html;
            MdCommon.hydrate(el);
            if (window.MdDiff) window.MdDiff.refreshStat();
            scroller.scrollTop = savedScroll;
        })
        .catch(function() {});
}

window.MdReload = function() {
    // raw / diff 表示中はファイル変更をその再取得に回す（本文には戻さない）。
    if (window.MdRaw && window.MdRaw.isActive()) { window.MdRaw.refresh(); return; }
    if (window.MdDiff && window.MdDiff.isActive()) { window.MdDiff.refresh(); return; }
    loadNormalBody();
};
// 単一ファイルモードで扱えるリンクはページ内アンカーだけ（別ファイルへ移る先が無い）。
// ⌘W と ⌘A は common.js が keymap 経由で処理する。
document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    MdCommon.scrollToAnchor(a.getAttribute('href'), e);
});
