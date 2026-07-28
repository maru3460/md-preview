// キーボードショートカット一覧のオーバーレイ。search.js / toc.js / diff.js と同じ流儀で
// window.MdHelp を公開する。`?`（Shift+/）で開閉、Esc / 背景クリックで閉じる。
// 右クリックメニューの「ショートカット一覧」からも MdHelp.open() で開ける。
//
// 表示する内容（キー・説明・表示範囲）は keymap.js の MdKeymap が単一の定義元。
// ここは描画だけを持つ。ショートカットの追加・変更は keymap.js 側で行う。
//
// 配色は context-menu / diff-toggle と同じくシステムカラー（Canvas/CanvasText/…）と
// color-mix で theme-agnostic に組む。themes/*.css への追記は不要。
(function() {
  var overlay = null;
  var onResize = null;    // 開いている間だけ張る resize ハンドラ
  var resizeTick = false;

  function build(welcome) {
    // パネルを一度でも開いたら、初回の自動表示はもう出さない。
    markOnboarded();
    overlay = document.createElement('div');
    overlay.className = 'md-help-backdrop';
    overlay.id = 'md-help-backdrop'; // MdCommon.isOverlayOpen が O(1) で存在を見るため

    var panel = document.createElement('div');
    panel.className = 'md-help-panel';

    var title = document.createElement('div');
    title.className = 'md-help-title';
    title.textContent = 'キーボードショートカット';
    panel.appendChild(title);

    // 初回だけ、使い方の一言を添える。「?」でいつでも呼べることを伝える。
    if (welcome) {
      var intro = document.createElement('div');
      intro.className = 'md-help-welcome';
      intro.textContent = 'md へようこそ。使えるショートカットの一覧です。この一覧は「?」でいつでも開けます。'
        + '覚えるまでは「⌘/」で画面の右端に出しっぱなしにできます。';
      panel.appendChild(intro);
    }

    // 行は専用ラッパへ入れ、CSS の段組み(column-count)で上→下→次の列と流す。
    // 列単位で流れるのでカテゴリのまとまりが崩れず、縦の高さも半分に収まる。
    // カテゴリ見出しごと 1 つの箱に入れ、見出しだけが列末に取り残されないようにする。
    var rows = document.createElement('div');
    rows.className = 'md-help-rows';
    window.MdKeymap.groups().forEach(function(g) {
      var group = document.createElement('div');
      group.className = 'md-help-group';

      var head = document.createElement('div');
      head.className = 'md-help-cat';
      head.textContent = g.cat.label;
      group.appendChild(head);

      g.binds.forEach(function(b) {
        var row = document.createElement('div');
        row.className = 'md-help-row';
        var key = document.createElement('kbd');
        key.className = 'md-help-key';
        key.textContent = b.keys;
        var desc = document.createElement('span');
        desc.className = 'md-help-desc';
        desc.textContent = b.desc;
        row.appendChild(key);
        row.appendChild(desc);
        group.appendChild(row);
      });
      rows.appendChild(group);
    });
    panel.appendChild(rows);

    var hint = document.createElement('div');
    hint.className = 'md-help-hint';
    hint.textContent = 'Esc または背景クリックで閉じる';
    panel.appendChild(hint);

    overlay.appendChild(panel);
    // 背景クリックで閉じる（パネル内クリックは無視）。
    overlay.addEventListener('mousedown', function(e) {
      if (e.target === overlay) close();
    });
    document.body.appendChild(overlay);
    fit(panel);
    // 開いている間に窓が変わったら測り直す（閉じる時に外す）。
    onResize = function() {
      if (resizeTick) return;
      resizeTick = true;
      requestAnimationFrame(function() { resizeTick = false; if (overlay) fit(panel); });
    };
    window.addEventListener('resize', onResize, { passive: true });
  }

  // ── 高さに収める ──────────────────────────────────────────────
  // この一覧にもフォーカスは無く、キーボードでスクロールできない。見えない行は
  // 存在しないのと同じなので、スクロールさせずに収める（コマンドパネルと同じ方針）。
  // 段組みのモーダルは横に伸ばせるぶん有利なので、まず段数を増やす。
  //   1. そのまま(2段) → 2. 3段 → 3. 4段 → 4. 密度を詰める → 5. 最後の手段でスクロール
  // 段数を増やしても本文の可読性は落ちないので、密度を詰めるより先に試す。

  // [クラス, その段数にしてよい最小の窓幅]。狭い窓で段だけ増やすと 1 段が細くなりすぎる。
  // ただし「1 段が窮屈」より「行が見えない」ほうが悪いので、閾値は攻めに倒す
  // （幅 700 の 3 段 = 1 段あたり約 210px。説明文はよく折り返すが読める）。
  var COL_STEPS = [['cols-3', 700], ['cols-4', 1150]];

  function overflowing(panel) {
    return panel.scrollHeight > panel.clientHeight + 1;
  }

  function fit(panel) {
    // 開いたまま窓の大きさが変わることがあるので、毎回まっさらから測り直す。
    // overflow:hidden なので、収め損ねると（スクロールバーが出るのではなく）行が消える。
    panel.classList.remove('cols-3', 'cols-4', 'is-compact', 'is-scroll');
    if (!overflowing(panel)) return;

    var vw = document.documentElement.clientWidth || window.innerWidth || 0;
    for (var i = 0; i < COL_STEPS.length; i++) {
      if (vw < COL_STEPS[i][1]) break;
      if (i > 0) panel.classList.remove(COL_STEPS[i - 1][0]);
      panel.classList.add(COL_STEPS[i][0]);
      if (!overflowing(panel)) return;
    }

    panel.classList.add('is-compact');
    if (!overflowing(panel)) return;

    panel.classList.add('is-scroll');
  }

  function open(welcome) {
    if (overlay) { close(); return; }
    build(welcome);
  }

  function close() {
    if (!overlay) return;
    if (onResize) { window.removeEventListener('resize', onResize); onResize = null; }
    overlay.remove();
    overlay = null;
  }

  // 初回オンボーディング。初めて使うときだけ、ショートカット一覧を
  // 自動で開いて使い方を提示する。一度出したら（＝パネルを一度でも開いたら）
  // 二度と自動では出さない。
  var ONBOARDED_KEY = 'md-help-onboarded';

  function lsGet(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function lsSet(k, v) { try { localStorage.setItem(k, v); } catch (e) {} }

  function markOnboarded() { lsSet(ONBOARDED_KEY, '1'); }

  function maybeOnboard() {
    if (lsGet(ONBOARDED_KEY) === '1') return;
    // 既にユーザーが自分で開いていたら邪魔しない（フラグだけ立てる）。
    if (overlay) { markOnboarded(); return; }
    // 描画が落ち着いてから、歓迎の一言つきで一覧を開く。
    setTimeout(function() {
      if (overlay || lsGet(ONBOARDED_KEY) === '1') { markOnboarded(); return; }
      open(true);
    }, 500);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', maybeOnboard);
  } else {
    maybeOnboard();
  }

  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape' && overlay) {
      e.preventDefault();
      close();
      return;
    }
    if (e.key !== '?' || e.metaKey || e.ctrlKey || e.altKey) return;
    // 入力欄（検索ボックス等）にフォーカス中は通常入力に委ねる。
    var t = e.target;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    e.preventDefault();
    open();
  });

  window.MdHelp = {
    open: function() {
      // 右クリックメニューから呼ばれたとき、既に開いていれば開き直さず維持。
      if (!overlay) open();
    },
    close: close
  };
})();
