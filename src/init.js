function addHeadingIds() {
    document.querySelectorAll('h1,h2,h3,h4,h5,h6').forEach(function(h) {
        if (h.id) return;
        var base = h.textContent
            .toLowerCase()
            .replace(/[^\p{L}\p{N}\s-]/gu, '')
            .trim()
            .replace(/\s+/g, '-');
        if (!base) base = 'h';
        var id = base;
        var n = 2;
        while (document.getElementById(id)) {
            id = base + '-' + (n++);
        }
        h.id = id;
    });
}

function addCopyButtons(root) {
    var scope = root || document;
    scope.querySelectorAll('pre > code').forEach(function(code) {
        var pre = code.parentNode;
        if (pre.classList.contains('mermaid')) return;
        var wrapper;
        if (pre.parentNode && pre.parentNode.classList && pre.parentNode.classList.contains('code-wrapper')) {
            wrapper = pre.parentNode;
        } else {
            wrapper = document.createElement('div');
            wrapper.className = 'code-wrapper';
            pre.parentNode.insertBefore(wrapper, pre);
            wrapper.appendChild(pre);
        }
        if (wrapper.querySelector(':scope > .copy-btn')) return;
        var btn = document.createElement('button');
        btn.className = 'copy-btn';
        btn.type = 'button';
        btn.setAttribute('aria-label', 'Copy code');
        btn.textContent = 'Copy';
        btn.addEventListener('click', function() {
            var text = code.innerText;
            navigator.clipboard.writeText(text).then(function() {
                btn.textContent = 'Copied';
                btn.classList.add('copied');
                setTimeout(function() {
                    btn.textContent = 'Copy';
                    btn.classList.remove('copied');
                }, 1200);
            }).catch(function() {
                btn.textContent = 'Failed';
                setTimeout(function() { btn.textContent = 'Copy'; }, 1200);
            });
        });
        wrapper.appendChild(btn);
    });
}

var MdLibs = (function() {
    var cache = {};
    function load(url) {
        if (cache[url]) return cache[url];
        cache[url] = new Promise(function(resolve, reject) {
            var s = document.createElement('script');
            s.src = url;
            s.onload = function() { resolve(); };
            s.onerror = function() { reject(); };
            document.head.appendChild(s);
        });
        return cache[url];
    }
    return { load: load };
})();

function runMermaid(root) {
    var scope = root || document;
    var nodes = scope.querySelectorAll('pre.mermaid');
    if (!nodes.length) return;
    MdLibs.load('mdpreview://localhost/__lib/mermaid.min.js').then(function() {
        if (typeof mermaid === 'undefined') return;
        if (!mermaid.__mdInit) {
            var ap = window.MD_APPEARANCE || 'auto';
            var dark = ap === 'dark' || (ap === 'auto' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
            mermaid.initialize({
                startOnLoad: false,
                theme: dark ? 'dark' : 'neutral',
                securityLevel: 'loose'
            });
            mermaid.__mdInit = true;
        }
        try { mermaid.run({ nodes: nodes }); } catch (e) {}
    }).catch(function() {});
}

function runDrawio(root) {
    var scope = root || document;
    var nodes = scope.querySelectorAll('.mxgraph');
    if (!nodes.length) return;
    MdLibs.load('mdpreview://localhost/__lib/drawio-viewer.min.js').then(function() {
        if (typeof GraphViewer === 'undefined') return;
        try { GraphViewer.processElements(); } catch (e) {}
    }).catch(function() {});
}

document.addEventListener('DOMContentLoaded', function() {
    addHeadingIds();
    hljs.highlightAll();
    addCopyButtons();
    runMermaid();
    runDrawio();
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
            addHeadingIds();
            if (window.hljs) hljs.highlightAll();
            addCopyButtons(article);
            runMermaid(article);
            runDrawio(article);
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
