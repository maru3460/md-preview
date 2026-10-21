//! 右クリックの「デフォルトアプリで開く」を通してよいかの判定。
//!
//! 開く口は内線（`menu:open:`）なので、設定で文書の JavaScript を動かすと文書の script からも
//! 叩ける。開いた先のアプリが中身を実行するもの（端末・スクリプトエディタ・インストーラ）だと、
//! 文書がファイルを置いて開かせるだけで手元のコマンドが動く。そこで許可制にする。
//!
//! ファイルと担当アプリの事実は ObjC の層（バイナリ側の `platform.rs`）が LaunchServices から
//! 集めて渡し、判定はここで単体テストする（テストを手元の LaunchServices の状態に依存させない）。
//! 規則は [`decide`] の doc に全部書く。

use std::path::Path;

/// 開く対象のファイルの事実。
#[derive(Debug, Clone, Copy)]
pub struct FileFacts<'a> {
    pub kind: FileKind,
    /// 実行の権限（`x` のどれか）が立っているか。
    pub executable: bool,
    /// 中身の型（UTI）と、その型が準拠する型の全部。LaunchServices が型を決められなければ空。
    pub content_types: &'a [&'a str],
    /// 先頭の数 KB がテキストに見えるか（[`looks_like_text`]）。通常ファイルのときだけ見る。
    pub looks_text: bool,
}

/// canonicalize した後の実体の種類。シンボリックリンクはここへ来る前に解けている。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    /// ふつうのフォルダ（パッケージではないもの）。
    Folder,
    /// フォルダに見えて Finder が 1 つのファイルとして扱うもの（`.app`・`.pkg` の一部・`.workflow` など）。
    Package,
    /// Finder のエイリアス。
    Alias,
    /// FIFO・デバイスなど、通常ファイルでもフォルダでもないもの。
    Other,
}

/// 担当アプリの事実。
#[derive(Debug, Clone, Copy)]
pub struct Handler<'a> {
    /// `CFBundleIdentifier`。読めなければ None。
    pub bundle_id: Option<&'a str>,
    /// 画面に出す名前（トーストの文言に使う）。
    pub name: &'a str,
    /// このアプリが実行ファイル（`public.unix-executable`）を開けると名乗っているか。
    pub opens_executables: bool,
}

/// 判定の答え。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// 判定した担当アプリを指定して開く。
    Open,
    /// 開かずに Finder で表示し、理由をトーストで出す。
    Reveal(Refusal),
}

/// 開かなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// 拡張子が実行・移動・インストールにつながる種類。
    BlockedExt,
    /// 拡張子の無い実行ファイル。
    ExecutableNoExt,
    Alias,
    Package,
    /// 通常ファイルでもフォルダでもない。
    NotRegular,
    /// ファイルが無い（メニューを開いてから押すまでの間に消えた・動いた）。Finder では表示しない。
    Missing,
    /// ファイルの情報を読めない（権限など）。
    Unreadable,
    /// 中身の型が許可した種類（テキスト・画像・動画・音声・PDF・オフィス文書）に入らない。
    ContentType,
    /// 担当アプリが無い。
    NoHandler,
    /// 担当アプリが実行ファイルを開ける（端末など）。
    RunsExecutables(String),
    /// 担当アプリが拒否リストにある。
    DeniedHandler(String),
}

impl Refusal {
    /// トーストに出す文言。どれも「開かずに Finder で表示した」ことが分かる形にする。
    /// [`Refusal::Missing`] だけは表示するものが無いので、開けなかったことだけを言う。
    pub fn message(&self) -> String {
        let why = match self {
            Refusal::BlockedExt | Refusal::ExecutableNoExt => "実行されうる種類のファイル".to_string(),
            Refusal::Alias => "エイリアス".to_string(),
            Refusal::Package => "アプリやパッケージ".to_string(),
            Refusal::NotRegular => "通常のファイルではない".to_string(),
            Refusal::Missing => return "開けませんでした（ファイルが見つからない）".to_string(),
            Refusal::Unreadable => "ファイルの情報を読めない".to_string(),
            Refusal::ContentType => "開いてよい種類のファイルではない".to_string(),
            Refusal::NoHandler => "開くアプリが無い".to_string(),
            Refusal::RunsExecutables(app) => format!("{app} はコマンドを実行できるアプリ"),
            Refusal::DeniedHandler(app) => format!("{app} では開かない"),
        };
        format!("Finder で表示しました（{why}）")
    }
}

/// 開いてよいかを決める。次をすべて満たすときだけ [`Decision::Open`]。
///
/// 1. 拡張子がブロックリスト（[`is_blocked_ext`]）に無く、拡張子の無い実行ファイルでもない。
///    下の許可制で塞がるものも含むが、型や担当アプリの読み違いに備える手前の安全網として持つ
/// 2. エイリアスでもパッケージでもない（シンボリックリンクは呼ぶ側が canonicalize で解いてある）
/// 3. 通常ファイルなら、中身の型がテキスト（ソースコードを含む）・画像・動画・音声・PDF・
///    オフィス文書のどれかに準拠する（[`content_allowed`]）。型が `public.data` / `public.item`
///    止まり（`.gitignore`・`.env`・`Dockerfile`・`/etc/hosts` など、拡張子から型が決まらない
///    もの）なら、先頭がテキストに見えるときだけテキストとして通す（[`only_generic_types`]）。
///    フォルダは型を見ない
/// 4. 担当アプリがあり、実行ファイルを開けるアプリではない。端末（Terminal・Ghostty など）は
///    これで一括して弾ける
/// 5. 担当アプリが小さな拒否リスト（[`DENIED_HANDLERS`]）に無い。実行ファイルは開かないが、
///    渡されたものを実行・インストールするアプリ（Script Editor・インストーラ・Automator・
///    Python Launcher など）は 4 では弾けないので、名前で弾く
pub fn decide(path: &Path, file: &FileFacts, handler: Option<&Handler>) -> Decision {
    if is_blocked_ext(path) {
        return Decision::Reveal(Refusal::BlockedExt);
    }
    match file.kind {
        FileKind::Alias => return Decision::Reveal(Refusal::Alias),
        FileKind::Package => return Decision::Reveal(Refusal::Package),
        FileKind::Other => return Decision::Reveal(Refusal::NotRegular),
        FileKind::Folder => {}
        FileKind::Regular => {
            if file.executable && path.extension().is_none() {
                return Decision::Reveal(Refusal::ExecutableNoExt);
            }
            let text_by_content = file.looks_text && only_generic_types(file.content_types);
            if !content_allowed(file.content_types) && !text_by_content {
                return Decision::Reveal(Refusal::ContentType);
            }
        }
    }
    let Some(h) = handler else { return Decision::Reveal(Refusal::NoHandler) };
    if h.opens_executables {
        return Decision::Reveal(Refusal::RunsExecutables(h.name.to_string()));
    }
    if h.bundle_id.is_some_and(|id| DENIED_HANDLERS.iter().any(|d| d.eq_ignore_ascii_case(id))) {
        return Decision::Reveal(Refusal::DeniedHandler(h.name.to_string()));
    }
    Decision::Open
}

/// 許可する型の根。中身の型がこのどれかに準拠すれば通す。
///
/// オフィス文書は型の階層に共通の根が無い（docx は `org.openxmlformats.openxml`、doc は
/// `public.data` の直下）ので、種類ごとに挙げる。iWork はパッケージ形式なら 2 で弾かれ、
/// 1 つのファイルの形式だけがここで通る。
const ALLOWED_TYPES: &[&str] = &[
    // ソースコード・シェルスクリプト・HTML・XML も準拠する。中身を実行するアプリが担当なら 4・5 で弾く。
    "public.text",
    "public.image",
    "public.audiovisual-content",
    "public.movie",
    "public.audio",
    "com.adobe.pdf",
    "org.openxmlformats.wordprocessingml.document",
    "org.openxmlformats.spreadsheetml.sheet",
    "org.openxmlformats.presentationml.presentation",
    "com.microsoft.word.doc",
    "com.microsoft.excel.xls",
    "com.microsoft.powerpoint.ppt",
    "org.oasis-open.opendocument",
    "com.apple.iwork.pages.sffpages",
    "com.apple.iwork.numbers.sffnumbers",
    "com.apple.iwork.keynote.sffkey",
];

/// 中身の型が許可した種類か。
///
/// 動的な型（`dyn.` で始まる）も通す。どのアプリも型を宣言していない拡張子（`.rs`・`.toml`
/// など）に LaunchServices が仮に振る型で、ソースコードの多くがこれになる。宣言が無いので
/// 「開くと実行する」と名乗るアプリも無く、担当は利用者が拡張子に結び付けたアプリか、
/// 何でも開くアプリ（テキストエディット）になる。担当アプリの確かめ（4・5）は同じく掛かる。
///
/// Why not 動的な型を弾く: ツリーに出るソースコードの大半がエディタで開けなくなる。
pub fn content_allowed(types: &[&str]) -> bool {
    types.iter().any(|t| {
        t.starts_with("dyn.") || ALLOWED_TYPES.iter().any(|a| a.eq_ignore_ascii_case(t))
    })
}

/// 型が汎用の根（`public.data` / `public.item` / `public.content`）だけか。拡張子の無い
/// ファイルや、どのアプリも宣言していない名前（`.gitignore`）は LaunchServices がここで止める。
///
/// `.zip` などのアーカイブは `public.zip-archive` のような固有の型を持つので、ここには
/// 入らない（中身がたまたまテキストに見えても通さない。アーカイブの担当は展開ユーティリティで、
/// 開くと中身をフォルダへ書き出す）。型が分からない（空）ものも入れない。
pub fn only_generic_types(types: &[&str]) -> bool {
    const GENERIC: &[&str] = &["public.data", "public.item", "public.content"];
    !types.is_empty() && types.iter().all(|t| GENERIC.iter().any(|g| g.eq_ignore_ascii_case(t)))
}

/// 何バイト目まで見てテキストかを決めるか。呼ぶ側はファイルの先頭をこの長さだけ読んで渡す。
pub const TEXT_SNIFF_BYTES: usize = 8 * 1024;

/// ファイルの先頭がテキストに見えるか。NUL を含まず、UTF-8 として読める（末尾で文字が
/// 途中で切れているのは、読んだ長さで切ったせいなので許す）。
///
/// Why not 先頭の数バイト（マジックナンバー）で判定する: 実行ファイルやアーカイブは
/// 型の側で固有の UTI になって 3 で止まる。ここに来るのは型の決まらないファイルだけで、
/// 見たいのは「エディタで開いて読めるか」なので、文字として読めるかを見る。
pub fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // error_len() が None のときは、末尾で文字が途中のまま終わっている。
        Err(e) => e.error_len().is_none() && head.len() - e.valid_up_to() < 4,
    }
}

/// 渡されたものを実行・インストール・登録するアプリ。実行ファイルを開けると名乗らないので、
/// [`Handler::opens_executables`] では弾けない分。
///
/// 端末（Terminal・iTerm2・Ghostty）は `opens_executables` で弾けるが、名乗り方が版で変わった
/// ときの備えとして入れておく。
pub const DENIED_HANDLERS: &[&str] = &[
    "com.apple.ScriptEditor2",
    "com.apple.Automator",
    "com.apple.AutomatorInstaller",
    "com.apple.installer",
    "com.apple.shortcuts",
    "com.apple.systempreferences",
    "com.apple.mcx.ProfileHelper",
    "com.apple.DiskImageMounter",
    "com.apple.JarLauncher",
    // `.py` を渡すと python で実行する。
    "org.python.PythonLauncher",
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
];

/// 開くと実行・任意 URL への移動・インストールになりうる拡張子。
///
/// 許可制（[`decide`] の 2〜5）の手前の安全網。型や担当アプリの読み違いがあっても、
/// ここに挙げた種類は開かない。
pub fn is_blocked_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some(
            "app" | "command" | "terminal" | "workflow" | "scpt" | "scptd" | "applescript"
                | "osascript" | "action" | "shortcut" | "tool"
                | "appex" | "xpc" | "prefpane" | "qlgenerator" | "vbs"
                // Java 実行系。
                | "jar" | "jnlp"
                // URL を開くだけのファイル（Finder が既定のブラウザやアプリへ渡す）。
                | "webloc" | "url" | "fileloc" | "inetloc"
                // 構成プロファイルとインストーラ・ディスクイメージ。
                | "mobileconfig" | "pkg" | "mpkg" | "dmg"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &[&str] = &["net.daringfireball.markdown", "public.plain-text", "public.text", "public.data"];
    const SHELL: &[&str] = &["public.shell-script", "public.script", "public.source-code", "public.plain-text", "public.text"];
    const EXECUTABLE: &[&str] = &["public.unix-executable", "public.executable", "public.data", "public.item"];

    fn file(kind: FileKind, types: &'static [&'static str]) -> FileFacts<'static> {
        FileFacts { kind, executable: false, content_types: types, looks_text: false }
    }

    fn editor() -> Handler<'static> {
        Handler { bundle_id: Some("com.microsoft.VSCode"), name: "Visual Studio Code", opens_executables: false }
    }

    fn open(path: &str, f: &FileFacts, h: Option<&Handler>) -> Decision {
        decide(Path::new(path), f, h)
    }

    #[test]
    fn a_markdown_file_with_an_editor_opens() {
        assert_eq!(open("/r/a.md", &file(FileKind::Regular, TEXT), Some(&editor())), Decision::Open);
    }

    #[test]
    fn images_pdfs_media_and_office_documents_open() {
        for types in [
            &["public.png", "public.image", "public.data"][..],
            &["com.adobe.pdf", "public.composite-content", "public.data"][..],
            &["public.mpeg-4", "public.movie", "public.audiovisual-content"][..],
            &["public.mp3", "public.audio", "public.audiovisual-content"][..],
            &["org.openxmlformats.wordprocessingml.document", "org.openxmlformats.openxml", "public.data"][..],
            &["com.microsoft.excel.xls", "public.data"][..],
        ] {
            let f = FileFacts { kind: FileKind::Regular, executable: false, content_types: types, looks_text: false };
            assert_eq!(open("/r/x.bin", &f, Some(&editor())), Decision::Open, "{types:?}");
        }
    }

    #[test]
    fn source_code_with_a_dynamic_type_opens() {
        let f = file(FileKind::Regular, &["dyn.ah62d4rv4ge81e62", "public.data", "public.item"]);
        assert_eq!(open("/r/main.rs", &f, Some(&editor())), Decision::Open);
    }

    #[test]
    fn a_type_outside_the_allowed_kinds_is_revealed() {
        let f = file(FileKind::Regular, &["com.example.thing", "public.data", "public.item"]);
        assert_eq!(open("/r/a.thing", &f, Some(&editor())), Decision::Reveal(Refusal::ContentType));
        let f = file(FileKind::Regular, &[]);
        assert_eq!(open("/r/a.thing", &f, Some(&editor())), Decision::Reveal(Refusal::ContentType), "型が分からない");
    }

    #[test]
    fn a_shell_script_handled_by_a_terminal_is_revealed() {
        let ghostty = Handler { bundle_id: Some("com.mitchellh.ghostty"), name: "Ghostty", opens_executables: true };
        assert_eq!(
            open("/r/x.sh", &file(FileKind::Regular, SHELL), Some(&ghostty)),
            Decision::Reveal(Refusal::RunsExecutables("Ghostty".into()))
        );
    }

    #[test]
    fn a_handler_that_opens_executables_is_refused_even_without_a_known_bundle_id() {
        let term = Handler { bundle_id: None, name: "Some Terminal", opens_executables: true };
        assert_eq!(
            open("/r/a.md", &file(FileKind::Regular, TEXT), Some(&term)),
            Decision::Reveal(Refusal::RunsExecutables("Some Terminal".into()))
        );
    }

    #[test]
    fn handlers_on_the_deny_list_are_refused() {
        for (id, path, types) in [
            ("com.apple.ScriptEditor2", "/r/x.txt", TEXT),
            ("com.apple.mcx.ProfileHelper", "/r/x.xml", &["com.apple.mobileconfig", "public.xml", "public.text"][..]),
            ("com.apple.Automator", "/r/x.txt", TEXT),
            ("com.apple.installer", "/r/x.txt", TEXT),
            ("org.python.PythonLauncher", "/r/x.py", &["public.python-script", "public.script", "public.source-code", "public.plain-text", "public.text"][..]),
        ] {
            let h = Handler { bundle_id: Some(id), name: "App", opens_executables: false };
            let f = FileFacts { kind: FileKind::Regular, executable: false, content_types: types, looks_text: false };
            assert_eq!(open(path, &f, Some(&h)), Decision::Reveal(Refusal::DeniedHandler("App".into())), "{id}");
        }
    }

    #[test]
    fn deny_list_matches_bundle_ids_without_case() {
        let h = Handler { bundle_id: Some("COM.APPLE.SCRIPTEDITOR2"), name: "Script Editor", opens_executables: false };
        assert!(matches!(open("/r/a.txt", &file(FileKind::Regular, TEXT), Some(&h)), Decision::Reveal(_)));
    }

    #[test]
    fn no_handler_is_revealed() {
        assert_eq!(open("/r/a.md", &file(FileKind::Regular, TEXT), None), Decision::Reveal(Refusal::NoHandler));
    }

    #[test]
    fn aliases_packages_and_special_files_are_revealed() {
        let h = editor();
        assert_eq!(open("/r/a.md", &file(FileKind::Alias, TEXT), Some(&h)), Decision::Reveal(Refusal::Alias));
        assert_eq!(open("/r/Foo.bundle", &file(FileKind::Package, &[]), Some(&h)), Decision::Reveal(Refusal::Package));
        assert_eq!(open("/r/fifo", &file(FileKind::Other, &[]), Some(&h)), Decision::Reveal(Refusal::NotRegular));
    }

    #[test]
    fn a_plain_folder_opens_without_a_content_type() {
        let finder = Handler { bundle_id: Some("com.apple.finder"), name: "Finder", opens_executables: false };
        assert_eq!(open("/r/docs", &file(FileKind::Folder, &[]), Some(&finder)), Decision::Open);
    }

    #[test]
    fn an_executable_without_an_extension_is_revealed() {
        let f = FileFacts { kind: FileKind::Regular, executable: true, content_types: EXECUTABLE, looks_text: false };
        assert_eq!(open("/r/run", &f, Some(&editor())), Decision::Reveal(Refusal::ExecutableNoExt));
        // 拡張子があれば実行の権限だけでは止めない（型と担当アプリで決める）。
        let f = FileFacts { kind: FileKind::Regular, executable: true, content_types: SHELL, looks_text: true };
        assert_eq!(open("/r/build.sh", &f, Some(&editor())), Decision::Open);
    }

    #[test]
    fn blocked_extensions_are_revealed_before_anything_else() {
        let h = editor();
        for p in [
            "/r/a.app", "/r/a.command", "/r/a.scpt", "/r/a.webloc", "/r/a.url", "/r/a.fileloc",
            "/r/a.inetloc", "/r/a.mobileconfig", "/r/a.pkg", "/r/a.mpkg", "/r/a.dmg", "/r/a.jar",
            "/r/A.WEBLOC", "/r/a.terminal", "/r/a.workflow", "/r/a.tool",
        ] {
            assert_eq!(open(p, &file(FileKind::Regular, TEXT), Some(&h)), Decision::Reveal(Refusal::BlockedExt), "{p}");
        }
    }

    const GENERIC: &[&str] = &["public.data", "public.item"];

    #[test]
    fn files_typed_only_as_data_open_as_text_when_their_head_reads_as_text() {
        for p in ["/r/.gitignore", "/r/.env", "/r/Dockerfile", "/etc/hosts"] {
            let f = FileFacts { kind: FileKind::Regular, executable: false, content_types: GENERIC, looks_text: true };
            assert_eq!(open(p, &f, Some(&editor())), Decision::Open, "{p}");
        }
    }

    #[test]
    fn files_typed_only_as_data_stay_refused_when_their_head_is_not_text() {
        let f = FileFacts { kind: FileKind::Regular, executable: false, content_types: GENERIC, looks_text: false };
        assert_eq!(open("/r/blob", &f, Some(&editor())), Decision::Reveal(Refusal::ContentType));
    }

    #[test]
    fn text_looking_data_files_still_go_through_the_handler_checks() {
        let f = FileFacts { kind: FileKind::Regular, executable: false, content_types: GENERIC, looks_text: true };
        let term = Handler { bundle_id: Some("com.apple.Terminal"), name: "Terminal", opens_executables: true };
        assert_eq!(open("/r/.env", &f, Some(&term)), Decision::Reveal(Refusal::RunsExecutables("Terminal".into())));
        let py = Handler { bundle_id: Some("org.python.PythonLauncher"), name: "Python Launcher", opens_executables: false };
        assert_eq!(open("/r/.env", &f, Some(&py)), Decision::Reveal(Refusal::DeniedHandler("Python Launcher".into())));
        assert_eq!(open("/r/.env", &f, None), Decision::Reveal(Refusal::NoHandler));
        // 実行の権限が立った拡張子無しは、先頭がテキストでも開かない。
        let f = FileFacts { kind: FileKind::Regular, executable: true, content_types: GENERIC, looks_text: true };
        assert_eq!(open("/r/run", &f, Some(&editor())), Decision::Reveal(Refusal::ExecutableNoExt));
    }

    #[test]
    fn a_zip_stays_refused_even_if_its_head_reads_as_text() {
        let f = FileFacts {
            kind: FileKind::Regular,
            executable: false,
            content_types: &["public.zip-archive", "com.pkware.zip-archive", "public.archive", "public.data", "public.item"],
            looks_text: true,
        };
        assert_eq!(open("/r/a.zip", &f, Some(&editor())), Decision::Reveal(Refusal::ContentType));
    }

    #[test]
    fn an_unknown_type_is_not_treated_as_generic() {
        assert!(!only_generic_types(&[]));
        assert!(only_generic_types(&["public.data", "public.item"]));
        assert!(!only_generic_types(&["com.example.thing", "public.data"]));
    }

    #[test]
    fn text_sniffing_accepts_utf8_and_rejects_nul_and_other_encodings() {
        assert!(looks_like_text(b"node_modules/\n.env\n"));
        assert!(looks_like_text("日本語のコメント\n".as_bytes()));
        assert!(looks_like_text(b""));
        // 読んだ長さで切れて、末尾の文字が途中になったもの。
        let cut = &"あいう".as_bytes()[..8];
        assert!(looks_like_text(cut));
        assert!(!looks_like_text(b"\x7fELF\x02\x01\x01\x00\x00"));
        assert!(!looks_like_text(b"PK\x03\x04\x14\x00"));
        // Shift_JIS の「日本」。
        assert!(!looks_like_text(b"\x93\xfa\x96\x7b"));
        // 途中の不正なバイトは許さない。
        assert!(!looks_like_text(b"abc\xffdef"));
    }

    #[test]
    fn a_missing_file_does_not_claim_it_was_revealed() {
        let m = Refusal::Missing.message();
        assert!(!m.contains("Finder"), "{m}");
        assert!(m.contains("見つからない"), "{m}");
    }

    #[test]
    fn refusal_messages_say_it_was_revealed_in_finder() {
        let m = Refusal::RunsExecutables("Terminal".into()).message();
        assert!(m.starts_with("Finder で表示しました"), "{m}");
        assert!(m.contains("Terminal"), "{m}");
    }
}
