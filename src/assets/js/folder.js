(function() {
  // 現在プレビュー中のファイルの識別子（絶対パス）。何も開いていなければ null。
  var currentFilePath = null;
  // 本文フェッチの世代。loadPreview と clearPreview が進め、応答が届いた側は
  // 自分の番号と突き合わせてから本文を差し替える（viewmode.js の reqSeq と同じ形）。
  //
  // Why not `id !== currentFilePath` で判定する: 同じファイルを素早く 2 回開くと
  // どちらの応答も一致してしまい、古い方が新しい読み位置の復元を上書きする。
  // Why not MdCommon.bodyGen() を流用する: あれは hydrate の中で増える事後の
  // カウンタなので、先に着地した古い応答が世代を進めて新しい応答の方が捨てられる。
  var reqSeq = 0;
  // いまペインに入っている本文のファイル。`currentFilePath` は**フェッチを投げた時点**で
  // 切り替わるので、応答が着地するまでの間は 2 つがずれる。読み位置の錨はペインの中身を
  // 測るものなので、このずれている間に錨を読むと「前のファイルの位置」を掴む。
  var bodyPath = null;
  // ポンプを回してよいかのゲート。初期描画中は ?dir= / ?file= に帯域を譲る。
  var initialRenderDone = false;
  // 未送信の {path, row}。サーバに走査を止める手段が無いので、こちら側にできるのは
  // 「まだ投げていないものを投げない」ことだけ。そのための待ち行列。
  var mdCheckQueue = [];
  var mdCheckInflight = 0;
  // 同時に飛ばす本数。投げたぶんは畳んでも取り返せないので小さく、点が出揃う速さは
  // 保ちたいので 1 本（完全直列）にはしない。幅の広い root で点が遅ければ上げ、
  // 畳んだ後も焼けるなら下げる。触るのはこの 1 個だけで済むようにしてある。
  var MD_CHECK_LIMIT = 3;
  var sidebarOpen = true;

  // 畳んだ祖先の下にいる行。offsetParent を見ないのは、(1) ポンプのループの中で
  // 毎回レイアウトを強制するため、(2) ⌘B でサイドバーごと畳んだ時も「隠れている」と
  // 答えてしまうため。サイドバーは開き直した瞬間に点が要るので、それは別の問い。
  function isRowHidden(row) {
    return !!row.closest('.tree-children:not(.open)');
  }

  // 判定結果は行が持つ（data-md-dot: pending / yes / no / unknown）。パス→結果の
  // 辞書を別に置かないのは、1 パスにつき行は 1 個しか存在しない（loaded フラグが
  // 同じ dir の再描画を防ぐ）ので、辞書が同じ事実の二重帳簿になるため。
  function setMdDot(row, state) {
    row.dataset.mdDot = state;
    if (state === 'yes') {
      row.classList.add('has-md');
    }
  }

  function sendHasMdCheck(path, row) {
    // 加算は fetch を呼んだ後。先に増やすと、fetch が同期的に throw した時に
    // 減らす者が居なくなり、上限が恒久的に目減りする。
    var pending = fetch('/?has_md=' + encodeURIComponent(path));
    mdCheckInflight++;
    pending
      .then(function(r) { return r.ok ? r.json() : null; })
      .then(function(data) {
        // 値は文字列。"no" は truthy なので !! や if で判定してはいけない。
        var v = data && data.has_md;
        // 予算切れ(unknown)・非 200・壊れた応答は確定させない。確定させると点が
        // 二度と出ない（再判定の契機は「畳んで開き直す」だけなので、それを潰す）。
        setMdDot(row, v === 'yes' ? 'yes' : v === 'no' ? 'no' : 'unknown');
      })
      .catch(function() { setMdDot(row, 'unknown'); })
      .then(function() {
        mdCheckInflight--;
        pumpMdChecks();
      });
  }

  function pumpMdChecks() {
    if (!initialRenderDone) return;
    while (mdCheckInflight < MD_CHECK_LIMIT && mdCheckQueue.length) {
      // 先入れ先出し。後入れ先出しにすると「最後に展開した場所を優先できる」が、
      // 起動時は root の全行が積まれてから動き出すので、文書順の逆＝画面の下から
      // ドットが埋まる。一番目に付く先頭行が最後に点くほうが損。畳んだ行は送信前に
      // 捨てるので、古い積み残しが新しい展開を待たせ続けることもない。
      var next = mdCheckQueue.shift();
      if (next.row.dataset.mdDot !== 'pending') continue;
      if (isRowHidden(next.row)) {
        delete next.row.dataset.mdDot;
        continue;
      }
      sendHasMdCheck(next.path, next.row);
    }
  }

  function scheduleHasMdCheck(path, row) {
    if (row.dataset.mdDot === 'pending') return;
    row.dataset.mdDot = 'pending';
    mdCheckQueue.push({path: path, row: row});
    pumpMdChecks();
  }

  // 再び見えるようになった dir 行のうち、判定が確定していないものを積み直す。
  // 自動では再試行しない（unknown が返るのは最も重い枝なので、タイマーやホバーで
  // 再走査すると消したはずの CPU 焼きが戻る）。畳んで開き直すのは関心の表明なので、
  // 再走査の対価はそこで払う。
  function recheckDots(container) {
    var rows = container.querySelectorAll('.tree-item[data-kind="dir"]');
    for (var i = 0; i < rows.length; i++) {
      var state = rows[i].dataset.mdDot;
      if (state === 'yes' || state === 'no' || state === 'pending') continue;
      if (isRowHidden(rows[i])) continue;
      scheduleHasMdCheck(rows[i].dataset.path, rows[i]);
    }
  }

  function renderItems(items, parentEl, depth) {
    items.forEach(function(item) {
      var row = document.createElement('div');
      row.className = 'tree-item';
      row.style.paddingLeft = (8 + depth * 16) + 'px';
      // ツリー項目の右クリック（contextmenu.js）でパスを引けるよう保持しておく。
      row.dataset.path = item.path;
      row.dataset.kind = item.kind;

      var icon = document.createElement('span');
      icon.className = 'icon';

      if (item.kind === 'dir') {
        icon.textContent = '›';
        var children = document.createElement('div');
        children.className = 'tree-children';
        var loaded = false;

        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);
        parentEl.appendChild(children);

        scheduleHasMdCheck(item.path, row);

        // 子要素を読み込んで展開する。子の描画完了で解決する Promise を返す。
        function expand() {
          var wasOpen = children.classList.contains('open');
          children.classList.add('open');
          row.classList.add('dir-open');
          // 畳んでいる間に捨てた判定と、予算切れで不明だったものをここで拾い直す。
          // 開いているものを開き直した時にやらないのは、revealFile が祖先の _expand を
          // 開閉に関係なく呼ぶから。そこで積み直すと、ファイルを開くたびに（タブ切替も
          // ⌘P も）一番重い枝の再走査が走り、起動時のバーストをクリックごとのバースト
          // に置き換えることになる。
          if (!wasOpen) recheckDots(children);
          if (loaded) return Promise.resolve();
          loaded = true;
          return fetch('/?dir=' + encodeURIComponent(item.path))
            .then(function(r) { return r.json(); })
            .then(function(subItems) { renderItems(subItems, children, depth + 1); })
            .catch(function() {});
        }
        // revealFile から祖先フォルダをプログラム的に開けるよう保持しておく。
        row._expand = expand;

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          if (children.classList.contains('open')) {
            children.classList.remove('open');
            row.classList.remove('dir-open');
          } else {
            expand();
          }
        });
      } else {
        if (isRenderablePath(item.name)) {
          row.classList.add('md-file');
        }
        icon.textContent = '';
        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          loadPreview(item.path);
        });
      }
    });
  }

  // パスと種別に一致するツリー行を、描画済みの中から探す。
  function findRow(path, kind) {
    var rows = document.querySelectorAll('.tree-item');
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].dataset.path === path &&
          (!kind || rows[i].dataset.kind === kind)) {
        return rows[i];
      }
    }
    return null;
  }

  // 開いているファイルに対応するツリー項目をハイライトし、見える位置へスクロールする。
  function updateActiveItem(id) {
    // 非同期な reveal の完走中に別ファイルへ切り替わっていたら、現在の
    // ハイライトを壊さないよう何もしない。
    if (id !== currentFilePath) return;
    document.querySelectorAll('.tree-item.active').forEach(function(el) {
      el.classList.remove('active');
    });
    var row = findRow(id, 'file');
    if (row) {
      row.classList.add('active');
      row.scrollIntoView({ block: 'nearest' });
    }
  }

  // ファイルまで祖先フォルダを順に展開し、最後にハイライトする。
  // root の外のファイルはツリーに行が無いので、選択を外すだけにする
  // （居場所はタブが示す）。
  function revealFile(id) {
    if (MdCommon.isOutsideRoot(id)) { updateActiveItem(id); return; }
    // 祖先はツリー行と同じ識別子（絶対パス）で組む。root 自身はツリーに行が無い
    // （サイドバーそのもの）ので含めない。
    var base = MdCommon.rootPrefix();
    var segs = MdCommon.idToDisplay(id).split('/');
    var ancestors = [];
    for (var i = 0; i < segs.length - 1; i++) {
      ancestors.push(base + segs.slice(0, i + 1).join('/'));
    }

    function step(idx) {
      if (idx >= ancestors.length) {
        updateActiveItem(id);
        return;
      }
      var dirRow = findRow(ancestors[idx], 'dir');
      if (!dirRow || !dirRow._expand) {
        // 祖先が見つからなければ諦めて、今ある範囲でハイライトを試みる。
        updateActiveItem(id);
        return;
      }
      Promise.resolve(dirRow._expand()).then(function() { step(idx + 1); });
    }
    step(0);
  }

  // 通常表示がレンダリング結果になるファイル（md / html）。Raw トグルが意味を持つ対象。
  // 拡張子の一覧は Rust 側（request::RENDERABLE_EXT）が定義元で、起動時に
  // window.MD_RENDERABLE_EXT として注入される。ここに書き写さないこと
  // （書き写すと Rust 側だけ直した時に、開けるのに raw が出ないファイルが生まれる）。
  function isRenderablePath(p) {
    var m = /\.([^./\\]+)$/.exec(p || '');
    if (!m) return false;
    var exts = window.MD_RENDERABLE_EXT || [];
    return exts.indexOf(m[1].toLowerCase()) >= 0;
  }

  // iframe(.html-frame) 内の相対リンククリックを親のプレビュー遷移に回す。true を返すと
  // iframe 内遷移を止める。外部/アンカーや、md/html 以外（画像等）は iframe/wry に任せる。
  function frameLinkClick(href) {
    if (/^(https?:|mailto:|#)/i.test(href)) return false;
    var hashIdx = href.indexOf('#');
    var pathPart = hashIdx !== -1 ? href.slice(0, hashIdx) : href;
    if (!isRenderablePath(pathPart)) return false;
    // iframe の中身はサーバが書き換えていない（そのまま配信した html）ので、
    // ここだけは相対パスを自前で解決する。
    var resolved = currentFilePath ? MdCommon.resolvePath(currentFilePath, pathPart)
      : MdCommon.urlToId(pathPart);
    loadPreview(resolved);
    return true;
  }

  // 取得が非200だった時に、無反応にせず理由をペインへ出す。
  // textContent で組むのでファイル名に < 等が入っても安全。
  function showNotice(pane, text) {
    var article = document.createElement('div');
    article.className = 'markdown-body';
    var p = document.createElement('p');
    p.className = 'md-notice';
    p.textContent = text;
    article.appendChild(p);
    pane.innerHTML = '';
    pane.appendChild(article);
    if (window.MdToc) window.MdToc.refresh();
  }

  function showLoadError(pane, id) {
    var name = (id || '').split('/').pop() || id || '';
    showNotice(pane, 'このファイルは開けませんでした: ' + name
      + '（存在しないパス・権限・壊れたファイルなどの可能性）');
  }

  // 開いているファイルが 1 つも無い状態（ツリーだけがある `md .` の起動直後と同じ）
  // へ戻す。loadPreview が「現在のファイル」に紐付けたものを、同じ並びで解く。
  function clearPreview() {
    var pane = document.getElementById('preview-pane');
    currentFilePath = null;
    bodyPath = null;
    // 進行中の本文フェッチを無効にする。これが無いと「すべてのタブを閉じる」の
    // 直後に届いた応答が、空にしたはずのペインへ前のファイルを戻す。
    reqSeq++;
    if (pane) {
      var article = document.createElement('div');
      article.className = 'markdown-body';
      pane.innerHTML = '';
      pane.appendChild(article);
    }
    document.querySelectorAll('.tree-item.active').forEach(function(el) {
      el.classList.remove('active');
    });
    if (window.MdMenu) window.MdMenu.setCurrentFile(null);
    if (window.MdSearch) window.MdSearch.reset();
    // raw / diff は現在のファイルに対する表示なので、対象が無くなったら畳む。
    // restore(null) は状態だけを OFF にする（本文は上で空にした）。
    if (window.MdViewModes) window.MdViewModes.restore(null);
    if (window.MdRaw) window.MdRaw.setAvailable(false);
    if (window.MdDiff) window.MdDiff.refreshStat();
    if (window.MdToc) window.MdToc.refresh();
    focusPreview();
  }

  function loadPreview(id, preserveScroll) {
    var pane = document.getElementById('preview-pane');
    // タブ（tabs.js）はこの関数を唯一の入口として状態を持つ。ホットリロードは
    // ファイル切替ではないので通さない（タブが増えたり読み位置が動いたりしない）。
    if (!preserveScroll && window.MdTabs) MdTabs.onOpen(id);
    // 一度開いたタブへ戻る時は、そのタブに残した読み位置から再開する。
    var savedScroll = preserveScroll ? MdCommon.readScroll()
      : (window.MdTabs ? MdTabs.scrollFor(id) : 0);
    currentFilePath = id;
    // root の外のファイルは root の再帰監視に載らないので、個別に監視を頼む。
    // 頼まないとホットリロードだけが効かない（開けるのに更新されない）状態になる。
    // 識別子は全部先頭が `/` なので、形ではなく root の内外で判定する。
    if (MdCommon.isOutsideRoot(id) && window.ipc) window.ipc.postMessage('watch:' + id);
    // ファイル切替（ホットリロード以外）では本文ペインへフォーカスを戻し、直後から
    // スクロール素キー(j/k 等)が効くようにする。ホットリロードは現在のフォーカスを保つ。
    if (!preserveScroll) focusPreview();
    // ホットリロード(同一ファイルの再描画)ではツリーを動かさない。
    if (!preserveScroll) revealFile(id);
    if (window.MdMenu) window.MdMenu.setCurrentFile(id);
    if (window.MdSearch) window.MdSearch.reset();
    // バッジ（変更行数）は表示状態に関わらず、開いているファイルに追従させる。
    if (window.MdDiff) window.MdDiff.refreshStat();
    // md / html は通常表示がレンダリング結果なので Raw（ソース）トグルを有効化する。
    // それ以外は通常表示が既にソースなので raw は無効化（トグルを隠す）。raw 表示中に
    // 無効ファイルへ切り替えたら setAvailable(false) が状態を畳むので通常フェッチに落ちる。
    if (window.MdRaw) window.MdRaw.setAvailable(isRenderablePath(id));

    // raw / diff はモードとして維持する。ON のまま別ファイルへ移ったら、そのファイルの
    // ソース / 差分を表示する（本文レンダリングには戻さない）。
    var mode = window.MdViewModes && window.MdViewModes.active();
    if (mode) {
      // ファイル切替（preserveScroll=false）はタブの読み位置へ着地させ、
      // ホットリロードは現在位置を維持する（refresh の引数を省くとそうなる）。
      // ここでペインへ直に代入してはいけない。中身はまだ前のファイルなので、
      // 前が短いと clamp されて読み位置が 0 に落ちる。
      mode.refresh(preserveScroll ? undefined : savedScroll);
      return;
    }

    // 同じファイルを出し直す時（ホットリロード・raw / diff からの復帰）は、読み位置を
    // 行（data-src-line）で持ち回る。raw から戻る経路ではソースとレンダリング結果で
    // 高さが違い、ピクセルの scrollTop をそのまま入れると数十行ぶん飛ぶ。
    // ファイル切替はタブが同じ表示で記録したピクセルなので錨らない。
    //
    // モードの分岐より後で読む。モードが出ているなら show() が自分で錨を読むので、
    // ここで読むと全ユニットの実測が丸ごと無駄になる（ホットリロードの度に走る）。
    var anchor = preserveScroll ? MdCommon.readAnchor() : null;

    var myReq = ++reqSeq;
    fetch('/?file=' + encodeURIComponent(id), preserveScroll ? { cache: 'no-store' } : undefined)
      .then(function(r) { return r.ok ? r.text() : null; })
      .then(function(html) {
        // 応答が届くまでに別のファイルへ移っていた／本文を空にしていたら捨てる。
        // モードの ON は loadPreview を通らず reqSeq を進めないので、別に見る。
        if (myReq !== reqSeq) return;
        if (window.MdViewModes && MdViewModes.active()) return;
        // 非200(html==null)は握りつぶさず理由を表示する。サーバは id_to_path が
        // 解決できない時（消えたファイル・権限）に not_found を返すので、
        // 黙って無反応にならないようメッセージを出す。
        if (html == null) { showLoadError(pane, id); return; }
        pane.innerHTML = html;
        bodyPath = id;
        // html は iframe の中がスクロール主体なので、ここでは預けるだけになる
        // （実際に戻すのは中身の load 後、common.js の bindFrame）。錨が効く md では
        // この代入は下の restoreAnchor が上書きする（錨れなかった時の受け皿）。
        MdCommon.restoreScroll(savedScroll);
        // html 表示（iframe）の相対リンクは、iframe 内遷移ではなく親のプレビュー遷移に
        // 回す（サイドバーの選択やコメントの現在ファイルを同期させるため）。
        MdCommon.hydrate(pane, { onLinkClick: frameLinkClick });
        // 読み位置は hydrate の後に入れ直す。行の包み直し・コードハイライト・埋め込み
        // カードで高さが変わるので、上の代入した値はもう合っていない。錨が使えるなら
        // そちらを優先し（行なら表示をまたげる）、無ければピクセルで入れ直す
        // （タブへ戻る経路。同じ表示なので丸めずに exact へ戻せる）。
        if (!MdCommon.restoreAnchor(anchor)) MdCommon.holdScroll(savedScroll);
      })
      // then と同じ 2 つを見る。モードの ON は reqSeq を進めないので、世代だけ見ると
      // 「raw は出ているのに本文だけエラー表示」という食い違った画面になる。
      // （`viewmode.js` の catch は世代しか見ていない。倣った先の方がずれている）
      .catch(function() {
        if (myReq !== reqSeq) return;
        if (window.MdViewModes && MdViewModes.active()) return;
        showLoadError(pane, id);
      });
  }

  // ファイル監視（main.rs）から呼ばれる唯一の入口。引数は変更されたファイルの識別子。
  window.MdReload = function(id) {
    if (!currentFilePath) return;
    if (id !== currentFilePath) return;
    // raw / diff 表示中はファイル変更をその再取得に回す（本文には戻さない）。
    var mode = window.MdViewModes && window.MdViewModes.active();
    if (mode) { mode.refresh(); return; }
    // ペインの中身がまだ前のファイルなら、ここで再読込してはいけない。
    // preserveScroll の経路は「いま見えている位置」を錨として持ち回るので、中身が
    // 追いついていないと**前のファイルの読み位置に新しいファイルを着地させる**。
    // 飛ばしたぶんは、進行中のフェッチが持ってくるか、次の保存で拾う。
    if (bodyPath !== currentFilePath) return;
    loadPreview(currentFilePath, true);
  };

  // ── キーボードナビ ────────────────────────────────────────────
  // ・[ / ] : 表示中の描画可能ファイルを巡回して即プレビュー
  // ・Tab   : 本文ペイン ⇄ ファイルツリー のフォーカス切替
  // ・ツリーにフォーカス時: j/k 移動・g/G 端・Enter/l 開く&展開・h 畳む/親へ
  // keyscroll.js とは MdCommon.isSidebarFocused() で排他する（役割の二重発火を防ぐ）。
  var cursorRow = null;

  // 画面に見えている .tree-item（畳んだフォルダ内の隠れ行は offsetParent===null で除外）。
  function visibleRows() {
    return Array.prototype.filter.call(
      document.querySelectorAll('.tree-item'),
      function(r) { return r.offsetParent !== null; }
    );
  }
  // 見えている描画可能ファイル行（[ / ] の巡回対象）。
  function visibleFileRows() {
    return Array.prototype.filter.call(
      document.querySelectorAll('.tree-item.md-file'),
      function(r) { return r.offsetParent !== null; }
    );
  }

  function setCursor(row) {
    if (cursorRow) cursorRow.classList.remove('cursor');
    cursorRow = row || null;
    if (cursorRow) {
      cursorRow.classList.add('cursor');
      cursorRow.scrollIntoView({ block: 'nearest' });
    }
  }

  function focusPreview() {
    var pane = document.getElementById('preview-pane');
    // 検索入力などにフォーカス中は奪わない（本文ロード時は search.reset で閉じるので通常は安全）。
    if (window.MdCommon && MdCommon.isFieldEl(document.activeElement)) {
      document.body.classList.remove('nav-tree');
      return;
    }
    if (pane) pane.focus({ preventScroll: true });
    document.body.classList.remove('nav-tree');
  }

  function focusTree() {
    var sb = document.getElementById('sidebar');
    // 畳んでいる間は見えない行にカーソルを送らない（呼ぶ側が先に開くこと）。
    if (!sb || !sidebarOpen) return;
    sb.focus({ preventScroll: true });
    document.body.classList.add('nav-tree');
    // カーソルが未設定/不可視なら、開いているファイル→先頭可視行の順で置く。
    if (!cursorRow || cursorRow.offsetParent === null) {
      var active = document.querySelector('.tree-item.active');
      var rows = visibleRows();
      setCursor((active && active.offsetParent !== null) ? active : (rows[0] || null));
    } else {
      setCursor(cursorRow); // 見える位置へ再スクロール
    }
  }

  // ── サイドバーの開閉（⌘B / 右クリックメニュー / 閉じている時のリサイザ） ────
  // 幅は CSS 変数 --md-sidebar-w が持つので、ここはクラスの付け外しだけでよい。
  // 状態はセッション内のみ（このアプリはサイドバー幅も含め永続化していない）。
  function setSidebarOpen(open) {
    open = !!open;
    if (open === sidebarOpen) return;
    sidebarOpen = open;
    document.body.classList.toggle('sidebar-closed', !open);
    // 閉じた瞬間にツリーへフォーカスが残ると、見えない行にカーソルが居座って
    // j/k がツリー操作のままになる。本文へ返して nav-tree も畳む。
    if (!open && window.MdCommon && MdCommon.isSidebarFocused()) focusPreview();
    // 本文ペインの幅が変わるので、TOC の自動退避/復帰を評価し直す。
    // width の transition が終わってからでないと availWidth() が古い幅を見る。
    setTimeout(function() {
      if (window.MdToc) window.MdToc.reevaluate();
    }, 200);
  }

  function toggleSidebar() { setSidebarOpen(!sidebarOpen); }

  // 右クリックメニューがラベルの出し分け（隠す / 表示）に使う。
  window.MdSidebar = {
    isOpen: function() { return sidebarOpen; },
    toggle: toggleSidebar,
    open: function() { setSidebarOpen(true); },
    close: function() { setSidebarOpen(false); }
  };

  function moveCursor(delta) {
    var rows = visibleRows();
    if (!rows.length) return;
    var i = cursorRow ? rows.indexOf(cursorRow) : -1;
    if (i === -1) { setCursor(rows[delta < 0 ? rows.length - 1 : 0]); return; }
    setCursor(rows[Math.max(0, Math.min(rows.length - 1, i + delta))]);
  }
  function cursorEdge(toEnd) {
    var rows = visibleRows();
    if (rows.length) setCursor(rows[toEnd ? rows.length - 1 : 0]);
  }

  // カーソル行を含む .tree-children の直前にある親ディレクトリ行。無ければ null。
  function parentDirRow(row) {
    var container = row && row.parentNode;
    if (container && container.classList && container.classList.contains('tree-children')) {
      var prev = container.previousSibling;
      if (prev && prev.classList && prev.classList.contains('tree-item')) return prev;
    }
    return null;
  }

  function openCursorFile() {
    if (cursorRow && cursorRow.dataset.path) {
      loadPreview(cursorRow.dataset.path); // loadPreview 内で focusPreview 済み
    }
  }

  // l / → : dir=展開（開いていれば最初の子へ）/ file=開く
  function expandOrOpen() {
    if (!cursorRow) return;
    if (cursorRow.dataset.kind === 'dir') {
      if (!cursorRow.classList.contains('dir-open')) {
        // expand() を直接呼ばないのは、閉じる側の処理とキーの往復をクリック
        // ハンドラ 1 箇所に集めておくため。
        cursorRow.click();
      } else {
        var children = cursorRow.nextSibling;
        var first = (children && children.querySelector) ? children.querySelector('.tree-item') : null;
        if (first && first.offsetParent !== null) setCursor(first);
      }
    } else {
      openCursorFile();
    }
  }
  // Enter : dir=開閉トグル / file=開く
  function toggleOrOpen() {
    if (!cursorRow) return;
    if (cursorRow.dataset.kind === 'dir') cursorRow.click();
    else openCursorFile();
  }
  // h / ← : 開いた dir=畳む（カーソルはその dir に残る）/ それ以外=親 dir へ
  function collapseOrParent() {
    if (!cursorRow) return;
    if (cursorRow.dataset.kind === 'dir' && cursorRow.classList.contains('dir-open')) {
      cursorRow.click();
    } else {
      var parent = parentDirRow(cursorRow);
      if (parent) setCursor(parent);
    }
  }

  // [ / ] : 表示中の描画可能ファイルを DOM 順に巡回。端ではクランプ（wrap しない）。
  function gotoAdjacentFile(delta) {
    var files = visibleFileRows();
    if (!files.length) return;
    var cur = document.querySelector('.tree-item.active');
    var i = (cur && cur.offsetParent !== null) ? files.indexOf(cur) : -1;
    var ni;
    if (i === -1) {
      // 現在ファイルが一覧に無い（相対リンク先など）→ 端から入る。
      ni = delta < 0 ? files.length - 1 : 0;
    } else {
      ni = i + delta;
      if (ni < 0 || ni >= files.length) return; // 端で無反応（誤爆時の被害を抑える）
    }
    var row = files[ni];
    if (!row || !row.dataset.path) return;
    // ツリーで巡回中に [ / ] を押した時はツリーに留まりカーソルを新ファイルへ追従させる
    // （[ / ] は「開いて本文へ移動」ではなく「巡回」なので、フォーカス文脈を保つ）。
    var wasTree = !!(window.MdCommon && MdCommon.isSidebarFocused());
    loadPreview(row.dataset.path); // 内部で focusPreview（本文へ移動）
    if (wasTree) { setCursor(row); focusTree(); }
  }

  // キーの割り当て・効く文脈は keymap.js の表が持つ。ここは実処理だけ。
  //
  // 注: このファイルは初期化スクリプト（document-start）として注入されるので、
  // <head> の各モジュールより **先に** 評価される。MdKeymap はまだ存在しないため、
  // 登録は DOMContentLoaded まで遅らせる。
  function registerKeys() {
    if (!window.MdKeymap) return;
    // Tab / [ / ] はツリー内外どちらでも効くアプリ全体のナビ。
    MdKeymap.on('focus-toggle', function() {
      if (window.MdCommon && MdCommon.isSidebarFocused()) { focusPreview(); return; }
      // 畳んである時の Tab は「開いてからツリーへ」。Tab はツリーへ行く操作なので、
      // 閉じているという理由で無反応にするより開いてしまう方が意図に合う。
      setSidebarOpen(true);
      focusTree();
    });
    MdKeymap.on('sidebar-toggle', toggleSidebar);
    MdKeymap.on('file-cycle', function(e) {
      gotoAdjacentFile(e.key === '[' ? -1 : 1);
    });
    // ツリーにフォーカスがある時だけ呼ばれる（keymap.js 側の when が保証する）。
    MdKeymap.on('tree', function(e) {
      switch (e.key) {
        case 'j': case 'ArrowDown':  moveCursor(1); break;
        case 'k': case 'ArrowUp':    moveCursor(-1); break;
        case 'g':                    cursorEdge(false); break;
        case 'G':                    cursorEdge(true); break;
        case 'l': case 'ArrowRight': expandOrOpen(); break;
        case 'h': case 'ArrowLeft':  collapseOrParent(); break;
        case 'Enter':                toggleOrOpen(); break;
        default: break;
      }
    });
  }

  // フォーカスがサイドバー外へ出たら、サイドバーのアクティブ枠(nav-tree)を畳む。
  // 本文クリックやトグル操作でツリーから抜けた時に、枠が残って主役表示が嘘になるのを防ぐ。
  document.addEventListener('focusout', function() {
    setTimeout(function() {
      if (window.MdCommon && !MdCommon.isSidebarFocused()) {
        document.body.classList.remove('nav-tree');
      }
    }, 0);
  });

  document.addEventListener('DOMContentLoaded', function() {
    registerKeys();
    var resizer = document.getElementById('resizer');
    var sidebar = document.getElementById('sidebar');
    var isDragging = false;
    var startX, startWidth;
    resizer.addEventListener('mousedown', function(e) {
      // 畳んである時のリサイザはドラッグ用の取っ手ではなく、開くためのボタン。
      if (!sidebarOpen) { setSidebarOpen(true); e.preventDefault(); return; }
      isDragging = true;
      startX = e.clientX;
      startWidth = sidebar.offsetWidth;
      resizer.classList.add('dragging');
      document.body.classList.add('sidebar-resizing');
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      e.preventDefault();
    });
    document.addEventListener('mousemove', function(e) {
      if (!isDragging) return;
      var newWidth = startWidth + (e.clientX - startX);
      newWidth = Math.max(120, Math.min(newWidth, window.innerWidth - 200));
      // 幅は CSS 変数で持つ（開閉が幅の退避/復元なしに成り立つのはこのため）。
      document.documentElement.style.setProperty('--md-sidebar-w', newWidth + 'px');
    });
    document.addEventListener('mouseup', function() {
      if (!isDragging) return;
      isDragging = false;
      resizer.classList.remove('dragging');
      document.body.classList.remove('sidebar-resizing');
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      // ツリーを広げて preview-pane が閾値を割ったら TOC を退避、
      // 戻したら復帰させる（window resize を経由しない幅変化のため）。
      if (window.MdToc) window.MdToc.reevaluate();
    });

    if (window.MdSearch) {
      window.MdSearch.init(document.getElementById('preview-pane'));
    }
    if (window.MdToc) {
      window.MdToc.init(document.getElementById('preview-pane'));
    }
    if (window.MdTabs) {
      // タブ切替の実体は通常のファイル切替と同じ経路（loadPreview）。
      window.MdTabs.init({
        openFile: function(id) { loadPreview(id); },
        clearFile: clearPreview
      });
    }
    if (window.MdPalette) {
      // ファイル検索（⌘P）。選んだら通常のファイル切替と同じ経路で開く。
      window.MdPalette.init({ openFile: function(id) { loadPreview(id); } });
    }
    if (window.MdViewModes) {
      var previewPane = function() { return document.getElementById('preview-pane'); };
      window.MdViewModes.initAll({
        getContainer: previewPane,
        getScroller: previewPane,
        // 対象ファイルはクエリに載せて渡す（サーバは「開いているファイル」を持たない）。
        // `mode` は raw / diff のどちらか＝クエリのキー。値が識別子。
        url: function(mode) {
          return currentFilePath ? '/?' + mode + '=' + encodeURIComponent(currentFilePath) : null;
        },
        getStatUrl: function() {
          return currentFilePath ? '/?diffstat=' + encodeURIComponent(currentFilePath) : null;
        },
        reloadNormal: function() { if (currentFilePath) loadPreview(currentFilePath, true); }
      });
    }
    if (window.MdComment) {
      // 対象は #preview-pane。file 部は現在プレビュー中ファイルの識別子（絶対パス）。
      // openFile はパネル項目クリックで別ファイルのコメント先へ飛ぶために使う。
      window.MdComment.init({
        getContainer: function() { return document.getElementById('preview-pane'); },
        getFile: function() { return currentFilePath || ''; },
        openFile: function(id) { loadPreview(id); }
      });
    }

    // 初期描画が済んだ印。窓を出す合図ではない（窓は中身を待たず Rust が先に出す）。
    // UI テストがこの印を待つので、消すなら tests/ui/helpers.js も一緒に直すこと。
    function markInitialRenderDone() {
      initialRenderDone = true;
      document.documentElement.dataset.mdReady = '1';
      // 成功・失敗どちらの経路から来てもここで待ち行列が動き出す。
      pumpMdChecks();
    }

    // 最初のツリーも root の識別子で聞く。`?dir=` に載るのは常に識別子、という
    // 契約を 1 本にするため（空文字を「root の意味」にすると経路が 2 つになる）。
    //
    // 取得の失敗だけをここで畳んで null にする。catch を後ろに置くと、木が取れた
    // 後に初期タブの描画で throw したときまで拾ってしまい、「読めているのに
    // 読めなかった」と言いながら描いた本文を消すことになる。
    fetch('/?dir=' + encodeURIComponent(MdCommon.rootDir()))
      .then(function(r) {
        // 空文字が「root の意味」だった頃と違い、このリクエストは 404 しうる
        // （`resolve_tree_dir` が識別子として解決できなければ落とす）。
        if (!r.ok) throw new Error('tree ' + r.status);
        return r.json();
      })
      .catch(function(e) {
        // 無音で終わらせない。窓はデタッチすると stderr が /dev/null へ行くので、
        // 画面に出しておかないと「なぜか真っ白」しか手掛かりが残らない。
        showNotice(document.getElementById('preview-pane'),
          'フォルダを読み込めませんでした: ' + (MdCommon.rootDir() || '(未設定)'));
        if (window.console) console.error('ツリーを取得できませんでした', e);
        return null;
      })
      .then(function(items) {
        if (items === null) {
          setTimeout(function() { markInitialRenderDone(); }, 0);
          return;
        }
        renderItems(items, sidebar, 0);
        // 起動時に開くファイル（`md a.md b.md` なら 2 枚のタブ。先頭が最初に見える）。
        var initial = (typeof INITIAL_FILES !== 'undefined' && INITIAL_FILES) || [];
        if (initial.length && window.MdTabs) {
          window.MdTabs.openMany(initial); // 内部で loadPreview → focusPreview 済み
        } else if (initial.length) {
          loadPreview(initial[0]); // 内部で focusPreview 済み
        } else {
          // ファイル未指定でも本文ペインにフォーカスを置き、スクロール素キーの初期ターゲットにする。
          focusPreview();
        }
        setTimeout(function() {
          markInitialRenderDone();
          // ファイル検索の一覧を先に温めておく（初回の ⌘P を待たせない）。
          // 初期表示より後に投げるので、起動の体感速度は落とさない。
          if (window.MdPalette) window.MdPalette.prefetch();
        }, 0);
      });
  });

  // ⌘A は common.js が、⌘W は tabs.js が keymap 経由で処理する。
  document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href) return;
    if (MdCommon.scrollToAnchor(href, e)) {
      // ページ内アンカーは処理済み。
    } else if (!href.startsWith('http://') && !href.startsWith('https://') && !href.startsWith('mailto:')) {
      var hashIdx = href.indexOf('#');
      var pathPart = hashIdx !== -1 ? href.slice(0, hashIdx) : href;
      var anchorPart = hashIdx !== -1 ? href.slice(hashIdx + 1) : '';
      // md / html はプレビュー枠内で遷移させる。html を top-level 遷移させると、text/html
      // 配信になった今はウィンドウ全体が生ページに化けてサイドバー・トグルが消えてしまう。
      if (isRenderablePath(pathPart)) {
        e.preventDefault();
        // 本文の href はサーバが既に「開いているファイルの場所」基準で URL へ畳んで
        // いる（相対解決をここでやり直すと二重解決になる）。識別子へ戻すだけ。
        var resolved = MdCommon.urlToId(pathPart);
        loadPreview(resolved);
        if (anchorPart) {
          setTimeout(function() {
            var id = decodeURIComponent(anchorPart);
            var target = document.getElementById(id);
            if (target) target.scrollIntoView({ behavior: 'smooth' });
          }, 100);
        }
      }
    }
  });
})();
