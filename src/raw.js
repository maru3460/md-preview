// raw（ソース）表示のトグル。diff.js と同じ流儀で window.MdRaw を公開する。
// レンダリングされたプレビューと、Markdown の生ソース（レンダリング前のテキスト）を、
// 右下ボタン or Cmd/Ctrl+R で切り替える。raw は diff と同じく「モード」として維持され、
// ON のまま別ファイルへ移るとそのファイルのソースが出る（folder モードの loadPreview 側が
// isActive() を見て分岐する）。diff と raw は排他で、一方を ON にすると他方は畳まれる。
// init.js（単一ファイル）と folder.js（フォルダ）が init() でモード固有の入出力を渡す。
(function() {
  var active = false;
  var initialized = false;
  var btn = null;
  var opts = null;
  var available = true; // 現在のファイルで raw 表示が意味を持つか（.md のみ true）。
  var widthAdded = false; // raw 用に source-page(全幅)を自分で付けたか。付けた時だけ外す。
  var reqSeq = 0; // 進行中フェッチの世代。無関係になった応答（OFF/別ファイル/連打）を捨てる。

  function buildButton() {
    btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'md-raw-toggle';
    btn.setAttribute('aria-pressed', 'false');
    btn.title = 'Toggle raw source (Cmd/Ctrl+R)';
    btn.appendChild(document.createTextNode('Raw'));
    // toggle 後はボタンからフォーカスを外す。残るとスクロール素キーが不発になり、
    // Space でボタンが再トグルされてしまうため（本文側へフォーカスを返す）。
    btn.addEventListener('click', function() { toggle(); btn.blur(); });
    // 右下の共有スタックへ（diff/コメントと下から詰めて並ぶ）。
    var stack = (window.MdCommon && MdCommon.cornerStack) ? MdCommon.cornerStack() : document.body;
    stack.appendChild(btn);
  }

  function updateButton() {
    if (!btn) return;
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-pressed', active ? 'true' : 'false');
  }

  // raw はソース表示なので、非md のソースビューと同じく全幅にする。単一ファイルモードでは
  // コンテナ（.markdown-body 記事）に source-page を付けて 720px 制約を外す。フォルダモード
  // では Rust 側フラグメントが source-page を持つため、#preview-pane への付与は実質 no-op。
  // 自分で付けた時だけ外す（非md記事が恒久的に持つ source-page を誤って剥がさないため）。
  function applyWidth(container) {
    if (container && !container.classList.contains('source-page')) {
      container.classList.add('source-page');
      widthAdded = true;
    }
  }
  function clearWidth() {
    if (!widthAdded) return;
    var c = opts && opts.getContainer ? opts.getContainer() : null;
    if (c) c.classList.remove('source-page');
    widthAdded = false;
  }

  // raw の取得/表示に失敗したら、モードを解除して通常表示へ戻す。
  function fail() {
    active = false;
    updateButton();
    reqSeq++;
    clearWidth();
    if (opts) opts.reloadNormal();
  }

  // ソースを取得して表示中のコンテナに差し込む。中身は 1 個の <pre><code> なので、
  // hljs で構文ハイライトし、Copy ボタンも付ける（全文コピー用）。
  // active は呼び出し側（toggle / loadPreview）で先に立てておく前提。
  function showRaw() {
    if (!opts) return;
    var url = opts.getRawUrl();
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
        applyWidth(container);
        if (window.hljs) {
          container.querySelectorAll('pre code').forEach(function(el) {
            hljs.highlightElement(el);
          });
        }
        if (window.MdCommon) { MdCommon.addCopyButtons(container); MdCommon.addLineNumbers(container); }
        if (window.MdSearch) window.MdSearch.init(container);
        if (window.MdToc) window.MdToc.refresh();
        if (scroller) scroller.scrollTop = savedScroll;
      })
      .catch(function() { if (myReq === reqSeq) fail(); });
  }

  // 非 md ファイルでは通常表示が既にソースなので raw は無効。ボタンを隠し、
  // 表示中なら状態だけ畳む（通常表示＝ソースビューへは呼び出し側が戻す）。
  function setAvailable(ok) {
    available = !!ok;
    if (btn) btn.style.display = available ? '' : 'none';
    if (!available && active) deactivate();
  }

  function toggle() {
    if (!opts || !available) return;
    if (active) {
      active = false;
      updateButton();
      reqSeq++; // 進行中の raw フェッチを無効化してから通常表示へ戻す。
      clearWidth();
      opts.reloadNormal();
    } else {
      // 表示できるファイルが無ければ何もしない（フォルダモードで未選択のとき等）。
      if (!opts.getRawUrl()) return;
      // diff 表示とは排他。相手が出ていたら先に畳む（本文には戻さず raw で上書きする）。
      if (window.MdDiff && window.MdDiff.isActive()) window.MdDiff.deactivate();
      active = true;
      updateButton();
      showRaw();
    }
  }

  // 状態だけ OFF にする（通常表示には戻さない）。diff 表示へ切り替える時など、
  // この直後に別モードがコンテナを上書きする前提で使う。
  function deactivate() {
    if (!active) return;
    active = false;
    updateButton();
    reqSeq++;
    clearWidth();
  }

  window.MdRaw = {
    // opts: { getContainer(), getScroller(), getRawUrl(), reloadNormal() }
    init: function(o) {
      opts = o;
      if (initialized) return;
      initialized = true;
      buildButton();
      document.addEventListener('keydown', function(e) {
        if (!((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey
            && (e.key === 'r' || e.key === 'R' || e.code === 'KeyR'))) return;
        // 非 md ファイル（raw 無効）ではブラウザ既定に委ね、何も奪わない。
        if (!available) return;
        // 入力欄（検索ボックス等）にフォーカス中は通常のブラウザ挙動に委ねる。
        var t = e.target;
        if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
        e.preventDefault();
        toggle();
      });
    },
    isActive: function() { return active; },
    // 現在ファイルが .md かどうかで raw の有効/無効を切り替える（呼び出し側から）。
    setAvailable: setAvailable,
    // diff 表示など、他モードへ切り替える際に状態だけ畳む（通常表示には戻さない）。
    deactivate: deactivate,
    // raw 表示中に（現在ファイルに対して）再取得する。ファイル切替・監視リロード・
    // 明示更新のいずれからも使う。
    refresh: function() { if (active) showRaw(); }
  };
})();
