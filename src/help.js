// キーボードショートカット一覧のオーバーレイ。search.js / toc.js / diff.js と同じ流儀で
// window.MdHelp を公開する。`?`（Shift+/）で開閉、Esc / 背景クリックで閉じる。
// 右クリックメニューの「ショートカット一覧」からも MdHelp.open() で開ける。
//
// 配色は context-menu / diff-toggle と同じくシステムカラー（Canvas/CanvasText/…）と
// color-mix で theme-agnostic に組む。themes/*.css への追記は不要。
(function() {
  var overlay = null;

  // [キー, 説明, stdin でも出すか]。表示順そのまま。
  // stdin モードはファイルもサイドバーも git も無いので一部を伏せる。
  var SHORTCUTS = [
    ['⌘F', '検索', true],
    ['⌘T', 'アウトライン（見出しナビ）を開閉', true],
    ['⌘D', 'git 差分表示を切り替え', false],
    ['⌘R', 'raw（ソース）表示を切り替え', false],
    ['⌘A', '本文を全選択', true],
    ['⌘W', 'ウィンドウを閉じる', true],
    ['⌘Q', '終了', true],
    ['右クリック', 'コンテキストメニュー', true],
    ['?', 'このヘルプを開閉', true],
    ['Esc', '検索 / メニュー / ヘルプを閉じる', true]
  ];

  function isStdin() {
    return window.MD_MENU_MODE === 'stdin';
  }

  function build(welcome) {
    // パネルを一度でも開いたら、初回の自動表示はもう出さない。
    markOnboarded();
    overlay = document.createElement('div');
    overlay.className = 'md-help-backdrop';

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
      intro.textContent = 'md へようこそ。使えるショートカットの一覧です。この一覧は「?」でいつでも開けます。';
      panel.appendChild(intro);
    }

    var stdin = isStdin();
    SHORTCUTS.forEach(function(sc) {
      if (!sc[2] && stdin) return;
      var row = document.createElement('div');
      row.className = 'md-help-row';
      var key = document.createElement('kbd');
      key.className = 'md-help-key';
      key.textContent = sc[0];
      var desc = document.createElement('span');
      desc.className = 'md-help-desc';
      desc.textContent = sc[1];
      row.appendChild(key);
      row.appendChild(desc);
      panel.appendChild(row);
    });

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
  }

  function open(welcome) {
    if (overlay) { close(); return; }
    build(welcome);
  }

  function close() {
    if (!overlay) return;
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
