use crate::data::Kv;
use crate::data::Post;
use crate::serve::RequestResult;
use crate::serve::ServerContext;
use chrono::DateTime;
use chrono::Duration;

/// https://url.spec.whatwg.org/#fragment-percent-encode-set
const FRAGMENT: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// Encode a string for use in a URL fragment.
/// For example, "file a.pdf" becomes "file%20a.pdf".
pub fn url_encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, FRAGMENT).to_string()
}

#[test]
fn test_url_encode() {
    let s = "file a.pdf";
    let encoded = url_encode(s);
    assert_eq!(encoded, "file%20a.pdf");
}

fn border_style(width: u64) -> String {
    format!(
        "border-bottom: {}px solid var(--border); border-radius: 0px;",
        width
    )
}

pub fn escape_single_quote(s: &str) -> String {
    s.replace('\'', "&#39;")
}

pub fn show_date<Tz: chrono::TimeZone>(datetime: &DateTime<Tz>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(datetime.clone());

    if duration < Duration::hours(24) && duration >= Duration::zero() {
        let hours = duration.num_hours();
        if hours == 0 {
            let minutes = duration.num_minutes();
            if minutes == 0 {
                "just now".to_string()
            } else if minutes == 1 {
                "1 minute ago".to_string()
            } else {
                format!("{} minutes ago", minutes)
            }
        } else if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        }
    } else {
        datetime.date_naive().format("%Y-%m-%d").to_string()
    }
}

const SET_LEAVE_CONFIRMATION: &str = "window.onbeforeunload = () => true;";
const UNSET_LEAVE_CONFIRMATION: &str = "window.onbeforeunload = null;";

/// Automatically set the `id` attribute for headers.
///
/// pulldown-cmark supports header attributes, but markdown-rs does not. That's
/// why we need to fix the html manually. Note that pulldown-cmark also does not
/// automatically set the `id` attribute
/// (https://github.com/pulldown-cmark/pulldown-cmark/issues/700), so some kind
/// of fixing is needed anyway.
fn set_header_id(html: &str) -> String {
    html.lines()
        .map(|line| {
            if line.trim().starts_with("<h") {
                if line.contains(" id=") {
                    // Do nothing.
                    return line.to_string();
                }
                if line.contains("<a href=") {
                    // Bail for this complex case.
                    return line.to_string();
                }
                // --- is sometimes interpreted as a horizontal rule.
                if line.contains("<hr />") {
                    return line.to_string();
                }
                let title_start = line.find('>').unwrap() + 1;
                let level = line[2..title_start - 1].to_string();
                let title_end = match line.find("</h") {
                    Some(end) => end,
                    None => {
                        tracing::warn!("could not find </h> in {line}");
                        return line.to_string();
                    }
                };
                let title = line[title_start..title_end].to_string();
                // Replace special characters because ' will break HTML and other characters may
                // make it hard to refer to the id from Markdown.
                let id = title
                    .to_lowercase()
                    .replace(' ', "-")
                    .replace("'", "-")
                    .replace("<code>", "")
                    .replace("</code>", "")
                    .replace("<", "")
                    .replace(">", "");
                format!("<h{level} id='{id}'>{title}</h{level}>")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

#[test]
fn test_set_header_id() {
    let html = indoc::indoc! {r#"
    <h1>Hello</h1>
    <h2>Bar baz</h2>
    <h3 id='quux'>Quux</h3>
    <h1><a href='/bar'>foo</a></h1>

    "#};
    let actual = set_header_id(html);
    println!("actual:\n{}", actual);
    let expected = indoc::indoc! {r#"
    <h1 id='hello'>Hello</h1>
    <h2 id='bar-baz'>Bar baz</h2>
    <h3 id='quux'>Quux</h3>
    <h1><a href='/bar'>foo</a></h1>

    "#};
    assert_eq!(actual.trim(), expected.trim());

    let html = indoc::indoc! {r#"
        <pre><code>function f(x)
            println(1)
        end
        </code></pre>
        "#}
    .trim();
    assert_eq!(html, set_header_id(html), "code block has changed");

    let html = indoc::indoc! {r#"
        <hr />
        "#}
    .trim();
    assert!(!set_header_id(html).is_empty());

    // Apostrophe must not break the id attribute.
    let html = "<h1>John's heading</h1>";
    let actual = set_header_id(html);
    assert_eq!(actual, "<h1 id='john-s-heading'>John's heading</h1>");
}

/// Generate a post link with given slug.
///
/// Will also create a valid post link if the slug is empty.
pub fn post_link(post: &Post, slug: &str) -> String {
    if slug.is_empty() {
        format!("/posts/{}", post.id)
    } else {
        format!("/posts/{}/{slug}", post.id)
    }
}

/// Add extra information such as last update date around the post content.
pub fn wrap_post_content(post: &Post, slug: &str, is_front_page_preview: bool) -> String {
    // Not wrapping the full post in a `href` because that prevents text
    // selection. I've tried all kinds of workarounds with putting a `position:
    // relative` object in front of the link with `z-index`, but that didn't
    // work. Either the area was clickable or the text was selectable but not
    // both.
    let html = if is_front_page_preview {
        // Front page preview is already HTML.
        post.content.clone()
    } else {
        crate::md::content_to_html(&post.content)
    };
    let html = set_header_id(&html);
    let style = if is_front_page_preview {
        &border_style(1)
    } else {
        ""
    };
    let updated = if post.created == post.updated || is_front_page_preview {
        ""
    } else {
        &format!(
            "<div class='updated'>last update: {}</div>",
            show_date(&post.updated)
        )
    };
    let unstyled_link = if is_front_page_preview {
        format!("<a href='{}' class='unstyled-link'>", post_link(post, slug))
    } else {
        "<span>".to_string()
    };
    let post_link_end = if is_front_page_preview {
        "</a>".to_string()
    } else {
        "</span>".to_string()
    };
    let post_preview_class = if is_front_page_preview {
        "post-preview"
    } else {
        ""
    };
    let share_link = if is_front_page_preview {
        "".to_string()
    } else {
        format!(
            "
            <div style='display: flex; justify-content: flex-end; \
              border-top: 1px solid var(--border); padding-top: 10px;
              font-size: var(--small-font-size);'>
                 <a href='{}' class='unstyled-link' id='long-url'>
                    🔗 Link
                 </a>&nbsp;(
                 <a id='copy-long-url' href='javascript:void(0)' onclick='copyLongUrl()'>
                    copy
                 </a>)
            </div>
            ",
            post_link(post, slug)
        )
    };
    format!(
        "
        <div class='post' style='{style}'>
            {unstyled_link}
                <div class='post-header'>
                    <div class='created'>{}</div>
                    {updated}
                </div>
            {post_link_end}
            <div data-post-link='{}' class='post-content {post_preview_class}'>
            {html}
            </div>
            {share_link}
        </div>
        ",
        show_date(&post.created),
        post_link(post, slug)
    )
}

pub enum Top {
    /// Show the top section for the homepage.
    Homepage,
    /// A button that goes back to the homepage.
    GoHome,
    /// A button that goes back to the previous page.
    ///
    /// This is used after the preview to go back to the post that was being
    /// edited.
    GoBack,
}

pub struct PageSettings {
    title: String,
    /// Whether the user is logged in.
    ///
    /// None means don't show login/logout buttons.
    is_logged_in: Option<bool>,
    /// The description of the page, defaults to the site description.
    description: Option<String>,
    show_about: bool,
    top: Top,
    extra_head: String,
}

impl PageSettings {
    pub fn new(
        title: &str,
        is_logged_in: Option<bool>,
        description: Option<&str>,
        show_about: bool,
        top: Top,
        extra_head: &str,
    ) -> Self {
        Self {
            title: title.to_string(),
            is_logged_in,
            description: description.map(|s| s.to_string()),
            show_about,
            top,
            extra_head: extra_head.to_string(),
        }
    }
}

pub fn edit_post_buttons(_ctx: &ServerContext, post: &Post) -> String {
    let id = post.id;
    indoc::formatdoc! {r#"
    <div style="margin-left: auto; display: flex; align-items: center;">
        <a class="button" href="/posts/edit/{id}">
            edit
        </a>
        <a class="button" href="/posts/delete/{id}">
            delete
        </a>
    </div>
    "#}
}

fn add_post_form() -> String {
    let markdown_link = crate::md::markdown_link();
    format!(
        "
    <form style='width: 100%;' action='/posts/add' method='post'>
        <textarea \
          style='display: block; width: 100%; height: 180px; margin-top: 10px;' \
          class='boxsizing-border' \
          oninput='disable_form_submit_if_empty(this);' \
          onchange='{SET_LEAVE_CONFIRMATION}' \
          id='content' name='content' placeholder='Your text..' required>
        </textarea>
        <div style='font-size: 0.8rem; text-align: right;'>
            This field supports {markdown_link}.
        </div>
        <br>
        <div style='display: flex; justify-content: flex-end;'>
            <input type='submit' onclick='{UNSET_LEAVE_CONFIRMATION}' \
              name='preview' value='Preview'/>
            <input type='submit' onclick='{UNSET_LEAVE_CONFIRMATION}' \
              name='publish' value='Publish'/>
        </div>
    </form>
    "
    )
    .to_string()
}

pub fn edit_post_form(post: &Post) -> String {
    let id = post.id;
    let content = &post.content;
    let markdown_link = crate::md::markdown_link();
    format!(
        "
    <form style='width: 100%;' action='/posts/edit/{id}' \
      method='post' onchange='{SET_LEAVE_CONFIRMATION}'>
        <textarea \
          style='display: block; width: 100%; height: 60vh; margin-top: 10px;' \
          class='boxsizing-border' \
          oninput='disable_form_submit_if_empty(this);' \
          id='content' name='content' placeholder='Your text..'>\n{content}
        </textarea>
        <div style='font-size: 0.8rem; text-align: right;'>
            This field supports {markdown_link}.
        </div>
        <br>
        <div style='display: flex; justify-content: flex-end;'>
            <input type='submit' onclick='{UNSET_LEAVE_CONFIRMATION}' \
              name='preview' value='Preview'/>
            <input type='submit' onclick='{UNSET_LEAVE_CONFIRMATION}' \
              name='publish' value='Publish'/>
        </div>
    </form>
    "
    )
}

/// Return formatted HTML/CSS that is small and readable.
pub fn minify(page: &str) -> String {
    let mut lines = Vec::new();
    // Whether to minify the current line.
    let mut inside_textarea = false;
    let mut inside_code = false;
    for line in page.lines() {
        let trimmed = line.trim();
        // Don't minify the textarea content or it will effectively modify the
        // textarea content.
        if trimmed.starts_with("<textarea ") {
            inside_textarea = true;
            lines.push(trimmed);
            continue;
        }
        // Highlighted code has attributes on <pre> and spans inside <code>.
        if inside_code {
            lines.push(line);
            if line.contains("</pre>") {
                inside_code = false;
            }
            continue;
        }
        if trimmed.starts_with("<pre>") || trimmed.starts_with("<pre ") {
            inside_code = !trimmed.contains("</pre>");
            lines.push(trimmed);
            continue;
        }
        if trimmed.starts_with("</textarea>") {
            inside_textarea = false;
        }
        if inside_textarea || inside_code {
            lines.push(line);
        } else if !trimmed.is_empty() {
            lines.push(trimmed);
        }
    }
    lines.join("\n")
}

#[test]
fn test_minify() {
    let page = indoc::indoc! {r#"
      <pre><code class="language-rust">x = 1;

    println!("{x}");
    </code></pre>
    "#};
    let expected = indoc::indoc! {r#"
    <pre><code class="language-rust">x = 1;

    println!("{x}");
    </code></pre>
    "#}
    .trim();
    assert_eq!(minify(page), expected);

    let page = indoc::indoc! {r#"
    <textarea id='about'>
    x = 1;

    println!("{x}");
    </textarea>
    "#};
    assert!(minify(page).contains("x = 1;\n\nprintln!"));
}

#[test]
fn test_minify_preserves_highlighted_code() {
    let code = "<pre class=\"syntax-code\"><code><span>fn</span> main() {\n    println!();\n\n}</code></pre>";
    let page = format!("  {code}\n   <p>After</p>\n\n");
    assert_eq!(minify(&page), format!("{code}\n<p>After</p>"));
    assert_eq!(
        minify("<pre><code>x</code></pre>\n\n   <p>After</p>"),
        "<pre><code>x</code></pre>\n<p>After</p>"
    );
}

async fn about(ctx: &ServerContext, settings: &PageSettings) -> RequestResult<String> {
    let about = Kv::get(&*ctx.conn()?, "about").unwrap();
    let about = String::from_utf8(about).unwrap();
    let about = crate::md::content_to_html(&about);
    let author_name = Kv::get(&*ctx.conn()?, "author_name").unwrap();
    let author_name = String::from_utf8(author_name).unwrap();
    let style = "font-size: 0.8rem; padding-top: 0.1rem;";
    let admin_buttons = if settings.is_logged_in.unwrap_or(false) {
        &format!(
            "
            <span>
                <a href='/files' class='unstyled-link' style='{style}'>
                    📁 Files
                </a>&nbsp;
                <a href='/settings' class='unstyled-link' style='{style}'>
                    ⚙️ Settings
                </a>
            </span>
            "
        )
    } else {
        ""
    };
    let container_style = "display: flex; justify-content: space-between;";
    let name_style = "font-size: 1.2rem; margin-bottom: 10px; font-weight: bold;";
    let search_button = format!(
        "
        <a href='/search' class='unstyled-link' style='{style}'>
            🔍
        </a>&nbsp;
        "
    );
    Ok(format!(
        "
    <div class='introduction' style='padding: 10px; {}'>
        <div style='{container_style}'>
            <div class='full-name' \
                style='{name_style}'>
                <a class='unstyled-link' href='/'>{author_name}</a>
            </div>
            <div>
                <span>
                    {search_button}
                </span>
            </div>
        </div>
        <div class='about' style='font-size: 0.9rem;'>{about}</div>
        <div>
            {admin_buttons}
        </div>
    </div>
    ",
        border_style(2),
    ))
}

fn katex_head(body: &str) -> String {
    let has_math = body.contains("<code class=\"language-math");
    let prefix = "https://cdn.jsdelivr.net/npm/katex@0.16.22/dist";
    if has_math {
        format!(
            "
            <link rel='stylesheet' href='{prefix}/katex.min.css' \
              crossorigin='anonymous'>
            <script defer src='{prefix}/katex.min.js' \
              crossorigin='anonymous'>
            </script>
            <script defer src='{prefix}/contrib/auto-render.min.js' \
              crossorigin='anonymous'>
            </script>
            <script defer src='/static/katex.js'>
            </script>
            "
        )
    } else {
        "".to_string()
    }
}

fn highlight_head(body: &str) -> &'static str {
    if body.contains("syntax-code") {
        "<link rel='stylesheet' href='/static/highlight.css'>"
    } else {
        ""
    }
}

pub async fn page(
    ctx: &ServerContext,
    settings: &PageSettings,
    body: &str,
) -> RequestResult<String> {
    let site_name = Kv::get(&*ctx.conn()?, "site_name").unwrap();
    let site_name = String::from_utf8(site_name).unwrap();
    let site_name = escape_single_quote(&site_name);
    let description = match &settings.description {
        Some(description) => description.clone(),
        None => Kv::get_or_empty_string(&*ctx.conn()?, "site_description"),
    };
    let full_title = if settings.title.is_empty() {
        site_name.clone()
    } else {
        format!("{} - {site_name}", settings.title)
    };
    let about = if settings.show_about {
        about(ctx, settings).await?
    } else {
        "".to_string()
    };
    let loginout = match settings.is_logged_in {
        Some(true) => r#"<a class="unstyled-link menu-space" href="/logout">Logout</a>"#,
        Some(false) => r#"<a class="unstyled-link menu-space" href="/login">Login</a>"#,
        None => "",
    };
    let top = match settings.top {
        Top::Homepage => {
            if settings.is_logged_in.unwrap_or(false) {
                &add_post_form()
            } else {
                ""
            }
        }
        Top::GoHome => indoc::indoc! {"
        <a href='/' class='button'>← back</a>
        "},
        Top::GoBack => indoc::indoc! {r#"
        <noscript>
            // no button because loading back will remove the previous content.
        </noscript>
        <script>
            document.write("<a href='javascript:history.back()' class='button'>← back</a>");
        </script>
        "#},
    };
    let html_lang = &ctx.args.html_lang;
    let extra_head = &settings.extra_head;
    let package_version = env!("CARGO_PKG_VERSION");
    let version = match option_env!("BLOGLITE_BUILD_LABEL") {
        Some("release") => package_version.to_owned(),
        None | Some("") => format!("{package_version} (custom build)"),
        Some(label) => format!("{package_version} ({label})"),
    };
    let highlight = highlight_head(body);
    let katex = katex_head(body);
    let og_title = if settings.title.is_empty() {
        &site_name
    } else {
        &settings.title
    };
    let data_theme = Kv::get(&*ctx.conn()?, "dark_mode").unwrap();
    let data_theme = String::from_utf8(data_theme).unwrap();
    let data_theme = if data_theme == "on" {
        ""
    } else {
        "data-theme='light'"
    };
    let page = indoc::formatdoc! {
        r#"
        <!DOCTYPE html>
        <html lang='{html_lang}' {data_theme}>
        <head>
            <meta charset='utf-8'>
            <meta name='viewport' content='width=device-width, initial-scale=1'>
            <link rel='stylesheet' href='/static/style.css'>
            <link rel='alternate' type='application/rss+xml' href='/feed.xml'>
            <script src='/static/script.js' defer></script>
            <title>{full_title}</title>
            <meta name='description' content='{description}'/>
            <meta property='og:description' content='{description}'/>
            <meta property='og:site_name' content='{site_name}'/>
            <meta property='og:title' content='{og_title}'/>
            {katex}
            {highlight}
            {extra_head}
        </head>
        <body>
            <div class='container'>
                <div class='middle'>
                    {about}
                    <div class='top'>
                        {top}
                    </div>
                    {body}
                    <div class='bottom'>
                        <a class='unstyled-link menu-space' href='https://github.com/rikhuijzer/bloglite'><u>Running bloglite</u> version: {version}</a>
                        {loginout}
                    </div>
                </div>
            </div>
            <script src='/static/nodefer.js'></script>
        </body>
        "#,
    };
    Ok(minify(&page))
}

pub async fn login(ctx: &ServerContext, error: Option<&str>) -> RequestResult<String> {
    let top = Top::Homepage;
    let description = "Login to the website";
    let settings = PageSettings::new("Login", None, Some(description), false, top, "");
    let error = match error {
        Some(error) => format!("<div style='font-style: italic;'>{error}</div>"),
        None => "".to_string(),
    };
    let style = "text-align: center; margin-top: 15vh;";
    let input_style = "font-size: 1rem;";
    let body = format!(
        "
        <form style='{style}' method='post' action='/login'>
            <input style='{input_style}' id='username' name='username' \
              type='text' placeholder='username' required/><br>
            <input style='{input_style}' id='password' name='password' \
              type='password' placeholder='password' required/><br>
            {error}
            <input style='{input_style}' type='submit' value='login'/>
        </form>
    "
    );
    page(ctx, &settings, &body).await
}
