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
    setTimeout(function() { window.ipc.postMessage('ready'); }, 0);
});

window.MdReload = function() {
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
            scroller.scrollTop = savedScroll;
        })
        .catch(function() {});
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
