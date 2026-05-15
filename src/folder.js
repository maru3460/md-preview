(function() {
  var expandedDirs = new Set();
  var currentFilePath = null;
  var windowReady = false;
  var mdCheckQueue = [];
  var mdDotCache = {};

  function doHasMdCheck(path, row) {
    if (path in mdDotCache) {
      if (mdDotCache[path]) {
        row.classList.add('has-md');
      }
      return;
    }
    fetch('/?has_md=' + encodeURIComponent(path))
      .then(function(r) { return r.json(); })
      .then(function(data) {
        mdDotCache[path] = !!data.has_md;
        if (data.has_md) {
          row.classList.add('has-md');
        }
      })
      .catch(function() {});
  }

  function scheduleHasMdCheck(path, row) {
    if (windowReady) {
      doHasMdCheck(path, row);
    } else {
      mdCheckQueue.push({path: path, row: row});
    }
  }

  function addHeadingIds() {
    document.querySelectorAll('#preview-pane h1,#preview-pane h2,#preview-pane h3,#preview-pane h4,#preview-pane h5,#preview-pane h6').forEach(function(h) {
      if (!h.id) {
        h.id = h.textContent
          .toLowerCase()
          .replace(/[^\p{L}\p{N}\s-]/gu, '')
          .trim()
          .replace(/\s+/g, '-');
      }
    });
  }

  function renderItems(items, parentEl, depth) {
    items.forEach(function(item) {
      var row = document.createElement('div');
      row.className = 'tree-item';
      row.style.paddingLeft = (8 + depth * 16) + 'px';

      var icon = document.createElement('span');
      icon.className = 'icon';

      if (item.kind === 'dir') {
        icon.textContent = '›';
        var children = document.createElement('div');
        children.className = 'tree-children';
        var loaded = false;

        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);
        parentEl.appendChild(children);

        scheduleHasMdCheck(item.path, row);

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          if (children.classList.contains('open')) {
            children.classList.remove('open');
            row.classList.remove('dir-open');
            expandedDirs.delete(item.path);
          } else {
            children.classList.add('open');
            row.classList.add('dir-open');
            expandedDirs.add(item.path);
            if (!loaded) {
              loaded = true;
              fetch('/?dir=' + encodeURIComponent(item.path))
                .then(function(r) { return r.json(); })
                .then(function(subItems) { renderItems(subItems, children, depth + 1); })
                .catch(function() {});
            }
          }
        });
      } else {
        if (item.name.endsWith('.md')) {
          row.classList.add('md-file');
        }
        icon.textContent = '';
        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          document.querySelectorAll('.tree-item.active').forEach(function(el) {
            el.classList.remove('active');
          });
          row.classList.add('active');
          loadPreview(item.path);
        });
      }
    });
  }

  function resolveRelativePath(base, rel) {
    var parts = base.split('/');
    parts.pop();
    rel.split('/').forEach(function(seg) {
      if (seg === '..') { if (parts.length > 0) parts.pop(); }
      else if (seg !== '.') { parts.push(seg); }
    });
    return parts.join('/');
  }

  function addCopyButtons(scope) {
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

  function runMermaidIn(scope) {
    if (typeof mermaid === 'undefined') return;
    var nodes = scope.querySelectorAll('pre.mermaid');
    if (!nodes.length) return;
    try { mermaid.run({ nodes: nodes }); } catch (e) {}
  }

  function loadPreview(relPath) {
    if (window.MdSearch) window.MdSearch.reset();
    fetch('/?file=' + encodeURIComponent(relPath))
      .then(function(r) { return r.text(); })
      .then(function(html) {
        currentFilePath = relPath;
        var pane = document.getElementById('preview-pane');
        pane.innerHTML = html;
        pane.scrollTop = 0;
        addHeadingIds();
        if (window.hljs) hljs.highlightAll();
        addCopyButtons(pane);
        runMermaidIn(pane);
      })
      .catch(function() {});
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
    initMermaid();
    var resizer = document.getElementById('resizer');
    var sidebar = document.getElementById('sidebar');
    var isDragging = false;
    var startX, startWidth;
    resizer.addEventListener('mousedown', function(e) {
      isDragging = true;
      startX = e.clientX;
      startWidth = sidebar.offsetWidth;
      resizer.classList.add('dragging');
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      e.preventDefault();
    });
    document.addEventListener('mousemove', function(e) {
      if (!isDragging) return;
      var newWidth = startWidth + (e.clientX - startX);
      newWidth = Math.max(120, Math.min(newWidth, window.innerWidth - 200));
      sidebar.style.width = newWidth + 'px';
    });
    document.addEventListener('mouseup', function() {
      if (!isDragging) return;
      isDragging = false;
      resizer.classList.remove('dragging');
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    });

    if (window.MdSearch) {
      window.MdSearch.init(document.getElementById('preview-pane'));
    }

    fetch('/?dir=')
      .then(function(r) { return r.json(); })
      .then(function(items) {
        renderItems(items, sidebar, 0);
        if (typeof INITIAL_FILE === 'string' && INITIAL_FILE) {
          loadPreview(INITIAL_FILE);
        }
        setTimeout(function() {
          window.ipc.postMessage('ready');
          windowReady = true;
          mdCheckQueue.forEach(function(item) { doHasMdCheck(item.path, item.row); });
          mdCheckQueue = [];
        }, 0);
      })
      .catch(function() {
        setTimeout(function() { window.ipc.postMessage('ready'); windowReady = true; }, 0);
      });
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
    if (!href) return;
    if (href.charAt(0) === '#') {
      e.preventDefault();
      var id = decodeURIComponent(href.slice(1));
      var target = document.getElementById(id);
      if (target) target.scrollIntoView({ behavior: 'smooth' });
    } else if (!href.startsWith('http://') && !href.startsWith('https://') && !href.startsWith('mailto:')) {
      var hashIdx = href.indexOf('#');
      var pathPart = hashIdx !== -1 ? href.slice(0, hashIdx) : href;
      var anchorPart = hashIdx !== -1 ? href.slice(hashIdx + 1) : '';
      if (pathPart.endsWith('.md')) {
        e.preventDefault();
        var resolved = currentFilePath ? resolveRelativePath(currentFilePath, pathPart) : pathPart;
        loadPreview(resolved);
        if (anchorPart) {
          setTimeout(function() {
            var id = decodeURIComponent(anchorPart);
            var target = document.getElementById(id);
            if (target) target.scrollIntoView({ behavior: 'smooth' });
          }, 100);
        }
      }
    }
  });
})();
