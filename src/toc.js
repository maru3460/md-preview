(function() {
  var panel = null;
  var listEl = null;
  var emptyEl = null;
  var scroller = null;
  var anchorMap = [];
  var initialized = false;
  var open = false;
  var scrollTarget = null;

  function buildPanel() {
    panel = document.createElement('div');
    panel.className = 'md-toc-panel hidden';
    panel.innerHTML =
      '<div class="md-toc-header">' +
        '<span class="md-toc-title">Outline</span>' +
        '<button type="button" class="md-toc-close" title="Close" aria-label="Close">×</button>' +
      '</div>' +
      '<div class="md-toc-empty" hidden>No headings</div>' +
      '<ul class="md-toc-list"></ul>';
    document.body.appendChild(panel);
    listEl = panel.querySelector('.md-toc-list');
    emptyEl = panel.querySelector('.md-toc-empty');
    panel.querySelector('.md-toc-close').addEventListener('click', closePanel);
  }

  function ensureHeadingId(h) {
    if (h.id) return h.id;
    var base = (h.textContent || '')
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
    return id;
  }

  function build() {
    if (!scroller) return;
    var headings = Array.prototype.slice.call(
      scroller.querySelectorAll('h1,h2,h3,h4,h5,h6')
    );
    listEl.innerHTML = '';
    anchorMap = [];
    if (!headings.length) {
      emptyEl.hidden = false;
      listEl.hidden = true;
      return;
    }
    emptyEl.hidden = true;
    listEl.hidden = false;
    headings.forEach(function(h) {
      var id = ensureHeadingId(h);
      var li = document.createElement('li');
      li.className = 'md-toc-item lvl-' + h.tagName.charAt(1);
      var a = document.createElement('a');
      a.href = '#' + id;
      a.textContent = h.textContent;
      a.addEventListener('click', function(e) {
        e.preventDefault();
        h.scrollIntoView({ behavior: 'smooth', block: 'start' });
      });
      li.appendChild(a);
      listEl.appendChild(li);
      anchorMap.push({ heading: h, link: a });
    });
    updateActive();
  }

  function updateActive() {
    if (!open || !anchorMap.length) return;
    var threshold = 80;
    var active = null;
    for (var i = 0; i < anchorMap.length; i++) {
      var top = anchorMap[i].heading.getBoundingClientRect().top;
      if (top <= threshold) active = anchorMap[i];
      else break;
    }
    if (!active) active = anchorMap[0];
    anchorMap.forEach(function(it) { it.link.classList.remove('active'); });
    active.link.classList.add('active');
    var aRect = active.link.getBoundingClientRect();
    var lRect = listEl.getBoundingClientRect();
    if (aRect.top < lRect.top || aRect.bottom > lRect.bottom) {
      active.link.scrollIntoView({ block: 'nearest' });
    }
  }

  function openPanel() {
    if (!initialized || open) return;
    open = true;
    panel.classList.remove('hidden');
    build();
  }

  function closePanel() {
    if (!open) return;
    open = false;
    panel.classList.add('hidden');
  }

  function toggle() {
    if (open) closePanel(); else openPanel();
  }

  function attachScrollListener() {
    if (!scrollTarget) return;
    var ticking = false;
    var handler = function() {
      if (ticking) return;
      ticking = true;
      requestAnimationFrame(function() {
        ticking = false;
        updateActive();
      });
    };
    scrollTarget.addEventListener('scroll', handler, { passive: true });
  }

  window.MdToc = {
    init: function(scrollerEl) {
      scroller = scrollerEl;
      scrollTarget = (scrollerEl === document.scrollingElement
        || scrollerEl === document.documentElement
        || scrollerEl === document.body)
        ? window
        : scrollerEl;
      if (!initialized) {
        buildPanel();
        initialized = true;
        document.addEventListener('keydown', function(e) {
          if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && (e.key === 't' || e.key === 'T' || e.code === 'KeyT')) {
            e.preventDefault();
            toggle();
          }
        });
        attachScrollListener();
      }
    },
    refresh: function() {
      if (open) build();
    },
    close: closePanel,
    open: openPanel
  };
})();
