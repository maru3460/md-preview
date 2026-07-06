// init.js / folder.js / toc.js から共有するヘルパ群。<head> に inline され、
// window.MdCommon として公開する。各関数は scope（省略時は document）を受け取り、
// 単一ファイル表示（init: document 全体）とフォルダ表示（folder: #preview-pane）の
// 両方を 1 つの実装で賄う。
(function() {
  // 遅延ロードした <script> を URL 単位でメモ化する。mermaid/drawio の巨大ライブラリを
  // 必要になった時に 1 度だけ読み込むためのもの。
  var cache = {};
  function loadLib(url) {
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

  // 見出しテキストをアンカー用スラグへ変換する。記号を落とし空白をハイフン化。
  function slugify(text) {
    return (text || '')
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\s-]/gu, '')
      .trim()
      .replace(/\s+/g, '-');
  }

  // 既存 id があればそれを返し、無ければスラグから一意な id を生成して付与する。
  // 同名見出しは -2 / -3 ... で一意化し、空スラグは 'h' にフォールバックする。
  function ensureHeadingId(h) {
    if (h.id) return h.id;
    var base = slugify(h.textContent);
    if (!base) base = 'h';
    var id = base;
    var n = 2;
    while (document.getElementById(id)) {
      id = base + '-' + (n++);
    }
    h.id = id;
    return id;
  }

  function ensureHeadingIds(scope) {
    var root = scope || document;
    root.querySelectorAll('h1,h2,h3,h4,h5,h6').forEach(function(h) {
      ensureHeadingId(h);
    });
  }

  // 各コードブロックを .code-wrapper で包み Copy ボタンを差し込む。多重付与は防ぐ。
  function addCopyButtons(scope) {
    var root = scope || document;
    root.querySelectorAll('pre > code').forEach(function(code) {
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
        navigator.clipboard.writeText(code.innerText).then(function() {
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

  // scope 内に mermaid 図があれば lib を遅延ロードして描画する。
  function runMermaid(scope) {
    var root = scope || document;
    var nodes = root.querySelectorAll('pre.mermaid');
    if (!nodes.length) return;
    loadLib('mdpreview://localhost/__lib/mermaid.min.js').then(function() {
      if (typeof mermaid === 'undefined') return;
      if (!mermaid.__mdInit) {
        var ap = window.MD_APPEARANCE || 'auto';
        var dark = ap === 'dark' || (ap === 'auto' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
        mermaid.initialize({ startOnLoad: false, theme: dark ? 'dark' : 'neutral', securityLevel: 'loose' });
        mermaid.__mdInit = true;
      }
      try { mermaid.run({ nodes: nodes }); } catch (e) {}
    }).catch(function() {});
  }

  // scope 内に draw.io 図があれば lib を遅延ロードして描画する。
  // GraphViewer.processElements() はページ全体を対象にするため scope は存在チェックのみに使う。
  function runDrawio(scope) {
    var root = scope || document;
    var nodes = root.querySelectorAll('.mxgraph');
    if (!nodes.length) return;
    loadLib('mdpreview://localhost/__lib/drawio-viewer.min.js').then(function() {
      if (typeof GraphViewer === 'undefined') return;
      try { GraphViewer.processElements(); } catch (e) {}
    }).catch(function() {});
  }

  // Cmd/Ctrl+A で本文（.markdown-body）だけを全選択する。ページ全体（サイドバーや
  // フロントマター外の UI）を巻き込まないようにするためのもの。
  // 入力欄や編集可能要素にフォーカスがある時は通常の全選択に任せる。
  function selectBody(e) {
    if (!(e.metaKey || e.ctrlKey) || e.key !== 'a') return;
    var t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    var body = document.querySelector('.markdown-body');
    if (!body) return;
    e.preventDefault();
    var sel = window.getSelection();
    if (!sel) return;
    var range = document.createRange();
    range.selectNodeContents(body);
    sel.removeAllRanges();
    sel.addRange(range);
  }
  document.addEventListener('keydown', selectBody);

  window.MdCommon = {
    loadLib: loadLib,
    slugify: slugify,
    ensureHeadingId: ensureHeadingId,
    ensureHeadingIds: ensureHeadingIds,
    addCopyButtons: addCopyButtons,
    runMermaid: runMermaid,
    runDrawio: runDrawio
  };
})();
