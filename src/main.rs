use std::borrow::Cow;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use pulldown_cmark::{html, Options, Parser};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{RequestAsyncResponder, WebViewBuilder};

const CSS: &str = include_str!("style.css");
const HLJS_JS: &str = include_str!("highlight.min.js");
const HLJS_LIGHT_CSS: &str = include_str!("hljs-light.min.css");
const HLJS_DARK_CSS: &str = include_str!("hljs-dark.min.css");

const INIT_JS: &str = r#"
function addHeadingIds() {
    document.querySelectorAll('h1,h2,h3,h4,h5,h6').forEach(function(h) {
        if (!h.id) {
            h.id = h.textContent
                .toLowerCase()
                .replace(/[^\p{L}\p{N}\s-]/gu, '')
                .trim()
                .replace(/\s+/g, '-');
        }
    });
}
// DOMContentLoaded 後すぐにウィンドウを表示するとペイント前の白背景が一瞬見える。
// setTimeout で1フレーム分待ってペイント完了後に表示する。
document.addEventListener('DOMContentLoaded', function() {
    addHeadingIds();
    hljs.highlightAll();
    setTimeout(function() { window.ipc.postMessage('ready'); }, 50);
});
document.addEventListener('keydown', function(e) {
    if (e.metaKey && e.key === 'w') {
        e.preventDefault();
        window.ipc.postMessage('close');
    }
});
document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href || href.charAt(0) !== '#') return;
    e.preventDefault();
    var id = decodeURIComponent(href.slice(1));
    var target = document.getElementById(id);
    if (target) target.scrollIntoView({ behavior: 'smooth' });
});
"#;

const MD_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES);

fn render_body(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, MD_OPTIONS);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    body
}

fn build_html(body: &str, title: &str, custom_css: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{css}</style>
<style>{hljs_light}</style>
<style>@media(prefers-color-scheme:dark){{{hljs_dark}}}</style>
<style>{custom_css}</style>
<script>{hljs_js}</script>
</head>
<body>
<article class="markdown-body">
{body}
</article>
</body>
</html>"#,
        title = title,
        css = CSS,
        hljs_light = HLJS_LIGHT_CSS,
        hljs_dark = HLJS_DARK_CSS,
        custom_css = custom_css,
        hljs_js = HLJS_JS,
        body = body,
    )
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn extension_to_hljs_lang(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("ts") => "typescript",
        Some("tsx" | "jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("c" | "h") => "c",
        Some("cpp" | "cc" | "cxx" | "hpp") => "cpp",
        Some("cs") => "csharp",
        Some("rb") => "ruby",
        Some("sh" | "bash" | "zsh" | "fish") => "bash",
        Some("json") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("html" | "htm" | "xml") => "xml",
        Some("css") => "css",
        Some("scss" | "sass") => "scss",
        Some("sql") => "sql",
        Some("kt" | "kts") => "kotlin",
        Some("swift") => "swift",
        Some("lua") => "lua",
        Some("php") => "php",
        _ => "plaintext",
    }
}

/// Extract YAML front matter from the start of a Markdown string.
/// Returns (pairs, remaining_body).
fn parse_frontmatter(s: &str) -> (Vec<(String, String)>, &str) {
    let after_open = if s.starts_with("---\r\n") {
        &s[5..]
    } else if s.starts_with("---\n") {
        &s[4..]
    } else {
        return (Vec::new(), s);
    };

    // Find the closing ---
    let close_pos = after_open.find("\n---\r\n")
        .map(|i| (i, i + 6))
        .or_else(|| after_open.find("\n---\n").map(|i| (i, i + 5)))
        .or_else(|| {
            // File ends with \n---
            after_open.strip_suffix("\n---").map(|_| {
                let i = after_open.len() - 4;
                (i, after_open.len())
            })
        });

    let (fm_end, body_start) = match close_pos {
        Some(v) => v,
        None => return (Vec::new(), s),
    };

    let fm_content = &after_open[..fm_end];
    let body = &after_open[body_start..];

    let pairs: Vec<(String, String)> = fm_content
        .lines()
        .filter_map(|line| {
            let colon = line.find(':')?;
            let key = line[..colon].trim().to_string();
            let val = line[colon + 1..].trim().to_string();
            if key.is_empty() { None } else { Some((key, val)) }
        })
        .collect();

    (pairs, body)
}

fn render_frontmatter_html(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut rows = String::new();
    for (k, v) in pairs {
        rows.push_str(&format!(
            r#"<div class="fm-row"><span class="fm-key">{}</span><span class="fm-val">{}</span></div>"#,
            html_escape(k),
            html_escape(v)
        ));
    }
    format!(r#"<div class="frontmatter">{}</div>"#, rows)
}

fn has_md_descendant(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let p = entry.path();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if has_md_descendant(&p) {
                return true;
            }
        } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
            return true;
        }
    }
    false
}

/// List immediate children of `dir` (all files + directories).
/// Returns JSON bytes: [{name, path (relative to root_dir), kind}, ...]
fn list_dir_json(dir: &Path, root_dir: &Path) -> Vec<u8> {
    let mut dirs: Vec<(String, String)> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let entry_path = entry.path();
            let rel = entry_path
                .strip_prefix(root_dir)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .into_owned();

            if entry_path.is_dir() {
                dirs.push((name, rel));
            } else {
                files.push((name, rel));
            }
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut items: Vec<serde_json::Value> = Vec::new();
    for (name, path) in dirs {
        items.push(serde_json::json!({ "name": name, "path": path, "kind": "dir" }));
    }
    for (name, path) in files {
        items.push(serde_json::json!({ "name": name, "path": path, "kind": "file" }));
    }

    serde_json::to_vec(&items).unwrap_or_else(|_| b"[]".to_vec())
}

/// Safely resolve a root-relative path, blocking traversal outside root_dir.
/// `canonical_root` must already be canonicalized by the caller.
fn safe_join(canonical_root: &Path, rel: &str) -> Option<PathBuf> {
    // Reject obviously dangerous patterns before canonicalize
    if rel.contains("..") {
        return None;
    }
    let candidate = canonical_root.join(rel);
    // canonicalize resolves symlinks — verify the path stays inside root
    let canonical = candidate.canonicalize().ok()?;
    if canonical.starts_with(canonical_root) {
        Some(canonical)
    } else {
        None
    }
}

const FOLDER_JS: &str = r#"
(function() {
  var expandedDirs = new Set();
  var currentFilePath = null;
  var windowReady = false;
  var mdCheckQueue = [];
  var mdDotCache = {};

  function doHasMdCheck(path, row) {
    if (path in mdDotCache) {
      if (mdDotCache[path]) {
        row.classList.add('has-md');
      }
      return;
    }
    fetch('/?has_md=' + encodeURIComponent(path))
      .then(function(r) { return r.json(); })
      .then(function(data) {
        mdDotCache[path] = !!data.has_md;
        if (data.has_md) {
          row.classList.add('has-md');
        }
      })
      .catch(function() {});
  }

  function scheduleHasMdCheck(path, row) {
    if (windowReady) {
      doHasMdCheck(path, row);
    } else {
      mdCheckQueue.push({path: path, row: row});
    }
  }

  function addHeadingIds() {
    document.querySelectorAll('#preview-pane h1,#preview-pane h2,#preview-pane h3,#preview-pane h4,#preview-pane h5,#preview-pane h6').forEach(function(h) {
      if (!h.id) {
        h.id = h.textContent
          .toLowerCase()
          .replace(/[^\p{L}\p{N}\s-]/gu, '')
          .trim()
          .replace(/\s+/g, '-');
      }
    });
  }

  function renderItems(items, parentEl, depth) {
    items.forEach(function(item) {
      var row = document.createElement('div');
      row.className = 'tree-item';
      row.style.paddingLeft = (8 + depth * 16) + 'px';

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

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          if (children.classList.contains('open')) {
            children.classList.remove('open');
            row.classList.remove('dir-open');
            expandedDirs.delete(item.path);
          } else {
            children.classList.add('open');
            row.classList.add('dir-open');
            expandedDirs.add(item.path);
            if (!loaded) {
              loaded = true;
              fetch('/?dir=' + encodeURIComponent(item.path))
                .then(function(r) { return r.json(); })
                .then(function(subItems) { renderItems(subItems, children, depth + 1); })
                .catch(function() {});
            }
          }
        });
      } else {
        if (item.name.endsWith('.md')) {
          row.classList.add('md-file');
        }
        icon.textContent = '';
        row.appendChild(icon);
        row.appendChild(document.createTextNode(item.name));
        parentEl.appendChild(row);

        row.addEventListener('click', function(e) {
          e.stopPropagation();
          document.querySelectorAll('.tree-item.active').forEach(function(el) {
            el.classList.remove('active');
          });
          row.classList.add('active');
          loadPreview(item.path);
        });
      }
    });
  }

  function resolveRelativePath(base, rel) {
    var parts = base.split('/');
    parts.pop();
    rel.split('/').forEach(function(seg) {
      if (seg === '..') { if (parts.length > 0) parts.pop(); }
      else if (seg !== '.') { parts.push(seg); }
    });
    return parts.join('/');
  }

  function loadPreview(relPath) {
    fetch('/?file=' + encodeURIComponent(relPath))
      .then(function(r) { return r.text(); })
      .then(function(html) {
        currentFilePath = relPath;
        var pane = document.getElementById('preview-pane');
        pane.innerHTML = html;
        pane.scrollTop = 0;
        addHeadingIds();
        if (window.hljs) hljs.highlightAll();
      })
      .catch(function() {});
  }

  document.addEventListener('DOMContentLoaded', function() {
    fetch('/?dir=')
      .then(function(r) { return r.json(); })
      .then(function(items) {
        var sidebar = document.getElementById('sidebar');
        renderItems(items, sidebar, 0);
        if (typeof INITIAL_FILE === 'string' && INITIAL_FILE) {
          loadPreview(INITIAL_FILE);
        }
        setTimeout(function() {
          window.ipc.postMessage('ready');
          windowReady = true;
          mdCheckQueue.forEach(function(item) { doHasMdCheck(item.path, item.row); });
          mdCheckQueue = [];
        }, 50);
      })
      .catch(function() {
        setTimeout(function() { window.ipc.postMessage('ready'); windowReady = true; }, 50);
      });
  });

  document.addEventListener('keydown', function(e) {
    if (e.metaKey && e.key === 'w') {
      e.preventDefault();
      window.ipc.postMessage('close');
    }
  });

  document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href) return;
    if (href.charAt(0) === '#') {
      e.preventDefault();
      var id = decodeURIComponent(href.slice(1));
      var target = document.getElementById(id);
      if (target) target.scrollIntoView({ behavior: 'smooth' });
    } else if (!href.startsWith('http://') && !href.startsWith('https://') && !href.startsWith('mailto:')) {
      var hashIdx = href.indexOf('#');
      var pathPart = hashIdx !== -1 ? href.slice(0, hashIdx) : href;
      var anchorPart = hashIdx !== -1 ? href.slice(hashIdx + 1) : '';
      if (pathPart.endsWith('.md')) {
        e.preventDefault();
        var resolved = currentFilePath ? resolveRelativePath(currentFilePath, pathPart) : pathPart;
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
"#;

fn build_folder_html(title: &str, custom_css: &str, initial_file: Option<&str>) -> String {
    let initial_file_script = format!(
        "<script>var INITIAL_FILE = {};</script>",
        serde_json::to_string(&initial_file).unwrap_or_else(|_| "null".to_string())
    );
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{css}</style>
<style>{hljs_light}</style>
<style>@media(prefers-color-scheme:dark){{{hljs_dark}}}</style>
<style>{custom_css}</style>
<script>{hljs_js}</script>
{initial_file_script}
</head>
<body class="folder-mode">
<div class="folder-layout">
  <div id="sidebar"></div>
  <div id="preview-pane"><div class="markdown-body"></div></div>
</div>
</body>
</html>"#,
        title = title,
        css = CSS,
        hljs_light = HLJS_LIGHT_CSS,
        hljs_dark = HLJS_DARK_CSS,
        custom_css = custom_css,
        hljs_js = HLJS_JS,
        initial_file_script = initial_file_script,
    )
}

#[cfg(target_os = "macos")]
fn get_frontmost_pid() -> Option<libc::pid_t> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(app.processIdentifier())
}

#[cfg(target_os = "macos")]
fn activate_pid(pid: libc::pid_t) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
}

#[cfg(target_os = "macos")]
fn setup_menu() {
    use objc2::sel;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::ns_string;

    let mtm = MainThreadMarker::new().expect("must be on main thread");

    let menubar = NSMenu::new(mtm);

    // App メニュー
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    unsafe {
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Quit"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        );
        app_menu.addItem(&quit);
        app_item.setSubmenu(Some(&app_menu));
        menubar.addItem(&app_item);
    }

    // Edit メニュー
    let edit_item = NSMenuItem::new(mtm);
    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Edit"));
    let make = |title: &objc2_foundation::NSString,
                sel_: objc2::runtime::Sel,
                key: &objc2_foundation::NSString| {
        unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                title,
                Some(sel_),
                key,
            )
        }
    };

    edit_menu.addItem(&make(ns_string!("Undo"),       sel!(undo:),      ns_string!("z")));
    let redo = make(ns_string!("Redo"), sel!(redo:), ns_string!("z"));
    redo.setKeyEquivalentModifierMask(
        objc2_app_kit::NSEventModifierFlags::Command
            | objc2_app_kit::NSEventModifierFlags::Shift,
    );
    edit_menu.addItem(&redo);
    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
    edit_menu.addItem(&make(ns_string!("Cut"),        sel!(cut:),       ns_string!("x")));
    edit_menu.addItem(&make(ns_string!("Copy"),       sel!(copy:),      ns_string!("c")));
    edit_menu.addItem(&make(ns_string!("Paste"),      sel!(paste:),     ns_string!("v")));
    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
    edit_menu.addItem(&make(ns_string!("Select All"), sel!(selectAll:), ns_string!("a")));

    edit_item.setSubmenu(Some(&edit_menu));
    menubar.addItem(&edit_item);

    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&menubar));
}

fn ok_response(content_type: &str, body: Vec<u8>) -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .header("Content-Type", content_type)
        .body(Cow::Owned(body))
        .unwrap()
}

fn not_found_response() -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .status(404)
        .body(Cow::Borrowed(b"Not Found" as &[u8]))
        .unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: md <file.md|directory>");
        std::process::exit(1);
    }

    let path = std::path::Path::new(&args[1])
        .canonicalize()
        .unwrap_or_else(|e| {
            eprintln!("Error: cannot resolve '{}': {}", args[1], e);
            std::process::exit(1);
        });

    let custom_css = std::env::var("HOME")
        .ok()
        .and_then(|home| {
            std::fs::read_to_string(format!("{}/.config/md-preview/style.css", home)).ok()
        })
        .unwrap_or_default();

    let is_folder = path.is_dir();

    let current_dir = std::env::current_dir().ok()
        .and_then(|d| d.canonicalize().ok());

    let file_in_cwd = !is_folder && current_dir.as_ref()
        .map(|cwd| path.starts_with(cwd))
        .unwrap_or(false);

    let (title, init_script, html_bytes, window_width, root_dir) = if is_folder {
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .to_string();
        let html = build_folder_html(&title, &custom_css, None);
        (title, FOLDER_JS, html.into_bytes(), 1200.0_f64, path.clone())
    } else if file_in_cwd {
        let cwd = current_dir.unwrap();
        let rel = path.strip_prefix(&cwd)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let dir_title = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .to_string();
        let html = build_folder_html(&dir_title, &custom_css, Some(&rel));
        (dir_title, FOLDER_JS, html.into_bytes(), 1200.0_f64, cwd)
    } else {
        let markdown = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Error: cannot read '{}': {}", path.display(), e);
                std::process::exit(1);
            }
        };
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Markdown Preview")
            .to_string();
        let (fm_pairs, body) = parse_frontmatter(&markdown);
        let fm_html = render_frontmatter_html(&fm_pairs);
        let html = build_html(&format!("{}{}", fm_html, render_body(body)), &title, &custom_css);
        let base_dir = path.parent().unwrap_or(&path).to_path_buf();
        (title, INIT_JS, html.into_bytes(), 900.0_f64, base_dir)
    };

    #[cfg(target_os = "macos")]
    let launcher_pid = get_frontmost_pid();

    let event_loop = EventLoopBuilder::<&'static str>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(window_width, 700.0_f64))
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create window");

    let webview = WebViewBuilder::new()
        .with_initialization_script(init_script)
        .with_navigation_handler(|url: String| {
            if url.starts_with("http://") || url.starts_with("https://") {
                std::process::Command::new("open").arg(&url).spawn().ok();
                false
            } else {
                true
            }
        })
        .with_asynchronous_custom_protocol("mdpreview".to_string(), {
            let custom_css = custom_css;
            move |_webview_id, request, responder: RequestAsyncResponder| {
                let url_path = percent_decode_str(request.uri().path())
                    .decode_utf8_lossy()
                    .into_owned();
                let query = request.uri().query().unwrap_or("").to_string();

                if let Some(rel_encoded) = query.strip_prefix("has_md=") {
                    let rel = percent_decode_str(rel_encoded).decode_utf8_lossy().into_owned();
                    let root = root_dir.clone();
                    std::thread::spawn(move || {
                        let found = safe_join(&root, &rel)
                            .map(|p| has_md_descendant(&p))
                            .unwrap_or(false);
                        let body = serde_json::to_vec(&serde_json::json!({"has_md": found}))
                            .unwrap_or_else(|_| b"{}".to_vec());
                        responder.respond(ok_response("application/json; charset=utf-8", body));
                    });
                    return;
                }

                let response = (|| {
                // Folder mode: ?dir=<rel> or ?file=<rel>
                if let Some(rel_encoded) = query.strip_prefix("dir=") {
                    let rel = percent_decode_str(rel_encoded).decode_utf8_lossy().into_owned();
                    let target_dir = if rel.is_empty() {
                        Some(root_dir.clone())
                    } else {
                        safe_join(&root_dir, &rel)
                    };
                    return match target_dir {
                        Some(dir) if dir.is_dir() => {
                            let json = list_dir_json(&dir, &root_dir);
                            ok_response("application/json; charset=utf-8", json)
                        }
                        _ => not_found_response(),
                    };
                }

                if let Some(rel_encoded) = query.strip_prefix("file=") {
                    let rel = percent_decode_str(rel_encoded).decode_utf8_lossy().into_owned();
                    return match safe_join(&root_dir, &rel) {
                        Some(file_path) => {
                            let is_md = file_path.extension().and_then(|e| e.to_str()) == Some("md");
                            if is_md {
                                match std::fs::read_to_string(&file_path) {
                                    Ok(content) => {
                                        let (fm_pairs, body) = parse_frontmatter(&content);
                                        let fm_html = render_frontmatter_html(&fm_pairs);
                                        let fragment = format!(
                                            r#"{}<div class="markdown-body">{}</div>"#,
                                            fm_html,
                                            render_body(body)
                                        );
                                        ok_response("text/html; charset=utf-8", fragment.into_bytes())
                                    }
                                    Err(_) => not_found_response(),
                                }
                            } else {
                                match std::fs::read(&file_path) {
                                    Ok(bytes) => match String::from_utf8(bytes) {
                                        Ok(content) => {
                                            let lang = extension_to_hljs_lang(&file_path);
                                            let escaped = html_escape(&content);
                                            let fragment = format!(
                                                r#"<div class="markdown-body"><pre><code class="language-{}">{}</code></pre></div>"#,
                                                lang, escaped
                                            );
                                            ok_response("text/html; charset=utf-8", fragment.into_bytes())
                                        }
                                        Err(_) => {
                                            let name = file_path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("file");
                                            let fragment = format!(
                                                r#"<div class="markdown-body"><p class="binary-msg">バイナリファイルは表示できません: {}</p></div>"#,
                                                html_escape(name)
                                            );
                                            ok_response("text/html; charset=utf-8", fragment.into_bytes())
                                        }
                                    },
                                    Err(_) => not_found_response(),
                                }
                            }
                        }
                        None => not_found_response(),
                    };
                }

                // Root: serve initial HTML
                if url_path == "/" {
                    return ok_response("text/html; charset=utf-8", html_bytes.clone());
                }

                // Single-file mode: relative asset / .md links
                let relative = url_path.strip_prefix('/').unwrap_or(&url_path);
                let file_path = root_dir.join(relative);
                if file_path.extension().and_then(|e| e.to_str()) == Some("md") {
                    match std::fs::read_to_string(&file_path) {
                        Ok(content) => {
                            let file_title = file_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Markdown Preview");
                            let rendered =
                                build_html(&render_body(&content), file_title, &custom_css);
                            ok_response("text/html; charset=utf-8", rendered.into_bytes())
                        }
                        Err(_) => not_found_response(),
                    }
                } else {
                    match std::fs::read(&file_path) {
                        Ok(bytes) => ok_response(guess_mime(&file_path), bytes),
                        Err(_) => not_found_response(),
                    }
                }
                })();
                responder.respond(response);
            }
        })
        .with_ipc_handler(move |msg| {
            match msg.body().as_str() {
                "close" => { let _ = proxy.send_event("close"); }
                "ready" => { let _ = proxy.send_event("ready"); }
                _ => {}
            }
        })
        .with_url("mdpreview://localhost/")
        .build(&window)
        .expect("Failed to create WebView");

    #[cfg(target_os = "macos")]
    setup_menu();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _ = &webview;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent("close") => {
                #[cfg(target_os = "macos")]
                if let Some(pid) = launcher_pid {
                    activate_pid(pid);
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent("ready") => {
                window.set_visible(true);
            }
            _ => {}
        }
    });
}
