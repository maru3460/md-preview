document.addEventListener('DOMContentLoaded', function() {
    MdCommon.ensureHeadingIds();
    hljs.highlightAll();
    MdCommon.addCopyButtons();
    MdCommon.runMermaid();
    MdCommon.runDrawio();
    if (window.MdSearch) {
        window.MdSearch.init(document.querySelector('.markdown-body') || document.body);
    }
    if (window.MdToc) {
        window.MdToc.init(document.scrollingElement || document.documentElement);
    }
    // stdin モードにはファイルが無いので diff トグルは出さない。
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
            MdCommon.runMermaid(article);
            MdCommon.runDrawio(article);
            if (window.MdSearch) {
                window.MdSearch.reset && window.MdSearch.reset();
                window.MdSearch.init(article);
            }
            if (window.MdToc) window.MdToc.refresh();
            if (window.MdDiff) window.MdDiff.refreshStat();
            scroller.scrollTop = savedScroll;
        })
        .catch(function() {});
}

window.MdReload = function() {
    // diff 表示中はファイル変更を diff の再取得に回す（本文には戻さない）。
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
