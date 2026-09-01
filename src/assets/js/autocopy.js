// 範囲選択したらその場でクリップボードへ入れる（select-to-copy）。⌘C を押す手間を無くす。
// iTerm2 の "Copy to pasteboard on selection" / Ghostty の copy-on-select /
// herdr の ui.copy_on_select と同じ挙動で、どれも既定 ON なのでトグルは持たない。
//
// 発火は「左ボタンの mousedown → mouseup の対で、選択の中身が変わった時」だけ。
// mouseup 単独で見てはいけない——この機能はわざと選択を残すので、以後ずっと
// getSelection() は非空のままになり、無関係なクリックが全部再コピーになる。
// しかも選択を消さないよう mousedown を preventDefault しているハンドラが幾つもあり
// （contextmenu.js のメニュー・tabs.js のタブ・folder.js のリサイザ）、
// それらのクリックまで「コピーした」と言い出す。開始時点の選択と見比べて、
// このジェスチャが実際に選択を作った時だけ走らせる。
//
// ドラッグ選択とダブルクリック / トリプルクリックの単語・段落選択はこれで全部拾える。
// selectionchange まで見るとキーボード選択（Shift+矢印）でも走るが、そこまでは要らない
// （参考にした 3 つもマウス操作だけを対象にしている）。
//
// 成功の合図はマウスを離した位置に出すチェック 1 つ（.md-copy-tick）で、文字は出さない。
// 失敗した時だけ MdCommon.toast で文字を出す——黙って失敗すると、コピーできたつもりの
// まま貼りに行くことになる。
//
// 親 document ぶんはここで張り、.html-frame の中は common.js の bindFrame が
// bindDoc() を呼んで張る（iframe の mouseup は親に届かないため）。
(function() {
  var TICK_MS = 620;   // 読ませる文字が無いぶん、トースト（1500ms）より短くてよい

  // 入力欄の中の選択か。ドラッグを欄の外で離すと mouseup の target は本文に化けるので、
  // イベント側だけでなく選択の起点ノード側も見る。contenteditable は子孫にも効くので
  // 要素そのものを見る isFieldEl では足りず、closest で祖先まで辿る。
  var FIELD_SEL = 'input, textarea, [contenteditable=""], [contenteditable="true"]';
  function inField(node) {
    var el = node && (node.nodeType === 1 ? node : node.parentElement);
    return !!(el && el.closest && el.closest(FIELD_SEL));
  }

  // ── 合図のチェック ──────────────────────────────────────────
  // 1 つを作り置いて位置と表示だけ動かす（なぞるたびに DOM を作り直さない）。
  var tickEl = null, tickTimer = null;

  function ensureTick() {
    if (tickEl) return tickEl;
    tickEl = document.createElement('div');
    tickEl.className = 'md-copy-tick';
    tickEl.setAttribute('role', 'status');
    tickEl.setAttribute('aria-live', 'polite');
    tickEl.innerHTML =
      '<svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">' +
      '<path d="M3 8.5l3.2 3.2L13 4.8" stroke="currentColor" stroke-width="2.1" ' +
      'stroke-linecap="round" stroke-linejoin="round"/></svg>' +
      '<span class="md-a11y"></span>';
    document.body.appendChild(tickEl);
    return tickEl;
  }

  // x/y はビューポート座標（position:fixed と同じ基準）。
  function showTick(x, y) {
    var el = ensureTick();
    var SIZE = 24, GAP = 12, EDGE = 6;
    // カーソルの右下に置く。画面の端では内側へ折り返して、はみ出して見えなくなるのを防ぐ。
    var left = x + GAP;
    var top = y + GAP;
    if (left + SIZE > window.innerWidth - EDGE) left = x - GAP - SIZE;
    if (top + SIZE > window.innerHeight - EDGE) top = y - GAP - SIZE;
    el.style.left = Math.max(EDGE, left) + 'px';
    el.style.top = Math.max(EDGE, top) + 'px';

    // 連続してなぞった時に、出かかりのアニメーションを頭から演り直させる。
    el.classList.remove('show');
    void el.offsetWidth;   // ここで再フローさせないと、class の付け外しが 1 つに畳まれる
    el.classList.add('show');

    // 文字は読み上げにだけ渡す。空 → 文言の変化で aria-live が鳴るので、消す時に
    // クリアしておく（同じタスク内で '' と文言を続けて入れても、最終状態が前回と
    // 同じなら読み上げは起きない）。DOM に文言を residue として残さない意味もある。
    var label = el.lastChild;
    label.textContent = 'コピーしました';

    if (tickTimer) clearTimeout(tickTimer);
    tickTimer = setTimeout(function() {
      el.classList.remove('show');
      label.textContent = '';
    }, TICK_MS);
  }

  // ── 本体 ────────────────────────────────────────────────────
  // 左ドラッグの開始時点。{ doc, prev } を持ち、mouseup で使い切って捨てる。
  // 同時に 2 つのジェスチャは走らないので 1 つで足りる。
  var gesture = null;

  function onMouseDown(doc, e) {
    // 右 / 中クリックは選択のジェスチャではない（右はメニュー、中はタブを閉じる）。
    if (e.button !== 0) { gesture = null; return; }
    gesture = { doc: doc, prev: MdCommon.selectionText(doc) };
  }

  // frame は .html-frame の要素（iframe から呼ばれた時だけ）。iframe 内の座標は
  // その文書のビューポート基準なので、親のビューポート基準へ寄せてから合図を出す。
  function onMouseUp(doc, frame, e) {
    var g = gesture;
    gesture = null;
    // 対になる mousedown をこの文書で受けていない mouseup は、選択のジェスチャの
    // 終わりではない（別フレームで始まった、あるいは右クリックだった）。
    if (!g || g.doc !== doc || e.button !== 0) return;

    // コメントモード中の本文ドラッグは「行ユニットの範囲選択」に割り当たっていて
    // （comment.js の mousedown）、本文は user-select:none なので拾うものが無い。
    // 通すとコメント付与と同時に合図が出て、何が起きたか読めなくなる。
    //
    // iframe の中身は別扱いで、モード中でも生かす。comment.js のハンドラは親 document
    // 直付けで iframe には届かず競合しない上、html にはそもそも行コメントを付けられない
    // ので、ここで止めると読む側が損をするだけになる。
    if (doc === document && window.MdComment && MdComment.isMode()) return;
    if (inField(e.target)) return;

    var sel;
    try { sel = doc.getSelection ? doc.getSelection() : null; } catch (err) { return; }
    if (!sel || sel.isCollapsed) return;
    if (inField(sel.anchorNode) || inField(sel.focusNode)) return;

    var text = MdCommon.selectionText(doc);
    if (!text) return;
    // 開始時と変わっていないなら、このジェスチャは選択を作っていない。
    // 素の mousedown は選択を畳むので、本当になぞった時は prev が空になって必ず通る
    // （同じ範囲をなぞり直した時も、間に畳まれるので通る）。残るのは
    // 「選択を保つために mousedown を preventDefault する UI」——メニュー行・タブ・
    // サイドバーのリサイザ——のクリックで、そこだけがここで落ちる。
    if (text === g.prev) return;

    var x = e.clientX, y = e.clientY;
    if (frame) {
      var r = frame.getBoundingClientRect();
      x += r.left;
      y += r.top;
    }

    // 選択は保ったまま入れる（copyText は選択に触らない）。参考にした 3 つと同じく、
    // なぞった範囲が消えずに残るのが期待される挙動。
    MdCommon.copyText(
      text,
      function() { showTick(x, y); },
      function() { MdCommon.toast('コピーに失敗しました'); }
    );
  }

  function bindDoc(doc, frame) {
    if (!doc || doc.__mdAutoCopy) return;
    doc.__mdAutoCopy = true;
    doc.addEventListener('mousedown', function(e) { onMouseDown(doc, e); });
    doc.addEventListener('mouseup', function(e) { onMouseUp(doc, frame, e); });
  }

  bindDoc(document, null);

  window.MdAutoCopy = {
    // common.js の bindFrame が .html-frame の中身に対して呼ぶ。
    bindDoc: bindDoc
  };
})();
