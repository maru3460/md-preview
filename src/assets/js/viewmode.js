// 「通常表示の代わりに別の見せ方を出すモード」の共通実装。いまは raw（ソース）と
// diff（git 差分）の 2 つで、右下ボタン or ⌘R / ⌘D で切り替える。
//
// モードは維持される。ON のまま別ファイルへ移ると、そのファイルのソース / 差分が出る
// （フォルダモードの loadPreview 側が isActive() を見て分岐する）。同時に active に
// なれるのは 1 つで、排他はこのファイルの activeId が持つ（以前は diff.js と raw.js が
// 互いの isActive()/deactivate() を名指しで呼び合っており、3 つ目を足すと組み合わせが
// 増える形だった）。
//
// init.js（単一ファイル）と folder.js（フォルダ）が initAll() でモード固有の入出力を渡す。
(function() {
  // モードの定義。ここに 1 行足せば、ボタン・ショートカット・排他・再取得が揃う。
  // CSS は `.md-<id>-toggle` / `.md-<id>-count` を見る（並び順は base.css の order）。
  // キー割り当ては keymap.js の `view-<id>` 行が持つ（ここには書かない）。
  var SPECS = [
    { id: 'raw',  label: 'Raw',  title: 'Toggle raw source (Cmd/Ctrl+R)', global: 'MdRaw' },
    { id: 'diff', label: 'Diff', title: 'Toggle git diff (Cmd/Ctrl+D)', global: 'MdDiff', badge: true }
  ];

  var opts = null;      // { getContainer, getScroller, url(id), getStatUrl, reloadNormal }
  var activeId = null;  // 同時に active になれるのは 1 つだけ
  var wired = false;

  function container() { return opts && opts.getContainer ? opts.getContainer() : null; }
  function scroller() { return opts && opts.getScroller ? opts.getScroller() : null; }

  function createMode(spec) {
    var btn = null;
    var badgeEl = null;
    var available = true;  // 現在のファイルでこのモードが意味を持つか（raw のみ使う）
    var widthAdded = false; // 全幅(source-page)を自分で付けたか。付けた時だけ外す。
    var reqSeq = 0;         // 進行中フェッチの世代。無関係になった応答を捨てる。
    var statSeq = 0;        // バッジ集計フェッチの世代（同上）。

    function isActive() { return activeId === spec.id; }

    function buildButton() {
      btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'md-' + spec.id + '-toggle';
      btn.setAttribute('aria-pressed', 'false');
      btn.title = spec.title;
      btn.appendChild(document.createTextNode(spec.label));
      if (spec.badge) {
        badgeEl = document.createElement('span');
        badgeEl.className = 'md-' + spec.id + '-count';
        btn.appendChild(badgeEl);
      }
      // toggle 後はボタンからフォーカスを外す。残るとスクロール素キーが不発になり、
      // Space でボタンが再トグルされてしまうため（本文側へフォーカスを返す）。
      btn.addEventListener('click', function() { toggle(); btn.blur(); });
      var stack = (window.MdCommon && MdCommon.cornerStack) ? MdCommon.cornerStack() : document.body;
      stack.appendChild(btn);
    }

    function updateButton() {
      if (!btn) return;
      btn.classList.toggle('active', isActive());
      btn.setAttribute('aria-pressed', isActive() ? 'true' : 'false');
    }

    // ソース / 差分は本文の 720px 制約を外して全幅で見せる。単一ファイルモードでは
    // コンテナ（.markdown-body 記事）に source-page を付ける。フォルダモードでは
    // Rust 側フラグメントが source-page を持つため、#preview-pane への付与は実質 no-op。
    // 自分で付けた時だけ外す（非md記事が恒久的に持つ source-page を剥がさないため）。
    function applyWidth(el) {
      if (el && !el.classList.contains('source-page')) {
        el.classList.add('source-page');
        widthAdded = true;
      }
    }
    function clearWidth() {
      if (!widthAdded) return;
      var el = container();
      if (el) el.classList.remove('source-page');
      widthAdded = false;
    }

    // 状態だけ OFF にする（通常表示には戻さない）。別モードへ切り替える時など、
    // この直後に相手がコンテナを上書きする前提で使う。
    function deactivate() {
      if (!isActive()) return;
      activeId = null;
      updateButton();
      reqSeq++;  // 進行中のフェッチを無効化する
      clearWidth();
    }

    // 取得 / 表示に失敗したらモードを解除して通常表示へ戻す。
    function fail() {
      deactivate();
      if (opts) opts.reloadNormal();
    }

    function show() {
      if (!opts) return;
      var url = opts.url(spec.id);
      var el = container();
      if (!url || !el) { fail(); return; }
      var sc = scroller();
      var savedScroll = sc ? sc.scrollTop : 0;
      var myReq = ++reqSeq;
      fetch(url, { cache: 'no-store' })
        .then(function(r) { return r.ok ? r.text() : null; })
        .then(function(html) {
          // 応答が届くまでに OFF / 別ファイル / 別モードで無効化されていたら捨てる。
          if (myReq !== reqSeq || !isActive()) return;
          if (html == null) { fail(); return; }
          el.innerHTML = html;
          applyWidth(el);
          MdCommon.hydrate(el);
          if (sc) sc.scrollTop = savedScroll;
        })
        .catch(function() { if (myReq === reqSeq) fail(); });
    }

    function toggle() {
      if (!opts || !available) return;
      if (isActive()) {
        deactivate();
        opts.reloadNormal();
        return;
      }
      // 表示できるファイルが無ければ何もしない（フォルダモードで未選択のとき等）。
      if (!opts.url(spec.id)) return;
      // 他モードとは排他。出ていたら先に畳む（本文には戻さず、こちらで上書きする）。
      deactivateOthers(spec.id);
      activeId = spec.id;
      updateButton();
      show();
    }

    // 現在ファイルでこのモードが意味を持たないならボタンを隠し、表示中なら畳む。
    function setAvailable(ok) {
      available = !!ok;
      if (btn) btn.style.display = available ? '' : 'none';
      if (!available) deactivate();
    }

    // バッジ（+N −M）を更新する。両方 0 なら空＝差分なし。textContent なので XSS 無し。
    function refreshStat() {
      if (!badgeEl || !opts || !opts.getStatUrl) return;
      var url = opts.getStatUrl();
      if (!url) { badgeEl.textContent = ''; return; }
      var myStat = ++statSeq;
      fetch(url, { cache: 'no-store' })
        .then(function(r) { return r.ok ? r.json() : null; })
        .then(function(d) {
          if (myStat !== statSeq || !d) return;
          var add = d.add || 0, del = d.del || 0;
          badgeEl.textContent = (add || del) ? (' +' + add + ' −' + del) : '';
        })
        .catch(function() {});
    }

    buildButton();

    return {
      id: spec.id,
      isActive: isActive,
      deactivate: deactivate,
      // 状態だけ ON にする（取得も表示もしない）。タブ切替のように、この直後に
      // 呼び出し側が本文を出し直す場面で使う（deactivate の対）。
      //
      // ここで available を見てはいけない。available が指すのは**まだ切り替える前の**
      // ファイルの可否で、新しいファイルの分は直後の loadPreview が setAvailable で
      // 入れ直す。見てしまうと「非 md を経由してから raw のタブへ戻ると raw が
      // 復元されない」になる。開けない組み合わせはその setAvailable が畳む。
      activate: function() {
        activeId = spec.id;
        updateButton();
      },
      setAvailable: setAvailable,
      // 表示中に（現在ファイルに対して）再取得する。ファイル切替・監視リロード・
      // 明示更新のいずれからも使う。バッジも合わせて更新する。
      refresh: function() { if (isActive()) show(); refreshStat(); },
      // バッジだけ更新する。通常表示の経路（ファイル切替・ホットリロード）から使う。
      refreshStat: refreshStat,
      toggle: toggle,
      isAvailable: function() { return available; }
    };
  }

  var modes = [];

  function deactivateOthers(id) {
    modes.forEach(function(m) { if (m.id !== id) m.deactivate(); });
  }

  // キーの割り当てと「入力欄では譲る」判定は keymap.js の表が持つ。
  // ここでは対応するモードの toggle を差し込むだけ（無効なモードは toggle 側で no-op）。
  function attachShortcuts() {
    if (!window.MdKeymap) return;
    modes.forEach(function(mode) {
      MdKeymap.on('view-' + mode.id, function() { mode.toggle(); });
    });
  }

  window.MdViewModes = {
    // o: { getContainer(), getScroller(), url(id), getStatUrl(), reloadNormal() }
    // 2 回目以降の呼び出しは入出力の差し替えだけ（ボタンとリスナは作り直さない）。
    initAll: function(o) {
      opts = o;
      if (wired) return;
      wired = true;
      SPECS.forEach(function(spec) {
        var mode = createMode(spec);
        modes.push(mode);
        // 既存の呼び出し側（init.js / folder.js / MdReload）が使う名前で公開する。
        window[spec.global] = mode;
      });
      attachShortcuts();
    },
    // いま出ているモード（無ければ null）。ファイル切替時の分岐に使う。
    active: function() {
      for (var i = 0; i < modes.length; i++) if (modes[i].isActive()) return modes[i];
      return null;
    },
    // いま出ているモードの id（無ければ null）。タブが保存する値。
    currentId: function() { return activeId; },
    // 保存しておいた id へ状態だけ戻す。表示の更新は呼び出し側（loadPreview）が行う。
    // 対象が現在のファイルで意味を持たない（raw が無効など）なら OFF のままになる。
    restore: function(id) {
      if (id === activeId) return;
      deactivateOthers(id || '');
      if (!id) return;
      for (var i = 0; i < modes.length; i++) {
        if (modes[i].id === id) { modes[i].activate(); return; }
      }
    }
  };
})();
