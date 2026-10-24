// 本文がアプリの部品に化けられないこと（common.js の regions / appHtmlFrames、folder.js の
// ツリーのキー操作、選択即コピーの写し）。
//
// md の本文はアプリの UI と同じ document にあるので、ツリーの行や `.html` のプレビューの枠と
// 同じ id・クラスの要素を書ける。そうした要素を置いても、キー操作・枠の配線・コピーが
// アプリの作った要素だけを相手にすること。
// 本文は応答を差し替えて作る（フィクスチャを足すとツリーの並びを見ている spec に響く）。
const { test, expect, openFolder, treeItem } = require('./helpers');

/// a.md の `?file=` の応答を、`inner` を本文にしたものへ差し替える。
async function fakeBody(page, inner) {
  await page.route((url) => /[?&]file=[^&]*a\.md$/.test(url.search), (route) => route.fulfill({
    contentType: 'text/html; charset=utf-8',
    body: '<div class="markdown-body"><h1>見出し A</h1>' + inner + '<p id="tail">TAIL</p></div>',
  }));
}

/// 本文に置くツリーの行もどき。`#sidebar` を名乗る箱と、裸の行の両方。
const FAKE_ROWS =
  '<div id="sidebar"><div class="tree-item md-file" data-path="/etc/fake-in-sidebar.md" data-kind="file">行もどき 1</div></div>' +
  '<div class="tree-item md-file" data-path="/etc/fake-bare.md" data-kind="file">行もどき 2</div>' +
  '<div class="tree-item qa-item md-file" data-path="/etc/fake-qa.md" data-kind="file">行もどき 3</div>';

test.describe('ツリーのキー操作は、本文の行もどきに乗らない', () => {
  test('j でツリーの末尾から先へ進んでも、カーソルはサイドバーの中に留まり、Enter と x は偽のパスを使わない', async ({ page }) => {
    await fakeBody(page, FAKE_ROWS);
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();

    await page.keyboard.press('Tab');
    await expect(page.locator('body')).toHaveClass(/nav-tree/);
    // ツリーと Quick Access の行の数より多く押す。
    const rows = await page.locator('#sidebar-col .tree-item').count();
    for (let i = 0; i < rows + 5; i++) await page.keyboard.press('j');

    const cursor = await page.evaluate(() => {
      const c = document.querySelectorAll('.tree-item.cursor');
      return Array.prototype.map.call(c, (el) => ({
        path: el.dataset.path,
        inSidebar: document.getElementById('sidebar-col').contains(el),
      }));
    });
    expect(cursor).toHaveLength(1);
    expect(cursor[0].inSidebar).toBe(true);

    await page.evaluate(() => { window.__mdIpc.length = 0; });
    await page.keyboard.press('x');
    await page.keyboard.press('Enter');
    const ipc = await page.evaluate(() => window.__mdIpc.slice());
    expect(ipc.filter((m) => m.includes('/etc/fake'))).toEqual([]);
    await expect(page.locator('.md-tab[data-path^="/etc/fake"]')).toHaveCount(0);
  });

  test('] の巡回は、本文の `#sidebar` の中の行へ進まない', async ({ page }) => {
    await fakeBody(page, FAKE_ROWS);
    await openFolder(page);
    // ツリーで最後に見えている描画可能なファイルを開き、その先へ ] を押す。
    const last = await page.evaluate(() => {
      const rows = Array.prototype.filter.call(
        document.querySelectorAll('#sidebar-col #sidebar .tree-item.md-file'),
        (r) => r.offsetParent !== null);
      return rows[rows.length - 1].dataset.path;
    });
    // a.md の本文だけを差し替えているので、最後のファイルを開いた後で a.md を開いて本文を入れ、
    // 最後のファイルへ戻ってから押す（行もどきが DOM に居ない間は確かめにならない）。
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();
    // 本文を残したまま、ツリーの帯だけを最後のファイルへ移す。
    await page.evaluate((p) => {
      document.querySelectorAll('#sidebar-col .tree-item.active').forEach((el) => el.classList.remove('active'));
      const row = Array.prototype.find.call(document.querySelectorAll('#sidebar-col .tree-item'), (r) => r.dataset.path === p);
      row.classList.add('active');
    }, last);

    await page.keyboard.press(']');
    await expect(page.locator('.md-tab[data-path^="/etc/fake"]')).toHaveCount(0);
    expect(await page.evaluate(() => {
      const a = document.querySelector('.md-tab.active');
      return a && a.dataset.path;
    })).not.toContain('/etc/fake');
  });

  test('本文の `id="sidebar"` にフォーカスがあっても、ツリーにフォーカスがあるとはみなさない', async ({ page }) => {
    await fakeBody(page, '<div id="sidebar" tabindex="0"><span>箱もどき</span></div>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();
    await page.locator('#preview-pane #sidebar').focus();
    expect(await page.evaluate(() => MdCommon.isSidebarFocused())).toBe(false);
  });
});

test.describe('`.html` のプレビューの枠は、サーバが作ったものだけを相手にする', () => {
  test('md の本文に書いた `iframe.html-frame` は配線せず、読み位置もスクロールも本文ペインのまま', async ({ page }) => {
    await fakeBody(page,
      '<iframe class="html-frame" id="fake-frame-1" src="page.html"></iframe>' +
      '<div class="markdown-body html-page"><iframe class="html-frame" id="fake-frame-2" src="page.html"></iframe></div>' +
      '<div style="height:3000px"></div>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeAttached();

    const state = await page.evaluate(() => ({
      wired: Array.prototype.map.call(document.querySelectorAll('#preview-pane iframe.html-frame'), (f) => !!f.__mdWired),
      htmlFrame: MdCommon.htmlFrame(),
    }));
    expect(state.wired).toEqual([false, false]);
    expect(state.htmlFrame).toBeNull();

    // j は本文ペインをスクロールする（もどきの枠の中ではなく）。
    const top = () => page.evaluate(() => document.getElementById('preview-pane').scrollTop);
    expect(await top()).toBe(0);
    await page.keyboard.press('j');
    await expect.poll(top).toBeGreaterThan(0);
  });

  test('本文に余った `</div>` で包みを閉じても、偽の枠はペインの直下に出ず、続きの本文は包みの中に残る', async ({ page }) => {
    await fakeBody(page,
      'まえ</div><div class="markdown-body html-page"><iframe class="html-frame" id="fake-frame-3" src="page.html"></iframe></div>' +
      '<p id="after-break">続きの本文</p><div style="height:3000px"></div>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeAttached();

    const state = await page.evaluate(() => {
      const pane = document.getElementById('preview-pane');
      const wrap = pane.firstElementChild;
      return {
        kids: pane.children.length,
        wrapIsHtmlPage: wrap.classList.contains('html-page'),
        fakeInWrap: wrap.contains(document.getElementById('fake-frame-3')),
        afterInWrap: wrap.contains(document.getElementById('after-break')),
        wired: !!document.getElementById('fake-frame-3').__mdWired,
        htmlFrame: MdCommon.htmlFrame(),
      };
    });
    expect(state).toEqual({ kids: 1, wrapIsHtmlPage: false, fakeInWrap: true, afterInWrap: true, wired: false, htmlFrame: null });

    const top = () => page.evaluate(() => document.getElementById('preview-pane').scrollTop);
    await page.keyboard.press('j');
    await expect.poll(top).toBeGreaterThan(0);
  });

  test('本物の `.html` のプレビューは今までどおり配線される', async ({ page }) => {
    await openFolder(page);
    await treeItem(page, 'page.html').click();
    const frame = page.frameLocator('iframe.html-frame');
    await expect(frame.locator('h1')).toContainText('HTML フィクスチャ');
    const state = await page.evaluate(() => ({
      wired: !!document.querySelector('#preview-pane iframe.html-frame').__mdWired,
      same: MdCommon.htmlFrame() === document.querySelector('#preview-pane iframe.html-frame'),
    }));
    expect(state).toEqual({ wired: true, same: true });
  });
});

test.describe('選択即コピーは、画面に出ていない文字を写さない', () => {
  /// el の中身を丸ごと選び、選択即コピーが作る文字列を返す。
  async function copyOf(page, selector) {
    return page.evaluate((sel) => {
      const el = document.querySelector(sel);
      const r = document.createRange();
      r.selectNodeContents(el);
      const s = getSelection();
      s.removeAllRanges();
      s.addRange(r);
      return { plain: String(s), copied: MdCommon.selectionMarkdown() };
    }, selector);
  }

  test('寸法 0 の文字と本文の箱の外へ追い出した文字は入らず、見えている文字は残る', async ({ page }) => {
    await fakeBody(page,
      '<div id="t1"><p>見える前半 <span style="font-size:0">HIDDEN-ZERO</span>中ほど ' +
      '<span style="position:absolute;left:-99999px">HIDDEN-OFF</span>見える後半</p>' +
      '<p>次の段落 <span style="display:inline-block;width:0;height:0;overflow:hidden">HIDDEN-BOX</span>おわり</p></div>' +
      '<div id="t2"><p>コード <code>x</code> と <span style="font-size:0">HIDDEN-CODE</span>文字</p></div>' +
      '<div id="t3"><p style="height:0">はみ出して見える</p><p style="margin-top:2em">後ろ</p></div>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();

    const t1 = await copyOf(page, '#preview-pane #t1');
    // 素の選択の文字列には入っている（ここを写さないための仕掛け）。
    expect(t1.plain).toContain('HIDDEN-ZERO');
    expect(t1.copied).not.toContain('HIDDEN');
    expect(t1.copied).toContain('見える前半');
    expect(t1.copied).toContain('見える後半');
    expect(t1.copied).toContain('おわり');

    const t2 = await copyOf(page, '#preview-pane #t2');
    expect(t2.copied).toContain('`x`');
    expect(t2.copied).not.toContain('HIDDEN');

    // 高さ 0 でも、はみ出しが見えている段落の文字は残す。
    const t3 = await copyOf(page, '#preview-pane #t3');
    expect(t3.copied).toContain('はみ出して見える');
    expect(t3.copied).toContain('後ろ');
  });

  test('横スクロールの箱の中で、スクロールの先に隠れている文字は写る（横に長い表・ハイライトされた長いコードの行）', async ({ page }) => {
    const long = 'あ'.repeat(300);
    await fakeBody(page,
      '<div id="w1"><table><thead><tr><th>見出し</th><th>右の見出し</th></tr></thead>' +
      '<tbody><tr><td style="white-space:nowrap">' + long + '</td><td style="white-space:nowrap">FAR-RIGHT-CELL</td></tr></tbody></table>' +
      '<p>表の後 <span style="font-size:0">HIDDEN-NEAR-TABLE</span>おわり</p></div>' +
      '<div id="w2"><pre><code class="hljs language-js"><span class="hljs-keyword">const</span> x = <span class="hljs-string">"' + long + '"</span>; ' +
      '<span class="hljs-string">"FAR-RIGHT-CODE"</span>;</code></pre></div>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();

    // どちらも箱の中で横にはみ出していて、右端の文字は本文の箱の外に出ている（確かめの前提）。
    const pre = await page.evaluate(() => {
      const body = document.querySelector('#preview-pane .markdown-body').getBoundingClientRect();
      const out = (el) => el.getBoundingClientRect().left >= body.right;
      const cell = Array.prototype.find.call(document.querySelectorAll('#preview-pane #w1 td'), (td) => td.textContent === 'FAR-RIGHT-CELL');
      const str = Array.prototype.find.call(document.querySelectorAll('#preview-pane #w2 span'), (sp) => sp.textContent.includes('FAR-RIGHT-CODE'));
      return { cell: out(cell), code: out(str) };
    });
    expect(pre).toEqual({ cell: true, code: true });

    const w1 = await copyOf(page, '#preview-pane #w1');
    expect(w1.copied).toContain('FAR-RIGHT-CELL');
    // 見えない文字は外れている（写しを経由した上で残っている、の確かめ）。
    expect(w1.copied).not.toContain('HIDDEN');

    const w2 = await copyOf(page, '#preview-pane #w2');
    expect(w2.copied).toContain('FAR-RIGHT-CODE');
    expect(w2.copied).toContain('```');
  });

  test('見えない箱の中だけを選んだとき（共通祖先が見えない要素）も何も写さない', async ({ page }) => {
    await fakeBody(page, '<p>前 <span id="box" style="font-size:0">HIDDEN-A <b>HIDDEN-B</b> HIDDEN-C</span> 後</p>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();
    const got = await page.evaluate(() => {
      const box = document.querySelector('#preview-pane #box');
      const r = document.createRange();
      r.setStart(box.firstChild, 2);
      r.setEnd(box.lastChild, 3);
      const s = getSelection();
      s.removeAllRanges();
      s.addRange(r);
      return { plain: String(s), copied: MdCommon.selectionMarkdown() };
    });
    expect(got.plain).toContain('HIDDEN-B');
    expect(got.copied).toBe('');
  });

  test('選択即コピーは、見えない文字だけをなぞったときクリップボードに何も入れず、合図も出さない', async ({ page }) => {
    // clipboard を失敗させて IPC 経路に寄せる（中身を観測するのはこの道しかない。autocopy.spec.js と同じ）。
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        get: () => ({ writeText: () => Promise.reject(new Error('stubbed')) })
      });
    });
    await fakeBody(page, '<p id="host">前 <span id="only" style="font-size:0">HIDDEN-ONLY</span> 後</p>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();

    // なぞる操作を、mousedown → 選択 → mouseup の順に組み立てる（見えない文字はマウスでは
    // 狙えないため）。
    const gesture = (page, sel) => page.evaluate((sel) => {
      window.__mdIpc.length = 0;
      const host = document.querySelector('#preview-pane #host');
      getSelection().removeAllRanges();
      host.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
      const r = document.createRange();
      const el = document.querySelector(sel);
      r.setStartBefore(el);
      r.setEndAfter(el);
      getSelection().addRange(r);
      host.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, button: 0, clientX: 10, clientY: 10 }));
    }, sel);
    const copies = () => page.evaluate(() =>
      window.__mdIpc.filter((m) => m.indexOf('copy:') === 0).map((m) => m.slice(5)));

    await gesture(page, '#preview-pane #only');
    // 失敗した clipboard から IPC へ落ちるのは非同期なので、少し待ってから見る。
    await page.waitForTimeout(200);
    expect(await copies()).toEqual([]);
    await expect(page.locator('.md-copy-tick.show')).toHaveCount(0);
    await expect(page.locator('.md-toast.show')).toHaveCount(0);

    // 同じ組み立てで見える文字を選べば入る（上が「何も起きていない」だけではないことの確かめ）。
    await gesture(page, '#preview-pane #host');
    await expect.poll(copies).toHaveLength(1);
    const got = await copies();
    expect(got[0]).toContain('前');
    expect(got[0]).not.toContain('HIDDEN');
  });

  test('見えない文字だけを選んだときは何も写さない', async ({ page }) => {
    await fakeBody(page, '<p>前 <span id="only" style="font-size:0">HIDDEN-ONLY</span> 後</p>');
    await openFolder(page);
    await treeItem(page, 'a.md').click();
    await expect(page.locator('#preview-pane #tail')).toBeVisible();
    const got = await page.evaluate(() => {
      const el = document.querySelector('#preview-pane #only');
      const r = document.createRange();
      r.setStartBefore(el);
      r.setEndAfter(el);
      const s = getSelection();
      s.removeAllRanges();
      s.addRange(r);
      return MdCommon.selectionMarkdown();
    });
    expect(got).toBe('');
  });
});
