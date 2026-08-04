// ファイル検索パレット（⌘P）。フォルダ全体からファイル名であいまい検索して開く。
// help.js と同じく「開いている間だけ DOM に存在する」オーバーレイで、配色は
// システムカラー（Canvas/CanvasText/Highlight）と color-mix で組むので themes/*.css
// への追記は不要。
//
// folder モード専用（folder.js が init する）。単一ファイル / stdin モードには
// 「別のファイルを開く」入口が無いので初期化しない。keymap.js 側も scope:'folder'。
//
// ファイル一覧はサーバの `/?files=1`（root 以下を再帰的に走査。node_modules 等は除外）
// から取る。一度取ったらメモリに持ち、次回以降は即描画してから裏で取り直す
// （開いた瞬間に一覧が出ることを優先する。ファイルの増減は数百 ms 後に反映される）。
//
// あわせて `/?changed=1`（git の変更ファイル）も取り、**変更のあるファイルを優先する**。
// 未入力なら先頭に並べ、検索中も少し加点し、`+N −M` のバッジを出す。「差分のある
// ファイルへ飛ぶ」導線を新しいキーやボタンとして足さず、既にある ⌘P に寄せている
// （⌘D で差分 ON にしたまま ⌘P で飛べば、そのファイルの差分がそのまま出る）。
(function() {
  var overlay = null;
  var input = null;
  var warnEl = null;
  var listEl = null;
  var hintEl = null;
  var opts = null;
  var initialized = false;

  var serverFiles = [];  // /?files=1 の生の一覧（サーバの走査順＝浅い階層が先）
  var files = [];        // 検索対象。serverFiles ＋ 一覧に無い変更ファイル（rebuild で作る）
  var lowerFiles = [];   // files と同じ添字の小文字版（毎キー入力での再生成を避ける）
  var stats = null;      // 変更のあるファイル: path -> { add, del }。無変更・未取得なら null
  var changedPaths = []; // 変更のあるファイル（git の出力順）。未入力時はこの順で先頭に出す
  // サーバ側で探索を打ち切ったか。{ reason: 'files'|'dirs'|'depth', limit: n } または null。
  // 上限の数値はサーバから貰う（ここに書くと片方だけ直した時に文面が嘘になる）。
  var truncation = null;
  var fetched = false;
  var fetching = null;

  var rows = [];         // 表示中の行 [{ path, el }]
  var cursor = 0;

  // 描画する最大件数。数千件を DOM に流すと入力ごとに固まるので、上位だけ出す。
  var MAX_ROWS = 80;

  // 変更のあるファイルへの加点。語境界ヒット 1 つ（+8）より少し強く、連続ヒット
  // （+10/文字）で決まる名前一致の優劣は覆さない程度に留める。「打った名前と違う
  // ファイルが変更されているだけで上に来る」のは検索としては壊れているため。
  var CHANGED_BONUS = 15;

  // ── あいまい検索 ────────────────────────────────────────────────
  // 「クエリがパスの部分列になっているか」で絞り、語境界・連続・先頭からの近さで
  // 点数を付ける。完全な最適解（DP）は候補数×クエリ長で重くなるので採らない。

  // text[i] が語の切れ目か。区切り記号の直後と camelCase の境目を語頭とみなす。
  function isBoundary(text, i) {
    if (i === 0) return true;
    var prev = text.charAt(i - 1);
    if (prev === '/' || prev === '-' || prev === '_' || prev === '.' || prev === ' ') return true;
    var cur = text.charAt(i);
    return prev >= 'a' && prev <= 'z' && cur >= 'A' && cur <= 'Z';
  }

  // q（小文字）が lower の部分列なら { score, pos } を返す。マッチしなければ null。
  // text は元のままの文字列（語境界判定に大文字小文字が必要）。
  function scoreSeq(text, lower, q) {
    var n = lower.length;
    var m = q.length;
    if (m === 0 || m > n) return null;

    // 1) 前方から貪欲にマッチさせる（ここで不一致なら部分列ではない）。
    var pos = new Array(m);
    var from = 0;
    for (var i = 0; i < m; i++) {
      var found = lower.indexOf(q.charAt(i), from);
      if (found < 0) return null;
      pos[i] = found;
      from = found + 1;
    }

    // 2) 右から寄せ直す。貪欲マッチは "request" の 'r' を "src" の r に取ってしまうなど
    //    左に寄りすぎるので、後ろへずらせる範囲で「次の文字と連続」＞「語境界」の順に
    //    より良い位置へ移す。右から回すことで連続の判定が左へ伝播する。
    for (var k = m - 1; k >= 0; k--) {
      var limit = (k + 1 < m) ? pos[k + 1] - 1 : n - 1;
      var pick = pos[k];
      for (var t = pos[k] + 1; t <= limit; t++) {
        if (lower.charAt(t) !== q.charAt(k)) continue;
        if (k + 1 < m && t + 1 === pos[k + 1]) { pick = t; break; }   // 連続が最優先
        if (isBoundary(text, t) && !isBoundary(text, pick)) pick = t; // 次善は語境界
      }
      pos[k] = pick;
    }

    var score = 0;
    for (var j = 0; j < m; j++) {
      score += 1;
      if (isBoundary(text, pos[j])) score += 8;
      if (j > 0) {
        if (pos[j] === pos[j - 1] + 1) score += 10;                       // 連続はご褒美
        else score -= Math.min(6, pos[j] - pos[j - 1] - 1);               // 飛びは減点
      }
    }
    score -= Math.min(10, pos[0]); // 先頭から遠いほど減点
    return { score: score, pos: pos };
  }

  // パス全体とファイル名それぞれで採点し、良い方を採る。
  // 探すときに打つのはたいていファイル名なので、ファイル名で当たった方を優遇する。
  // pos はパス全体を基準にした添字で返す（ハイライトに使う）。
  function matchPath(path, lower, q) {
    var slash = path.lastIndexOf('/');
    var best = null;

    if (slash >= 0) {
      var baseHit = scoreSeq(path.slice(slash + 1), lower.slice(slash + 1), q);
      if (baseHit) {
        var shifted = new Array(baseHit.pos.length);
        for (var i = 0; i < baseHit.pos.length; i++) shifted[i] = baseHit.pos[i] + slash + 1;
        best = { score: baseHit.score + 30, pos: shifted };
      }
      var fullHit = scoreSeq(path, lower, q);
      if (fullHit && (!best || fullHit.score > best.score)) best = fullHit;
    } else {
      // ルート直下はパス全体＝ファイル名なので、そのまま優遇分を足す。
      var only = scoreSeq(path, lower, q);
      if (only) best = { score: only.score + 30, pos: only.pos };
    }

    if (!best) return null;
    best.score -= path.length * 0.05; // 同点なら短いパスを上に
    return best;
  }

  // ── 描画 ─────────────────────────────────────────────────────
  function buildOverlay() {
    overlay = document.createElement('div');
    overlay.className = 'md-pal-backdrop';
    overlay.id = 'md-pal-backdrop'; // MdCommon.isOverlayOpen が O(1) で存在を見るため

    var panel = document.createElement('div');
    panel.className = 'md-pal-panel';
    panel.innerHTML =
      '<input type="text" class="md-pal-input" placeholder="ファイル名で検索" spellcheck="false" autocomplete="off">' +
      '<div class="md-pal-warn" hidden></div>' +
      '<div class="md-pal-list"></div>' +
      '<div class="md-pal-hint"></div>';
    overlay.appendChild(panel);

    input = panel.querySelector('.md-pal-input');
    warnEl = panel.querySelector('.md-pal-warn');
    listEl = panel.querySelector('.md-pal-list');
    hintEl = panel.querySelector('.md-pal-hint');

    input.addEventListener('input', render);
    input.addEventListener('keydown', onInputKey);

    // 背景クリックで閉じる（パネル内クリックは無視）。行クリックで開く。
    overlay.addEventListener('mousedown', function(e) {
      if (e.target === overlay) close();
    });
    listEl.addEventListener('click', function(e) {
      var row = e.target.closest ? e.target.closest('.md-pal-row') : null;
      if (!row) return;
      var idx = rows.findIndex(function(r) { return r.el === row; });
      if (idx >= 0) { cursor = idx; choose(); }
    });

    document.body.appendChild(overlay);
  }

  // text を el に流し込みつつ、hits（text 内の添字・昇順）の文字を span で包む。
  // パスはファイル名由来の untrusted な文字列なので innerHTML は使わない。
  function appendHighlighted(el, text, hits) {
    var cursorPos = 0;
    for (var i = 0; i < hits.length; i++) {
      var h = hits[i];
      if (h < 0 || h >= text.length) continue;
      if (h > cursorPos) el.appendChild(document.createTextNode(text.slice(cursorPos, h)));
      // 連続するヒットは 1 つの span にまとめる。
      var end = h + 1;
      while (i + 1 < hits.length && hits[i + 1] === end) { end++; i++; }
      var mark = document.createElement('span');
      mark.className = 'md-pal-hit';
      mark.textContent = text.slice(h, end);
      el.appendChild(mark);
      cursorPos = end;
    }
    if (cursorPos < text.length) el.appendChild(document.createTextNode(text.slice(cursorPos)));
  }

  function buildRow(path, pos) {
    var slash = path.lastIndexOf('/');
    // 区切りの `/` は表示しない（"base.css  src" と出す）。ヒット位置はパス全体基準の
    // ままなので、ファイル名側は区切りぶんずらし、ディレクトリ側はそのまま使える。
    var dir = slash >= 0 ? path.slice(0, slash) : '';
    var name = slash >= 0 ? path.slice(slash + 1) : path;
    var nameOffset = slash + 1;

    var row = document.createElement('div');
    row.className = 'md-pal-row';

    var nameEl = document.createElement('span');
    nameEl.className = 'md-pal-name';
    var dirEl = document.createElement('span');
    dirEl.className = 'md-pal-dir';

    var nameHits = [];
    var dirHits = [];
    (pos || []).forEach(function(p) {
      if (p >= nameOffset) nameHits.push(p - nameOffset);
      else if (p < dir.length) dirHits.push(p); // p === slash（区切り自体）は表示しないので捨てる
    });
    appendHighlighted(nameEl, name, nameHits);
    appendHighlighted(dirEl, dir, dirHits);

    row.appendChild(nameEl);
    if (dir) row.appendChild(dirEl);
    var stat = statOf(path);
    if (stat) row.appendChild(buildStat(stat));
    return row;
  }

  // 変更行数バッジ（+N −M）。0 の側は出さない。両方 0（バイナリや行数を数えなかった
  // 巨大ファイル）でも「変更あり」であることは示す必要があるので ● を出す
  // ── ここで何も出さないと、上に並んでいる理由が分からない行になる。
  function buildStat(stat) {
    var el = document.createElement('span');
    el.className = 'md-pal-stat';
    if (stat.add) {
      var add = document.createElement('span');
      add.className = 'md-pal-add';
      add.textContent = '+' + stat.add;
      el.appendChild(add);
    }
    if (stat.del) {
      var del = document.createElement('span');
      del.className = 'md-pal-del';
      del.textContent = '−' + stat.del;
      el.appendChild(del);
    }
    if (!stat.add && !stat.del) el.textContent = '●';
    return el;
  }

  function statOf(path) {
    return stats ? stats[path] || null : null;
  }

  function setCursor(idx) {
    if (!rows.length) { cursor = 0; return; }
    if (idx < 0) idx = rows.length - 1;
    if (idx >= rows.length) idx = 0;
    if (rows[cursor]) rows[cursor].el.classList.remove('active');
    cursor = idx;
    rows[cursor].el.classList.add('active');
    rows[cursor].el.scrollIntoView({ block: 'nearest' });
  }

  function render() {
    if (!overlay) return;
    var q = input.value.toLowerCase().replace(/\s+/g, '');
    listEl.innerHTML = '';
    rows = [];
    cursor = 0;

    var hits;
    if (!q) {
      // 未入力: 変更のあるファイルを先に、続けてサーバの走査順（浅い階層が先）。
      // 「⌘P → Enter で、いま触っているファイルへ飛ぶ」を成立させるための並びなのだ。
      hits = [];
      for (var c = 0; c < changedPaths.length && hits.length < MAX_ROWS; c++) {
        hits.push({ path: changedPaths[c], pos: null });
      }
      for (var f = 0; f < files.length && hits.length < MAX_ROWS; f++) {
        if (statOf(files[f])) continue; // 変更ありは上で出したので飛ばす
        hits.push({ path: files[f], pos: null });
      }
    } else {
      var scored = [];
      for (var i = 0; i < files.length; i++) {
        var m = matchPath(files[i], lowerFiles[i], q);
        if (!m) continue;
        if (statOf(files[i])) m.score += CHANGED_BONUS;
        scored.push({ path: files[i], pos: m.pos, score: m.score });
      }
      scored.sort(function(a, b) { return b.score - a.score; });
      hits = scored.slice(0, MAX_ROWS);
    }

    var frag = document.createDocumentFragment();
    hits.forEach(function(h) {
      var el = buildRow(h.path, h.pos);
      frag.appendChild(el);
      rows.push({ path: h.path, el: el });
    });
    listEl.appendChild(frag);
    updateWarning();
    if (rows.length) {
      rows[0].el.classList.add('active');
    } else {
      var empty = document.createElement('div');
      empty.className = 'md-pal-empty';
      empty.textContent = fetched ? '一致するファイルがありません' : '読み込み中…';
      listEl.appendChild(empty);
    }
    updateHint(q, hits.length);
  }

  function updateHint(q, shown) {
    var parts = [];
    if (fetched) {
      if (!q) {
        // 変更が何件あるか（＝上から何行が変更ファイルか）を出す。0 件のときは触れない。
        if (changedPaths.length) parts.push('変更 ' + changedPaths.length + ' 件');
        parts.push(files.length + ' ファイル');
      } else {
        parts.push(shown >= MAX_ROWS ? '上位 ' + MAX_ROWS + ' 件' : shown + ' 件');
      }
    }
    parts.push('↑↓ 移動 · Enter 開く · Esc 閉じる');
    hintEl.textContent = parts.join(' · ');
  }

  // 全部を探索できなかったことを明示する。数値はサーバが返した実際の上限を使う。
  // 「一覧に無いファイルがある」ことを黙って隠さないための表示なので、打ち切った時だけ出す。
  function truncationMessage(t) {
    var n = t.limit.toLocaleString();
    if (t.reason === 'files') {
      return 'ファイルが多いため、階層の浅いものから順に ' + n
        + ' 件で探索を打ち切りました。深い階層のファイルは一覧にありません。';
    }
    if (t.reason === 'dirs') {
      return 'ディレクトリが多いため、階層の浅いものから順に ' + n
        + ' ディレクトリで探索を打ち切りました。深い階層のファイルは一覧にありません。';
    }
    if (t.reason === 'depth') {
      return n + ' 階層より深いディレクトリは探索していません。';
    }
    return '探索を途中で打ち切りました。一覧に無いファイルがあります。';
  }

  function updateWarning() {
    if (!warnEl) return;
    if (!truncation) {
      warnEl.hidden = true;
      warnEl.textContent = '';
      return;
    }
    warnEl.hidden = false;
    warnEl.textContent = '⚠ ' + truncationMessage(truncation);
  }

  // ── 操作 ─────────────────────────────────────────────────────
  function onInputKey(e) {
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        close();
        return;
      case 'Enter':
        e.preventDefault();
        choose();
        return;
      case 'ArrowDown':
        e.preventDefault();
        setCursor(cursor + 1);
        return;
      case 'ArrowUp':
        e.preventDefault();
        setCursor(cursor - 1);
        return;
      case 'PageDown':
        e.preventDefault();
        setCursor(Math.min(rows.length - 1, cursor + 10));
        return;
      case 'PageUp':
        e.preventDefault();
        setCursor(Math.max(0, cursor - 10));
        return;
      case 'Tab':
        // パレットの外へフォーカスが飛ばないよう、Tab も上下移動に割り当てる。
        e.preventDefault();
        setCursor(cursor + (e.shiftKey ? -1 : 1));
        return;
      default:
        break;
    }
    // ⌃n / ⌃p（Emacs 流の上下移動）。⌘ 併用は下の toggle 側で扱う。
    if (e.ctrlKey && !e.metaKey && !e.altKey) {
      if (e.key === 'n') { e.preventDefault(); setCursor(cursor + 1); }
      else if (e.key === 'p') { e.preventDefault(); setCursor(cursor - 1); }
    }
  }

  function choose() {
    var row = rows[cursor];
    if (!row) return;
    close();
    if (opts && typeof opts.openFile === 'function') opts.openFile(row.path);
  }

  function open() {
    if (!opts) return;      // folder モード以外では初期化されていない
    if (overlay) return;
    // モーダルを重ねない（? のヘルプが開いていたら畳んでから出す）。
    if (window.MdHelp && window.MdHelp.close) window.MdHelp.close();
    buildOverlay();
    render();               // キャッシュがあれば即描画（無ければ「読み込み中…」）
    input.focus();
    // 開くたびに裏で取り直す。ファイルが増減していても次に開く時には追いつく。
    load().then(function() {
      if (!overlay) return;
      render();
    });
  }

  function close() {
    if (!overlay) return;
    overlay.remove();
    overlay = null;
    input = null;
    warnEl = null;
    listEl = null;
    hintEl = null;
    rows = [];
    cursor = 0;
  }

  function toggle() {
    if (overlay) close(); else open();
  }

  // ファイル一覧と変更ファイルを取得してキャッシュする。多重呼び出しは進行中の
  // fetch を共有する。片方が失敗しても、もう片方は活かす（変更一覧が取れなければ
  // 並べ替えとバッジが無いだけの、以前と同じパレットになる）。
  function load() {
    if (fetching) return fetching;
    fetching = Promise.all([loadFiles(), loadChanged()])
      .then(rebuild)
      .catch(function() {})
      .then(function() { fetching = null; });
    return fetching;
  }

  function loadFiles() {
    return fetch('/?files=1', { cache: 'no-store' })
      .then(function(r) { return r.ok ? r.json() : null; })
      .then(function(data) {
        if (!data || !Array.isArray(data.files)) return;
        serverFiles = data.files;
        truncation = data.truncated
          ? { reason: data.reason || '', limit: data.limit || 0 }
          : null;
        fetched = true;
      })
      .catch(function() {});
  }

  function loadChanged() {
    return fetch('/?changed=1', { cache: 'no-store' })
      .then(function(r) { return r.ok ? r.json() : null; })
      .then(function(data) {
        if (!data || !Array.isArray(data.changed)) return;
        // リポジトリ外なら空配列が返る。前回の結果を残さないよう毎回作り直す。
        var map = Object.create(null); // パス由来のキーなので __proto__ 等を踏まない器にする
        var paths = [];
        data.changed.forEach(function(c) {
          if (!c || typeof c.path !== 'string' || !c.path) return;
          map[c.path] = { add: c.add || 0, del: c.del || 0 };
          paths.push(c.path);
        });
        changedPaths = paths;
        stats = paths.length ? map : null;
      })
      .catch(function() {});
  }

  // 検索対象を組み直す。/?files=1 の一覧に無い変更ファイル（除外ディレクトリの中や
  // 探索打ち切りの向こう側にあるもの）も末尾に足す。変更のあるファイルは開きたい
  // 対象そのものなので、一覧の都合で引けない方が困る。
  function rebuild() {
    files = serverFiles.slice();
    var known = Object.create(null);
    for (var i = 0; i < serverFiles.length; i++) known[serverFiles[i]] = true;
    for (var c = 0; c < changedPaths.length; c++) {
      if (!known[changedPaths[c]]) files.push(changedPaths[c]);
    }
    lowerFiles = files.map(function(p) { return p.toLowerCase(); });
  }

  window.MdPalette = {
    // opts.openFile(relPath): 選んだファイルを開く（folder.js の loadPreview）
    init: function(o) {
      opts = o || {};
      if (initialized) return;
      initialized = true;
      document.addEventListener('keydown', function(e) {
        // パレット内の ⌃p（上へ移動）は input 側で処理済みなので、ここでは触らない。
        if (e.defaultPrevented) return;
        // 入力欄からフォーカスが外れていても Esc で閉じられるようにする
        // （リストの余白クリック等でフォーカスが抜けた時の保険）。
        if (e.key === 'Escape' && overlay) {
          e.preventDefault();
          close();
          return;
        }
        if (!(e.metaKey || e.ctrlKey) || e.shiftKey || e.altKey) return;
        if (e.key !== 'p' && e.key !== 'P' && e.code !== 'KeyP') return;
        e.preventDefault();
        toggle();
      });
    },
    // 起動直後に一覧を温めておく（初回の ⌘P を待たせない）。
    prefetch: function() { load(); },
    open: open,
    close: close,
    toggle: toggle
  };
})();
