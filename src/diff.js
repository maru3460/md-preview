// git diff 表示のトグル。search.js / toc.js と同じ流儀で window.MdDiff を公開する。
// 通常のレンダリングプレビューと、現在ファイルの VSCode 風ソース差分（`git diff HEAD`、
// インライン +/- と行内文字強調）を、右下ボタン or Cmd/Ctrl+D で切り替える。
// diff は「モード」として維持され、ON のまま別ファイルへ移るとそのファイルの差分が出る
// （フォルダモードの loadPreview 側が isActive() を見て分岐する）。
// init.js（単一ファイル）と folder.js（フォルダ）が init() でモード固有の入出力を渡す。
(function() {
  var active = false;
  var initialized = false;
  var btn = null;
  var countEl = null;
  var opts = null;
  var reqSeq = 0; // 進行中フェッチの世代。無関係になった応答（OFF/別ファイル/連打）を捨てる。
  var statSeq = 0; // バッジ集計フェッチの世代（同上）。

  function buildButton() {
    btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'md-diff-toggle';
    btn.setAttribute('aria-pressed', 'false');
    btn.title = 'Toggle git diff (Cmd/Ctrl+D)';
    btn.appendChild(document.createTextNode('Diff'));
    countEl = document.createElement('span');
    countEl.className = 'md-diff-count';
    btn.appendChild(countEl);
    btn.addEventListener('click', function() { toggle(); });
    document.body.appendChild(btn);
  }

  function updateButton() {
    if (!btn) return;
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-pressed', active ? 'true' : 'false');
  }

  // バッジに +N −M を出す（両方 0 なら空＝差分なし）。textContent なので XSS 無し。
  function setBadge(add, del) {
    if (!countEl) return;
    if (add || del) {
      countEl.textContent = ' +' + add + ' −' + del;
    } else {
      countEl.textContent = '';
    }
  }

  // 現在ファイルの追加/削除行数を非同期に取りに行き、バッジを更新する。
  // プレビュー描画はブロックしない（fire-and-forget）。
  function refreshStat() {
    if (!opts || !opts.getStatUrl) return;
    var url = opts.getStatUrl();
    if (!url) { setBadge(0, 0); return; }
    var myStat = ++statSeq;
    fetch(url, { cache: 'no-store' })
      .then(function(r) { return r.ok ? r.json() : null; })
      .then(function(d) {
        if (myStat !== statSeq || !d) return;
        setBadge(d.add || 0, d.del || 0);
      })
      .catch(function() {});
  }

  // diff の取得/表示に失敗したら、モードを解除して通常表示へ戻す。
  // 進行中の他フェッチも世代を進めて無効化する（HTTP/ネットワーク双方で一貫）。
  function fail() {
    active = false;
    updateButton();
    reqSeq++;
    if (opts) opts.reloadNormal();
  }

  // diff を取得して表示中のコンテナに差し込む。中身はレンダリングではなくソース差分
  // （自前の行/文字ハイライト）なので、hljs/mermaid/drawio の後処理は回さない。
  // active は呼び出し側（toggle / loadPreview）で先に立てておく前提。
  function showDiff() {
    if (!opts) return;
    var url = opts.getDiffUrl();
    var container = opts.getContainer();
    if (!url || !container) { fail(); return; }
    var scroller = opts.getScroller ? opts.getScroller() : null;
    var savedScroll = scroller ? scroller.scrollTop : 0;
    var myReq = ++reqSeq;
    fetch(url, { cache: 'no-store' })
      .then(function(r) { return r.ok ? r.text() : null; })
      .then(function(html) {
        // 応答が届くまでに OFF/別ファイル/別トグルで無効化されていたら捨てる。
        if (myReq !== reqSeq || !active) return;
        if (html == null) { fail(); return; }
        if (window.MdSearch) window.MdSearch.reset();
        container.innerHTML = html;
        if (window.MdSearch) window.MdSearch.init(container);
        if (window.MdToc) window.MdToc.refresh();
        if (scroller) scroller.scrollTop = savedScroll;
      })
      .catch(function() { if (myReq === reqSeq) fail(); });
  }

  function toggle() {
    if (!opts) return;
    if (active) {
      active = false;
      updateButton();
      reqSeq++; // 進行中の diff フェッチを無効化してから通常表示へ戻す。
      opts.reloadNormal();
    } else {
      // 表示できるファイルが無ければ何もしない（フォルダモードで未選択のとき等）。
      if (!opts.getDiffUrl()) return;
      // raw 表示とは排他。相手が出ていたら先に畳む（本文には戻さず差分で上書きする）。
      if (window.MdRaw && window.MdRaw.isActive()) window.MdRaw.deactivate();
      active = true;
      updateButton();
      showDiff();
    }
  }

  // 状態だけ OFF にする（通常表示には戻さない）。raw 表示へ切り替える時など、
  // この直後に別モードがコンテナを上書きする前提で使う。
  function deactivate() {
    if (!active) return;
    active = false;
    updateButton();
    reqSeq++; // 進行中の diff フェッチを無効化する。
  }

  window.MdDiff = {
    // opts: { getContainer(), getScroller(), getDiffUrl(), reloadNormal() }
    init: function(o) {
      opts = o;
      if (initialized) return;
      initialized = true;
      buildButton();
      document.addEventListener('keydown', function(e) {
        if (!((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey
            && (e.key === 'd' || e.key === 'D' || e.code === 'KeyD'))) return;
        // 入力欄（検索ボックス等）にフォーカス中は通常のブラウザ挙動に委ねる。
        var t = e.target;
        if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
        e.preventDefault();
        toggle();
      });
    },
    isActive: function() { return active; },
    // raw 表示など、他モードへ切り替える際に状態だけ畳む（通常表示には戻さない）。
    deactivate: deactivate,
    // diff 表示中に（現在ファイルに対して）再取得する。ファイル切替・監視リロード・
    // 明示更新のいずれからも使う。バッジも合わせて更新する。
    refresh: function() { if (active) showDiff(); refreshStat(); },
    // バッジ（+N −M）だけ更新する。通常表示の経路（ファイル切替・ホットリロード）から使う。
    refreshStat: refreshStat
  };
})();
