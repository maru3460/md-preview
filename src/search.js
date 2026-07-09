(function() {
  var container = null;
  var bar = null;
  var input = null;
  var counter = null;
  var caseBtn = null;
  var matches = [];
  var currentIndex = -1;
  var caseInsensitive = true;
  var initialized = false;

  function buildBar() {
    bar = document.createElement('div');
    bar.className = 'md-search-bar hidden';
    bar.innerHTML =
      '<input type="text" class="md-search-input" placeholder="Find" spellcheck="false" autocomplete="off">' +
      '<span class="md-search-counter">0/0</span>' +
      '<button type="button" class="md-search-btn" data-act="prev" title="Previous (Shift+Enter)">↑</button>' +
      '<button type="button" class="md-search-btn" data-act="next" title="Next (Enter)">↓</button>' +
      '<button type="button" class="md-search-btn md-search-case" data-act="case" title="Match case">Aa</button>' +
      '<button type="button" class="md-search-btn" data-act="close" title="Close (Esc)">×</button>';
    document.body.appendChild(bar);
    input = bar.querySelector('.md-search-input');
    counter = bar.querySelector('.md-search-counter');
    caseBtn = bar.querySelector('.md-search-case');

    input.addEventListener('input', function() { performSearch(); });
    input.addEventListener('keydown', function(e) {
      if (e.key === 'Enter') {
        e.preventDefault();
        if (e.shiftKey) jumpTo(currentIndex - 1);
        else jumpTo(currentIndex + 1);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        close();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        jumpTo(currentIndex + 1);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        jumpTo(currentIndex - 1);
      }
    });

    bar.addEventListener('click', function(e) {
      var btn = e.target.closest('button[data-act]');
      if (!btn) return;
      var act = btn.getAttribute('data-act');
      if (act === 'next') jumpTo(currentIndex + 1);
      else if (act === 'prev') jumpTo(currentIndex - 1);
      else if (act === 'close') close();
      else if (act === 'case') {
        caseInsensitive = !caseInsensitive;
        caseBtn.classList.toggle('active', !caseInsensitive);
        performSearch();
        input.focus();
      }
    });
  }

  function open() {
    if (!bar) return;
    bar.classList.remove('hidden');
    var sel = window.getSelection();
    var selText = sel ? sel.toString() : '';
    if (selText && selText.length < 200) {
      input.value = selText;
    }
    input.focus();
    input.select();
    performSearch();
  }

  function close() {
    if (!bar) return;
    bar.classList.add('hidden');
    clearHighlights();
    matches = [];
    currentIndex = -1;
  }

  function clearHighlights() {
    if (!container) return;
    var marks = container.querySelectorAll('mark.md-search-hit');
    var parents = new Set();
    marks.forEach(function(m) {
      var parent = m.parentNode;
      if (!parent) return;
      while (m.firstChild) parent.insertBefore(m.firstChild, m);
      parent.removeChild(m);
      parents.add(parent);
    });
    parents.forEach(function(p) { p.normalize(); });
  }

  function performSearch() {
    clearHighlights();
    matches = [];
    currentIndex = -1;
    var query = input.value;
    if (!query) {
      updateCounter();
      return;
    }
    highlightAll(query);
    if (matches.length > 0) {
      jumpTo(0);
    } else {
      updateCounter();
    }
  }

  function highlightAll(query) {
    if (!container) return;
    var needle = caseInsensitive ? query.toLowerCase() : query;
    var nLen = needle.length;
    if (nLen === 0) return;

    var walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT, {
      acceptNode: function(node) {
        var p = node.parentNode;
        while (p && p !== container) {
          var tag = p.nodeName;
          if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT') return NodeFilter.FILTER_REJECT;
          if (tag === 'svg' || tag === 'SVG') return NodeFilter.FILTER_REJECT;
          if (tag === 'BUTTON') return NodeFilter.FILTER_REJECT;
          if (p.classList) {
            if (p.classList.contains('md-search-bar')) return NodeFilter.FILTER_REJECT;
            if (p.classList.contains('copy-btn')) return NodeFilter.FILTER_REJECT;
            // diff の行番号・+/- 記号は装飾なので検索対象外（ヒット/件数を汚さない）。
            // ソースビューの行番号ガター(.source-gutter)は aria-hidden="true" を持つので
            // 下の aria-hidden チェックで弾かれる（ここで重ねてチェックしない）。
            if (p.classList.contains('diff-gutter')) return NodeFilter.FILTER_REJECT;
            if (p.classList.contains('diff-sign')) return NodeFilter.FILTER_REJECT;
          }
          // aria-hidden の装飾要素（行番号など）も一律で除外する。
          if (p.getAttribute && p.getAttribute('aria-hidden') === 'true') return NodeFilter.FILTER_REJECT;
          if (tag === 'MARK' && p.classList && p.classList.contains('md-search-hit')) return NodeFilter.FILTER_REJECT;
          p = p.parentNode;
        }
        return node.nodeValue && node.nodeValue.length > 0
          ? NodeFilter.FILTER_ACCEPT
          : NodeFilter.FILTER_REJECT;
      }
    });

    var textNodes = [];
    var n;
    while ((n = walker.nextNode())) textNodes.push(n);

    textNodes.forEach(function(node) {
      var text = node.nodeValue;
      var hay = caseInsensitive ? text.toLowerCase() : text;
      var idx = 0;
      var positions = [];
      while (true) {
        var found = hay.indexOf(needle, idx);
        if (found < 0) break;
        positions.push(found);
        idx = found + nLen;
      }
      if (positions.length === 0) return;

      var frag = document.createDocumentFragment();
      var cursor = 0;
      positions.forEach(function(pos) {
        if (pos > cursor) {
          frag.appendChild(document.createTextNode(text.slice(cursor, pos)));
        }
        var mark = document.createElement('mark');
        mark.className = 'md-search-hit';
        mark.textContent = text.slice(pos, pos + nLen);
        frag.appendChild(mark);
        matches.push(mark);
        cursor = pos + nLen;
      });
      if (cursor < text.length) {
        frag.appendChild(document.createTextNode(text.slice(cursor)));
      }
      node.parentNode.replaceChild(frag, node);
    });
  }

  function jumpTo(index) {
    if (matches.length === 0) {
      currentIndex = -1;
      updateCounter();
      return;
    }
    if (index < 0) index = matches.length - 1;
    if (index >= matches.length) index = 0;
    if (currentIndex >= 0 && matches[currentIndex]) {
      matches[currentIndex].classList.remove('md-search-hit-current');
    }
    currentIndex = index;
    var cur = matches[currentIndex];
    cur.classList.add('md-search-hit-current');
    cur.scrollIntoView({ block: 'center', behavior: 'smooth' });
    updateCounter();
  }

  function updateCounter() {
    if (!counter) return;
    if (matches.length === 0) {
      counter.textContent = input.value ? '0/0' : '';
    } else {
      counter.textContent = (currentIndex + 1) + '/' + matches.length;
    }
  }

  function attachShortcut() {
    document.addEventListener('keydown', function(e) {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'f' || e.key === 'F')) {
        if (!container) return;
        e.preventDefault();
        open();
      }
    });
  }

  window.MdSearch = {
    init: function(containerEl) {
      container = containerEl;
      if (!initialized) {
        buildBar();
        attachShortcut();
        initialized = true;
      }
    },
    reset: function() {
      close();
    },
    close: close
  };
})();
