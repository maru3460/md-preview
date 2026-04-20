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
document.addEventListener('DOMContentLoaded', function() {
    addHeadingIds();
    hljs.highlightAll();
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
