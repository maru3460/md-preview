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

  function loadPreview(relPath, preserveScroll) {
    var pane = document.getElementById('preview-pane');
    var savedScroll = preserveScroll ? pane.scrollTop : 0;
    fetch('/?file=' + encodeURIComponent(relPath), preserveScroll ? { cache: 'no-store' } : undefined)
      .then(function(r) { return r.ok ? r.text() : null; })
      .then(function(html) {
        if (html == null) return;
        if (window.MdSearch) window.MdSearch.reset();
        currentFilePath = relPath;
        pane.innerHTML = html;
        pane.scrollTop = savedScroll;
        MdCommon.ensureHeadingIds(pane);
        if (window.hljs) hljs.highlightAll();
        MdCommon.addCopyButtons(pane);
        MdCommon.runMermaid(pane);
        MdCommon.runDrawio(pane);
        if (window.MdToc) window.MdToc.refresh();
      })
      .catch(function() {});
  }

  window.MdReload = function(relPath) {
    if (!currentFilePath) return;
    if (relPath && relPath !== currentFilePath) return;
    loadPreview(currentFilePath, true);
  };

  document.addEventListener('DOMContentLoaded', function() {
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
    if (window.MdToc) {
      window.MdToc.init(document.getElementById('preview-pane'));
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
