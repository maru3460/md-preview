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

  // 直前に描画した対象。消すときに同じ場所を辿るため覚えておく。
  // 要素は { root, doc, win, mode, app }。mode は 'mark'（DOM に mark を挿す）か
  // 'highlight'（CSS Custom Highlight API で塗る）で、親＝mark / iframe＝highlight に固定。
  // app は「md-preview 自身が吐いた DOM か」で、除外ルールの適用範囲を決める。
  var targets = [];
  // テーマの配色は開くたびに読み直す（ライト/ダーク切替に追従させるため）。
  var themeColors = null;
  var searchTimer = null;

  // ::highlight() の名前。レジストリは document ごとなので固定名でよい。
  // CSS クラス名と同じ綴りにしてあるので、配色を追うときは grep 一発で両方に当たる。
  var HL_ALL = 'md-search-hit';
  var HL_CUR = 'md-search-hit-current';

  // ヒット数の上限。iframe には巨大な生成 html（カバレッジレポート等）が来うるので、
  // 上限が無いと 1 文字打つだけで数十万の live Range を作り、UI スレッドごと固まる
  // （スレッドは親と iframe で共通なので Esc も効かなくなる）。超えたら打ち切って
  // カウンタに + を付ける。
  var MAX_MATCHES = 5000;
  var truncated = false;
  // 入力のたびに全文書を走査するので、連続入力は最後の 1 回にまとめる。
  var DEBOUNCE_MS = 80;

  function buildBar() {
    bar = document.createElement('div');
    bar.className = 'md-search-bar hidden';
    bar.id = 'md-search-bar'; // MdCommon.isOverlayOpen が O(1) で開閉を見るため
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

    input.addEventListener('input', function() { scheduleSearch(); });
    input.addEventListener('keydown', function(e) {
      if (e.key === 'Enter') {
        e.preventDefault();
        // 打ち終わりに Enter を押されたら、待機中の検索を先に済ませてから移動する。
        if (searchTimer) performSearch();
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

  function scheduleSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(function() {
      searchTimer = null;
      performSearch();
    }, DEBOUNCE_MS);
  }

  // 選択テキストを初期値にする。html 表示では選択が iframe 側にあるので、親に無ければ
  // フレームの選択も見る。ただし WebKit はフォーカスが外れても選択を保持するので、
  // 「いま操作しているフレーム」に限らないと、ずっと前の選択を拾ってしまう。
  function selectedText() {
    var sel = window.getSelection();
    var text = sel ? sel.toString() : '';
    if (text) return text;
    frames().forEach(function(frame) {
      if (text) return;
      try {
        var doc = frame.contentDocument;
        if (!doc || !doc.hasFocus()) return;
        var s = frame.contentWindow.getSelection();
        if (s) text = s.toString();
      } catch (e) { /* cross-origin: 触れないので親の選択だけで諦める */ }
    });
    return text;
  }

  function open() {
    if (!bar) return;
    bar.classList.remove('hidden');
    themeColors = null; // テーマが変わっている可能性があるので読み直す
    var selText = selectedText();
    if (selText && selText.length < 200) {
      input.value = selText;
    }
    input.focus();
    input.select();
    performSearch();
  }

  function close() {
    if (!bar) return;
    if (searchTimer) { clearTimeout(searchTimer); searchTimer = null; }
    bar.classList.add('hidden');
    releaseSheets();
    clearHighlights();
    matches = [];
    currentIndex = -1;
  }

  // ── 検索対象の解決 ──────────────────────────────────────────

  function frames() {
    if (!container || !container.querySelectorAll) return [];
    return Array.prototype.slice.call(container.querySelectorAll('iframe.html-frame'));
  }

  // iframe 内リンクの遷移やホットリロードで文書が差し替わるとハイライトは消える。
  // バーが開いたままなら、新しい文書に対して検索をやり直す。
  function wireFrameReload(frame) {
    if (frame.__mdSearchWired) return;
    frame.__mdSearchWired = true;
    frame.addEventListener('load', function() {
      // targets を空にするだけでは、同じ配列に入っている「親」の後始末情報まで捨てて
      // しまい、親に挿し込み済みの mark が孤児になる（以後その箇所は走査から除外され、
      // 色だけ残って件数が減る）。差し替わった iframe 側への delete は空振りするだけ
      // なので、まとめて clearHighlights に通す。
      clearHighlights();
      if (bar && !bar.classList.contains('hidden') && input.value) performSearch();
    });
  }

  // CSS Custom Highlight API が使えるか。WebKit は Safari 17.2 以降で、WKWebView は
  // OS の Safari に追従するため、古い macOS では false になりうる。
  // false のフレームは検索対象から外す（mark を挿す方式には落とさない。他人の文書の
  // テキストノードを分割することになり、docs/html-iframe-search.md で却下した案A
  // そのものになるため。塗れないなら触らない、で揃える）。
  function supportsHighlight(win) {
    try {
      return !!(win.CSS && win.CSS.highlights && typeof win.Highlight === 'function');
    } catch (e) {
      return false; // cross-origin
    }
  }

  // 走査ルートの一覧。親の container と、その中の同一オリジン iframe の body。
  function resolveTargets() {
    var out = [];
    if (!container) return out;
    out.push({ root: container, doc: document, win: window, mode: 'mark', app: true });
    frames().forEach(function(frame) {
      var doc, win;
      try {
        doc = frame.contentDocument;
        win = frame.contentWindow;
      } catch (e) { return; } // 外部サイトへ遷移した iframe は cross-origin
      if (!doc || !doc.body || !win) return;
      wireFrameReload(frame);
      if (!supportsHighlight(win)) return;
      out.push({ root: doc.body, doc: doc, win: win, mode: 'highlight', app: false });
    });
    return out;
  }

  // ── ハイライトの配色 ────────────────────────────────────────

  // テーマ CSS は親にしか無いので、iframe へ持ち込む色を親から測って取る。
  // プローブは container の中に挿す。テーマは color: inherit を使うものがあり、
  // body 直下で測ると本文と違う色を拾うことがあるため。
  function readThemeColors() {
    if (themeColors) return themeColors;
    var host = container && container.appendChild ? container : document.body;
    var probe = function(cls) {
      var m = document.createElement('mark');
      m.className = cls;
      m.textContent = 'x';
      m.style.position = 'absolute';
      m.style.left = '-9999px';
      m.style.top = '0';
      host.appendChild(m);
      var cs = getComputedStyle(m);
      var v = { bg: cs.backgroundColor, fg: cs.color };
      host.removeChild(m);
      return v;
    };
    themeColors = { hit: probe('md-search-hit'), cur: probe('md-search-hit md-search-hit-current') };
    return themeColors;
  }

  // ::highlight() に渡せるのは色系のプロパティだけで、outline は指定できない。
  // カレントヒットを outline で表すテーマ（paper / ink など）だと背景色の差だけになって
  // 見分けにくいので、下線で補う。
  //
  // ここは文字列連結で CSS を組んでいるが、値は必ず getComputedStyle が返す色
  // （rgb() / rgba() に正規化済み）なので `;` や `}` は混ざらない。色以外の値（font-family
  // のような任意文字列を取れるもの）を同じ経路で持ち込むと、その時点で iframe への
  // CSS 注入になる。増やすときは注意すること。
  function highlightCss(c) {
    return '::highlight(' + HL_ALL + '){background-color:' + c.hit.bg + ';color:' + c.hit.fg + ';}' +
           '::highlight(' + HL_CUR + '){background-color:' + c.cur.bg + ';color:' + c.cur.fg +
           ';text-decoration:underline;}';
  }

  function sheetAdopted(doc, sheet) {
    return Array.prototype.indexOf.call(doc.adoptedStyleSheets || [], sheet) >= 0;
  }

  // ::highlight() の規則を iframe の文書へ入れる。構築済みスタイルシート
  // （adoptedStyleSheets）は CSSOM 経由なので、style-src の厳しいページでも通る。
  // 使えない環境では <style> 要素に落とす。
  //
  // 「差した覚えがあるか」ではなく「いま実際に adopt されているか」を見ること。
  // adoptedStyleSheets は FrozenArray なので、ページ側は代入で丸ごと差し替える
  // （Lit 等がそう書く）。そのとき黙って外れるので、覚えているだけだと
  // 「件数は出るのに色が一切付かない」という気付けない壊れ方をする。
  function ensureSheet(t) {
    var css = highlightCss(readThemeColors());
    try {
      var kept = t.doc.__mdSearchSheet;
      if (kept && sheetAdopted(t.doc, kept)) {
        kept.replaceSync(css);
        return;
      }
      var sheet = kept || new t.win.CSSStyleSheet();
      sheet.replaceSync(css);
      t.doc.adoptedStyleSheets = Array.prototype.slice.call(t.doc.adoptedStyleSheets || []).concat([sheet]);
      t.doc.__mdSearchSheet = sheet;
      return;
    } catch (e) { /* 構築済みスタイルシートが使えない環境 */ }
    try {
      var el = t.doc.__mdSearchStyleEl;
      if (!el || !el.isConnected) {
        el = t.doc.createElement('style');
        (t.doc.head || t.doc.documentElement).appendChild(el);
        t.doc.__mdSearchStyleEl = el;
      }
      el.textContent = css;
    } catch (e2) { /* 色が付かないだけで、移動と件数は動く */ }
  }

  // 閉じるときは注入した規則も片付ける。検索していない間まで他人の文書に
  // スタイルシートを残さないため（1 キーごとに出し入れすると無駄なので close 限定）。
  // targets ではなくフレームから引く。クエリが空のまま閉じた場合など、targets が
  // 空でも前回の注入が残っていることがあるため。
  function releaseSheets() {
    frames().forEach(function(frame) {
      try {
        var doc = frame.contentDocument;
        if (!doc) return;
        if (doc.__mdSearchSheet) {
          var sheet = doc.__mdSearchSheet;
          doc.adoptedStyleSheets = Array.prototype.slice.call(doc.adoptedStyleSheets || [])
            .filter(function(s) { return s !== sheet; });
          doc.__mdSearchSheet = null;
        }
        if (doc.__mdSearchStyleEl) {
          var el = doc.__mdSearchStyleEl;
          if (el.parentNode) el.parentNode.removeChild(el);
          doc.__mdSearchStyleEl = null;
        }
      } catch (e) { /* cross-origin / 差し替え後。残っていても実害は無い */ }
    });
  }

  function setHighlight(t, name, ranges, priority) {
    try {
      var h = new t.win.Highlight();
      ranges.forEach(function(r) { h.add(r); });
      h.priority = priority;
      t.win.CSS.highlights.set(name, h);
    } catch (e) {
      // ここで落ちると matches には積まれているので「12/47 と出るのに色が付かない」
      // 状態になる。原因は文書の差し替え中くらいで、次の入力でやり直せば直る。
    }
  }

  function clearHighlights() {
    targets.forEach(function(t) {
      try {
        if (t.mode === 'highlight') {
          t.win.CSS.highlights.delete(HL_ALL);
          t.win.CSS.highlights.delete(HL_CUR);
        } else {
          unwrapMarks(t.root);
        }
      } catch (e) { /* 文書が差し替わった後などは何もしない */ }
    });
    targets = [];
  }

  // ── mark 方式（親の本文のみ） ───────────────────────────────

  function unwrapMarks(root) {
    var marks = root.querySelectorAll('mark.md-search-hit');
    var parents = new Set();
    Array.prototype.forEach.call(marks, function(m) {
      var parent = m.parentNode;
      if (!parent) return;
      while (m.firstChild) parent.insertBefore(m.firstChild, m);
      parent.removeChild(m);
      parents.add(parent);
    });
    parents.forEach(function(p) { p.normalize(); });
  }

  // ── 走査 ────────────────────────────────────────────────────

  function performSearch() {
    if (searchTimer) { clearTimeout(searchTimer); searchTimer = null; }
    clearHighlights();
    matches = [];
    currentIndex = -1;
    truncated = false;
    var query = input.value;
    if (!query) {
      updateCounter();
      return;
    }
    targets = resolveTargets();
    targets.forEach(function(t) { renderTarget(t, query); });
    if (matches.length > 0) {
      jumpTo(0);
    } else {
      updateCounter();
    }
  }

  // 1 つのルートを走査して、見つけた位置をその文書に合った方法で描画する。
  function renderTarget(t, query) {
    var budget = MAX_MATCHES - matches.length;
    if (budget <= 0) { truncated = true; return; }
    var hits = collectHits(t, query, budget);
    if (hits.length === 0) return;
    if (t.mode === 'highlight') {
      ensureSheet(t);
      var ranges = [];
      hits.forEach(function(h) {
        try {
          var r = t.doc.createRange();
          r.setStart(h.node, h.start);
          r.setEnd(h.node, h.end);
          ranges.push(r);
          matches.push({ type: 'range', range: r, t: t });
        } catch (e) {
          // 走査後に文書が変わってオフセットが範囲外になった場合。そのヒットは
          // 諦める（件数から落ちる）。次の入力でやり直せば整合する。
        }
      });
      setHighlight(t, HL_ALL, ranges, 0);
    } else {
      wrapHits(t, hits);
    }
  }

  // toLowerCase は文字数が変わることがある（"İ" U+0130 → "i̇" の 2 文字）。位置を元テキスト
  // のオフセットとして Range や slice に渡すので、長さが変わる文字は畳まずそのまま残して
  // 桁を合わせる。畳まなかった文字は当たらなくなるが、桁がずれて別の場所を塗るよりよい。
  function foldCase(s) {
    var lower = s.toLowerCase();
    if (lower.length === s.length) return lower;
    var out = '';
    for (var i = 0; i < s.length; i++) {
      var c = s.charAt(i);
      var l = c.toLowerCase();
      out += l.length === c.length ? l : c;
    }
    return out;
  }

  // その要素の中身を検索対象から外すか。app（md-preview 自身が吐いた DOM）でだけ効く
  // ルールと、どの文書でも効くルールを分ける。前者を他人の html に当てると、画面に
  // 見えている <button> の文字や SVG 図中のラベルが「検索しても出てこない」ことになる。
  function rejectsSubtree(el, t) {
    var tag = el.nodeName;
    // 描画されないので、どの文書でも対象外。
    if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT') return true;
    // 見えていないテキストを数えると、Enter でカウンタだけ進んで画面が動かなくなる。
    if (el.hidden) return true;
    var cs = t.win.getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden') return true;
    if (!t.app) return false;
    // ここから下は md-preview 自身の DOM 向け。
    // svg: Mermaid / draw.io の描画結果。mark を挿すと図が壊れる。
    if (tag === 'svg' || tag === 'SVG') return true;
    // コードブロックのコピーボタン等、本文ではない UI。
    if (tag === 'BUTTON') return true;
    if (el.classList) {
      if (el.classList.contains('md-search-bar')) return true;
      if (el.classList.contains('copy-btn')) return true;
      // diff の行番号・+/- 記号は装飾なので検索対象外（ヒット/件数を汚さない）。
      // ソースビューの行番号ガター(.source-gutter)は aria-hidden="true" を持つので
      // 下の aria-hidden チェックで弾かれる（ここで重ねてチェックしない）。
      if (el.classList.contains('diff-gutter')) return true;
      if (el.classList.contains('diff-sign')) return true;
      if (tag === 'MARK' && el.classList.contains('md-search-hit')) return true;
    }
    // aria-hidden の装飾要素（行番号など）も一律で除外する。
    if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return true;
    return false;
  }

  // テキストノードごとに一致位置を集める。DOM はまだ触らない。
  function collectHits(t, query, budget) {
    var needle = caseInsensitive ? foldCase(query) : query;
    var nLen = needle.length;
    var hits = [];
    if (nLen === 0) return hits;
    var root = t.root;

    // 祖先の判定結果を要素ごとに覚える。TreeWalker はテキストノードを REJECT しても
    // 枝刈りされない（SKIP と等価）ので、これが無いと O(ノード数 × 深さ) で
    // getComputedStyle を呼び続けることになる。
    var memo = new Map();
    function excluded(node) {
      var chain = [];
      var verdict = false;
      var p = node.parentNode;
      while (p && p !== root) {
        if (p.nodeType === 1) {
          var m = memo.get(p);
          if (m !== undefined) { verdict = m; break; }
          chain.push(p);
          if (rejectsSubtree(p, t)) { verdict = true; break; }
        }
        p = p.parentNode;
      }
      // 祖先が除外なら子孫も除外、通るなら子孫も通る。どちらも下へ伝わる。
      chain.forEach(function(el) { memo.set(el, verdict); });
      return verdict;
    }

    var walker = t.doc.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: function(node) {
        if (!node.nodeValue || node.nodeValue.length === 0) return NodeFilter.FILTER_REJECT;
        return excluded(node) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT;
      }
    });

    var n;
    while ((n = walker.nextNode())) {
      var text = n.nodeValue;
      var hay = caseInsensitive ? foldCase(text) : text;
      var idx = 0;
      while (true) {
        var found = hay.indexOf(needle, idx);
        if (found < 0) break;
        hits.push({ node: n, start: found, end: found + nLen });
        if (hits.length >= budget) { truncated = true; return hits; }
        idx = found + nLen;
      }
    }
    return hits;
  }

  // mark 方式の描画。1 ノードぶんの一致をまとめて差し替える。
  function wrapHits(t, hits) {
    var byNode = new Map();
    hits.forEach(function(h) {
      var list = byNode.get(h.node);
      if (!list) { list = []; byNode.set(h.node, list); }
      list.push(h);
    });
    byNode.forEach(function(list, node) {
      var text = node.nodeValue;
      var frag = t.doc.createDocumentFragment();
      var cursor = 0;
      var created = [];
      list.forEach(function(h) {
        if (h.start > cursor) {
          frag.appendChild(t.doc.createTextNode(text.slice(cursor, h.start)));
        }
        var mark = t.doc.createElement('mark');
        mark.className = 'md-search-hit';
        mark.textContent = text.slice(h.start, h.end);
        frag.appendChild(mark);
        created.push(mark);
        cursor = h.end;
      });
      if (cursor < text.length) {
        frag.appendChild(t.doc.createTextNode(text.slice(cursor)));
      }
      if (!node.parentNode) return;
      node.parentNode.replaceChild(frag, node);
      created.forEach(function(mark) { matches.push({ type: 'mark', el: mark, t: t }); });
    });
  }

  // ── 現在位置 ────────────────────────────────────────────────

  function setCurrent(entry, on) {
    try {
      if (entry.type === 'mark') {
        entry.el.classList.toggle('md-search-hit-current', on);
      } else if (on) {
        setHighlight(entry.t, HL_CUR, [entry.range], 1);
      } else {
        entry.t.win.CSS.highlights.delete(HL_CUR);
      }
    } catch (e) {
      // 文書が差し替わった後にカレント表示を付け外ししようとした場合。
      // 次の performSearch で作り直されるので、ここは黙って諦めてよい。
    }
  }

  function isScrollable(el, win) {
    if (el.scrollHeight <= el.clientHeight + 1) return false;
    return /(auto|scroll|overlay)/.test(win.getComputedStyle(el).overflowY);
  }

  // Range には scrollIntoView が無いので、矩形を測って自分でスクロールする。
  // ページ内に独自のスクロール領域がある（ヘッダ固定のダッシュボード等）ことがあるので、
  // 内側のスクロール領域から順に外へ辿って動かす。内側だけ動かすと、その領域自体が
  // 画面外にあるとき一致が見えないままになる。
  //
  // body も候補に含める。`html{overflow:hidden} body{overflow-y:auto}` 構成のページでは
  // body がスクロールボックスであり、標準モードの scrollingElement は <html> を返すので、
  // body を飛ばすと「塗られてカウンタも進むのに画面が動かない」ことになる。
  function scrollRangeIntoView(entry) {
    var doc = entry.t.doc;
    var win = entry.t.win;
    var rect;
    try { rect = entry.range.getBoundingClientRect(); } catch (e) { return; }
    // 隠れているテキストは矩形を持たない。走査側で弾いているが、走査後に隠された
    // 場合もあるので、動かしようが無いものはここでも諦める。
    if (!rect || (rect.width === 0 && rect.height === 0)) return;

    // 一致位置の viewport 座標。内側を動かすたび、その移動量だけ引いて持ち回る
    // （スクロール完了を待たずに外側の必要量を出せる）。
    var top = rect.top;
    var el = entry.range.startContainer;
    if (el.nodeType !== 1) el = el.parentElement;
    while (el && el !== doc.documentElement) {
      if (isScrollable(el, win)) {
        var box = el.getBoundingClientRect();
        var from = el.scrollTop;
        var want = from + (top - box.top) - (el.clientHeight - rect.height) / 2;
        // 端で止まる分を引いておかないと、外側の計算がずれる。
        want = Math.min(Math.max(want, 0), el.scrollHeight - el.clientHeight);
        el.scrollTo({ top: want, behavior: 'smooth' });
        top -= want - from;
      }
      el = el.parentElement;
    }
    var root = doc.scrollingElement || doc.documentElement;
    root.scrollTo({ top: root.scrollTop + top - (win.innerHeight - rect.height) / 2, behavior: 'smooth' });
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
      setCurrent(matches[currentIndex], false);
    }
    currentIndex = index;
    var cur = matches[currentIndex];
    setCurrent(cur, true);
    if (cur.type === 'mark') cur.el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    else scrollRangeIntoView(cur);
    updateCounter();
  }

  function updateCounter() {
    if (!counter) return;
    if (matches.length === 0) {
      counter.textContent = input.value ? '0/0' : '';
    } else {
      // 打ち切ったときは「まだ先がある」ことを + で示す。
      counter.textContent = (currentIndex + 1) + '/' + matches.length + (truncated ? '+' : '');
    }
  }

  function attachShortcut() {
    document.addEventListener('keydown', function(e) {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'f' || e.key === 'F')) {
        if (!container) return;
        e.preventDefault();
        open();
        return;
      }
      // Esc は入力欄にフォーカスがある間しか届かない。html 表示中に iframe をクリックすると
      // フォーカスが移り、そこからの Esc は common.js が親へ転送してくるが、受け手が
      // 入力欄側にしか無いと誰も閉じられない。しかも検索バーが開いている間は
      // isOverlayOpen が true でスクロール素キーも止まるので、× をマウスで押すまで
      // キーボード操作が全部死ぬ。ここで document 側の逃げ道を用意する。
      if (e.key === 'Escape' && bar && !bar.classList.contains('hidden')) {
        // 他のオーバーレイ（ヘルプ・⌘P・右クリメニュー・コメント）が開いているときは
        // そちらの Esc なので譲る。一覧は MdCommon 側に一本化してある。
        if (window.MdCommon && MdCommon.isOverlayOpen && MdCommon.isOverlayOpen('md-search-bar')) return;
        e.preventDefault();
        close();
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
    open: function() {
      // 素キー '/' 等から呼ばれる。init 前（container 無し）は何もしない。
      if (container) open();
    },
    reset: function() {
      close();
    },
    close: close
  };
})();
