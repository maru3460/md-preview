function addHeadingIds() {
    document.querySelectorAll('h1,h2,h3,h4,h5,h6').forEach(function(h) {
        if (!h.id) {
            h.id = h.textContent
                .toLowerCase()
                .replace(/[^\p{L}\p{N}\s-]/gu, '')
                .trim()
                .replace(/\s+/g, '-');
        }
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

function runMermaid(root) {
    if (typeof mermaid === 'undefined') return;
    var scope = root || document;
    var nodes = scope.querySelectorAll('pre.mermaid');
    if (!nodes.length) return;
    try {
        mermaid.run({ nodes: nodes });
    } catch (e) {}
}

function initMermaid() {
    if (typeof mermaid === 'undefined') return;
    var dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
    mermaid.initialize({
        startOnLoad: false,
        theme: dark ? 'dark' : 'default',
        securityLevel: 'loose'
    });
}

document.addEventListener('DOMContentLoaded', function() {
    addHeadingIds();
    hljs.highlightAll();
    addCopyButtons();
    initMermaid();
    runMermaid();
    if (window.MdSearch) {
        window.MdSearch.init(document.querySelector('.markdown-body') || document.body);
    }
    setTimeout(function() { window.ipc.postMessage('ready'); }, 0);
});
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
