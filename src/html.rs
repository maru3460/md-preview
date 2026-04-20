use pulldown_cmark::{html, Options, Parser};

pub const CSS: &str = include_str!("style.css");
pub const HLJS_JS: &str = include_str!("highlight.min.js");
pub const HLJS_LIGHT_CSS: &str = include_str!("hljs-light.min.css");
pub const HLJS_DARK_CSS: &str = include_str!("hljs-dark.min.css");

pub const INIT_JS: &str = r#"
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

pub const MD_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES);

pub const FOLDER_JS: &str = r#"
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

pub fn render_body(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, MD_OPTIONS);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    body
}

pub fn build_html(body: &str, title: &str, custom_css: &str) -> String {
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

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn parse_frontmatter(s: &str) -> (Vec<(String, String)>, &str) {
    let after_open = if s.starts_with("---\r\n") {
        &s[5..]
    } else if s.starts_with("---\n") {
        &s[4..]
    } else {
        return (Vec::new(), s);
    };

    let close_pos = after_open.find("\n---\r\n")
        .map(|i| (i, i + 6))
        .or_else(|| after_open.find("\n---\n").map(|i| (i, i + 5)))
        .or_else(|| {
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

pub fn render_frontmatter_html(pairs: &[(String, String)]) -> String {
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

pub fn build_folder_html(title: &str, custom_css: &str, initial_file: Option<&str>) -> String {
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
