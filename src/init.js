document.addEventListener('DOMContentLoaded', function() {
    MdCommon.ensureHeadingIds();
    hljs.highlightAll();
    MdCommon.addCopyButtons();
    MdCommon.addLineNumbers();
    MdCommon.runMermaid();
    MdCommon.runDrawio();
    if (window.MdSearch) {
        window.MdSearch.init(document.querySelector('.markdown-body') || document.body);
    }
    if (window.MdToc) {
        window.MdToc.init(document.scrollingElement || document.documentElement);
    }
    // stdin モードにはファイルが無いので diff / raw トグルは出さない。
    if (window.MdDiff && window.MD_MENU_MODE !== 'stdin') {
        window.MdDiff.init({
            getContainer: function() { return document.querySelector('.markdown-body'); },
            getScroller: function() { return document.scrollingElement || document.documentElement; },
            getDiffUrl: function() { return '/?diff=1'; },
            getStatUrl: function() { return '/?diffstat=1'; },
            reloadNormal: loadNormalBody
        });
        window.MdDiff.refreshStat();
    }
    if (window.MdRaw && window.MD_MENU_MODE !== 'stdin') {
        window.MdRaw.init({
            getContainer: function() { return document.querySelector('.markdown-body'); },
            getScroller: function() { return document.scrollingElement || document.documentElement; },
            getRawUrl: function() { return '/?raw=1'; },
            reloadNormal: loadNormalBody
        });
        // 単一ファイルが .md 以外なら通常表示が既にソースなので raw は無効。
        // 非md表示は .markdown-body に source-page が付く（build_html 側）ことで判別する。
        var body = document.querySelector('.markdown-body');
        window.MdRaw.setAvailable(!(body && body.classList.contains('source-page')));
    }
    // html 表示（iframe）なら、フォーカスが iframe 内でもショートカットが効くよう配線する。
    // 単一ファイルモードは onLinkClick を渡さない＝iframe 内の相対遷移はそのまま許す。
    if (window.MdCommon && MdCommon.wireHtmlFrames) MdCommon.wireHtmlFrames(document);
    // コメント機能: 本文と現在ファイルの相対パスを渡して初期化する。stdin はファイルが
    // 無いので file 部は空（file:line を付けずファイル名だけで出す挙動になる）。
    if (window.MdComment) {
        window.MdComment.init({
            getContainer: function() { return document.querySelector('.markdown-body'); },
            getFile: function() { return window.MD_FILE_REL || ''; }
        });
    }
    setTimeout(function() { window.ipc.postMessage('ready'); }, 0);
});

function loadNormalBody() {
    var article = document.querySelector('.markdown-body');
    if (!article) return;
    var scroller = document.scrollingElement || document.documentElement;
    var savedScroll = scroller.scrollTop;
    fetch('/?body=1', { cache: 'no-store' })
        .then(function(r) { return r.ok ? r.text() : null; })
        .then(function(html) {
            if (html == null) return;
            article.innerHTML = html;
            MdCommon.ensureHeadingIds();
            if (window.hljs) hljs.highlightAll();
            MdCommon.addCopyButtons(article);
            MdCommon.addLineNumbers(article);
            MdCommon.runMermaid(article);
            MdCommon.runDrawio(article);
            if (window.MdSearch) {
                window.MdSearch.reset && window.MdSearch.reset();
                window.MdSearch.init(article);
            }
            if (window.MdToc) window.MdToc.refresh();
            if (window.MdDiff) window.MdDiff.refreshStat();
            if (window.MdCommon && MdCommon.wireHtmlFrames) MdCommon.wireHtmlFrames(article);
            // 本文差し替えでインラインのマーカーは消えるので、配列から貼り直す。
            if (window.MdComment) window.MdComment.reanchor();
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
document.addEventListener('keydown', function(e) {
    if (e.metaKey && e.key === 'w') {
        e.preventDefault();
        window.ipc.postMessage('close');
    }
});
document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href || href.charAt(0) !== '#') return;
    e.preventDefault();
    var id = decodeURIComponent(href.slice(1));
    var target = document.getElementById(id);
    if (target) target.scrollIntoView({ behavior: 'smooth' });
});
