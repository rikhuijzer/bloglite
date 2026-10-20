use crate::data::Post;
use markdown::Options;
use markdown::ParseOptions;
use markdown::mdast::Node;
use markdown::to_mdast;

pub fn markdown_link() -> &'static str {
    "<a href='https://www.markdownguide.org/'>Markdown</a>"
}

/// Convert a Markdown AST node back to a `String` with the same structure.
///
/// The default `to_string()` method only returns text.
fn node_to_html(node: &Node) -> String {
    // Maybe this method should be rewritten to return Markdown or use some
    // internal logic from the `markdown` crate for the preview. I think the
    // reason that this part is converting to HTML now is that HTML is more
    // clear than Markdown. The benefit of having this code here is to later be
    // able to easily modify the Markdown node to HTML conversion. For example,
    // to remove hyperlinks from some Markdown code.
    let mut preview = String::new();
    match node {
        Node::Code(code) => {
            let lang = code.lang.clone().unwrap_or("".to_string());
            let class = if lang.is_empty() {
                ""
            } else {
                &format!("class=\"language-{lang}\"")
            };
            let html = format!(
                "
                <pre><code {class}>{}</code></pre>
                ",
                crate::highlight::escape(&code.value)
            );
            preview.push_str(&html);
        }
        Node::Paragraph(paragraph) => {
            preview.push_str("<p>");
            for child in paragraph.children.iter() {
                let text = node_to_html(child);
                preview.push_str(&text);
            }
            preview.push_str("</p>");
        }
        Node::Heading(heading) => {
            preview.push_str(&format!("<h{}>", heading.depth));
            for child in heading.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str(&format!("</h{}>", heading.depth));
            preview.push_str("\n\n");
        }
        Node::Emphasis(emphasis) => {
            preview.push_str("<em>");
            for child in emphasis.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</em>");
        }
        Node::Strong(strong) => {
            preview.push_str("<strong>");
            for child in strong.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</strong>");
        }
        Node::Delete(delete) => {
            preview.push_str("<del>");
            for child in delete.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</del>");
        }
        Node::Text(text) => preview.push_str(&text.value),
        Node::Html(html) => preview.push_str(&html.value),
        Node::Link(link) => {
            let text = node_to_html(link.children.first().unwrap());
            let url = &link.url;
            preview.push_str(&format!("<a href='{url}'>{text}</a>"));
        }
        Node::Math(math) => {
            preview.push_str(&format!(
                r#"
                <pre><code class="language-math math-display">{}
                </code></pre>
                "#,
                crate::highlight::escape(&math.value)
            ));
        }
        Node::Table(table) => {
            preview.push_str("<table>");
            for child in table.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</table>");
        }
        Node::TableRow(table_row) => {
            preview.push_str("<tr>");
            for child in table_row.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</tr>");
        }
        Node::Break(_br) => {
            preview.push_str("<br />");
        }
        Node::TableCell(table_cell) => {
            preview.push_str("<td>");
            for child in table_cell.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</td>");
        }
        Node::InlineCode(inline_code) => {
            preview.push_str(&format!("<code>{}</code>", inline_code.value));
        }
        Node::InlineMath(inline_math) => {
            let value = crate::highlight::escape(&inline_math.value);
            preview.push_str(&format!("<code class=\"language-math\">{value}</code>"));
        }
        Node::List(list) => {
            let tag = if list.ordered { "ol" } else { "ul" };
            preview.push_str(&format!("<{tag}>"));
            for child in list.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str(&format!("</{tag}>"));
        }
        Node::ListItem(list_item) => {
            preview.push_str("<li>");
            for child in list_item.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</li>");
        }
        Node::Image(image) => {
            let url = &image.url;
            let alt = &image.alt;
            preview.push_str(&format!("<img src='{url}' alt='{alt}' />"));
        }
        Node::FootnoteDefinition(_footnote_definition) => {}
        Node::Blockquote(blockquote) => {
            preview.push_str("<blockquote>");
            for child in blockquote.children.iter() {
                preview.push_str(&node_to_html(child));
            }
            preview.push_str("</blockquote>");
        }
        Node::FootnoteReference(_footnote_reference) => {}
        _ => {}
    }
    preview
}

fn parse_options() -> ParseOptions {
    let mut options = ParseOptions::default();
    options.constructs.gfm_table = true;
    // Parse math separately from code so it can be rendered to MathML.
    options.constructs.math_flow = true;
    options.constructs.math_text = true;
    options.constructs.gfm_footnote_definition = true;
    options.constructs.gfm_label_start_footnote = true;
    options
}

fn to_html_options() -> Options {
    let mut options = Options::default();
    options.compile.allow_dangerous_html = true;
    options.parse = parse_options();
    options
}

pub fn content_to_html(content: &str) -> String {
    let options = to_html_options();
    crate::math::html(&crate::highlight::html(
        &markdown::to_html_with_options(content, &options).unwrap(),
    ))
}

/// Prepare post to be shown as preview.
pub fn preview(post: &mut Post, max_length: usize) {
    let options = parse_options();
    let tree = to_mdast(&post.content, &options).unwrap();
    let mut preview = String::new();
    let slug = crate::md::extract_slug(post);
    for node in tree.children().unwrap() {
        if max_length < preview.len() {
            let id = post.id;
            let style = "font-size: 0.8rem;";
            let expand = format!(
                "
                <p>
                    <a href='/posts/{id}/{slug}' style='{style}'>
                        Show more
                    </a>
                </p>
                "
            );
            preview.push_str(&expand);
            break;
        }
        preview.push_str(&node_to_html(node));
    }
    post.content = crate::math::html(&crate::highlight::html(&preview));
}

#[test]
fn test_keep_link() {
    let content = indoc::indoc! {"
        # Title

        Lorem ipsum [foo](https://example.com/foo) dolor sit amet
    "};
    let mut post = Post {
        id: 0,
        content: content.to_string(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
    };
    preview(&mut post, 600);
    let expected = indoc::indoc! {"
        <h1>Title</h1>

        <p>Lorem ipsum <a href='https://example.com/foo'>foo</a> dolor sit amet</p>
    "};
    assert_eq!(post.content, expected.trim());
}

#[test]
fn test_sanitize_preview() {
    use chrono::Utc;
    // Need indoc to avoid indented lines to be interpreted as code.
    let content = indoc::indoc! {"
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed
    do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad
    minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex
    ea commodo consequat.
    
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod
    tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam,
    quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo
    consequat.
    
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod
    tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam,
    quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo
    consequat.
    
    Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod
    tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam,
    quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo
    consequat.
    "};
    let mut post = Post {
        id: 0,
        content: content.to_string(),
        created: Utc::now(),
        updated: Utc::now(),
    };
    preview(&mut post, 600);
    println!("post:\n{}", post.content);
    assert!(post.content.contains("Show more"));
    assert!(post.content.contains("<p>Lorem"));
}

fn remove_urls(md: &str) -> String {
    // This will break on nested links, but commonmark does not support nested
    // links according to <https://spec.commonmark.org/0.31.2/#links>.
    let re = regex::Regex::new(r"\[(.*?)\]\(https?://.*?\)").unwrap();
    re.replace_all(md, "$1").to_string()
}

fn truncate(text: &str, max_length: usize) -> String {
    let mut text = text.to_string();
    if text.len() > max_length {
        let mut pos = max_length;
        while pos > 0 && !text.is_char_boundary(pos) {
            pos -= 1;
        }
        text.truncate(pos);
    }
    text.trim().to_string()
}

pub fn extract_html_title(post: &Post) -> String {
    let title = &post.content;
    // This also would make a post with a single word on the first line have
    // that as the title which I guess makes sense.
    let title = title.split("\n").next().unwrap();
    let title = if title.starts_with("# ") {
        title.trim_start_matches("# ")
    } else {
        title
    };
    // Remove trailing newlines.
    let title = title.trim();
    let title = remove_urls(title);
    // Better a bit too long than too short. Google truncates anyway.
    let max_length = 60;
    if title.len() <= max_length {
        title
    } else {
        format!("{}...", truncate(&title, max_length))
    }
}

/// Extract readable text while preserving separation between Markdown blocks.
fn description_text(node: &Node) -> String {
    match node {
        Node::Html(_)
        | Node::Definition(_)
        | Node::FootnoteDefinition(_)
        | Node::FootnoteReference(_) => String::new(),
        Node::Image(image) => image.alt.clone(),
        Node::ImageReference(image) => image.alt.clone(),
        Node::Break(_) | Node::ThematicBreak(_) => " ".to_string(),
        _ => {
            // Keep link text without hrefs for readable meta descriptions;
            // full links remain in the article and RSS HTML.
            let mut text = if let Some(children) = node.children() {
                children.iter().map(description_text).collect::<String>()
            } else {
                node.to_string()
            };
            if matches!(
                node,
                Node::Paragraph(_)
                    | Node::Heading(_)
                    | Node::Code(_)
                    | Node::Math(_)
                    | Node::TableCell(_)
                    | Node::TableRow(_)
            ) {
                text.push(' ');
            }
            text
        }
    }
}

/// Description for the meta description and Open Graph description.
pub fn extract_html_description(post: &Post) -> String {
    let tree = to_mdast(&post.content, &parse_options()).unwrap();
    let mut nodes = tree.children().unwrap().iter().peekable();
    if matches!(nodes.peek(), Some(Node::Heading(heading)) if heading.depth == 1) {
        nodes.next();
    }
    let description = nodes.map(description_text).collect::<String>();
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    // Better a bit too long than too short. Google truncates anyway.
    let max_length = 200;
    let description = if description.len() <= max_length {
        description
    } else {
        format!("{}...", truncate(&description, max_length))
    };
    // Escape after truncation so HTML entities cannot be cut in half.
    description
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("'", "&apos;")
        .replace("\"", "&quot;")
}

#[test]
fn test_extract_html_description() {
    let post = Post {
        id: 0,
        content: "# Title\nipsum".to_string(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
    };
    let description = extract_html_description(&post);
    assert_eq!(description, "ipsum");

    let post = Post {
        id: 0,
        content: "lorem & ipsum".to_string(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
    };
    let description = extract_html_description(&post);
    assert_eq!(description, "lorem &amp; ipsum");
}

#[test]
fn test_description_interprets_markdown() {
    let mut post = Post {
        id: 0,
        content: String::new(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
    };
    for (content, expected) in [
        (
            "# Title\n\nlike _this_ and **that** with `code`",
            "like this and that with code",
        ),
        (
            "[a **bold** link](/path) and [reference][ref]\n\n[ref]: https://example.com",
            "a bold link and reference",
        ),
        (
            "First paragraph.\n\nSecond paragraph.\n\n- One\n- Two",
            "First paragraph. Second paragraph. One Two",
        ),
        ("Line  \nbreak", "Line break"),
        ("![Alt text](/image.png) <em>hello</em>", "Alt text hello"),
        (
            "&amp; \"quoted\" < 'text'",
            "&amp; &quot;quoted&quot; &lt; &apos;text&apos;",
        ),
        ("", ""),
        ("# Title", ""),
    ] {
        post.content = content.to_string();
        assert_eq!(extract_html_description(&post), expected, "{content}");
    }
    post.content = format!("{}&", "é".repeat(99));
    assert_eq!(
        extract_html_description(&post),
        format!("{}&amp;", "é".repeat(99))
    );
    post.content = "é".repeat(101);
    assert_eq!(
        extract_html_description(&post),
        format!("{}...", "é".repeat(100))
    );
}

/// Extract a slug (a short URL suffix to clarify the post) from the post.
///
/// For example, a post with the title `Foo Bar` and id `1` would receive the
/// slug `foo-bar` so that the post can be shared as `/posts/1/foo-bar`.
///
/// This function should be called before `crate::md::preview` because otherwise
/// the content is converted to Markdown and texts such as `<p><a href` will end
/// up in the slug
pub fn extract_slug(post: &Post) -> String {
    let title = extract_html_title(post);
    let slug = title.replace(" ", "-");
    let slug = slug
        .replace(",", "-")
        .replace("\"", "-")
        .replace("'", "")
        .replace(":", "-")
        .replace(";", "-")
        .replace("!", "")
        .replace("?", "")
        .replace(".", "-")
        .replace("/", "-")
        .to_lowercase();
    let max_length = 50;
    if slug.len() <= max_length {
        slug
    } else {
        truncate(&slug, max_length)
    }
}

#[test]
fn test_extract_slug() {
    let mut post = Post {
        id: 0,
        content: "Foo Bar".to_string(),
        created: chrono::Utc::now(),
        updated: chrono::Utc::now(),
    };
    assert_eq!(extract_slug(&post), "foo-bar");
    post.content = "Lorem, ipsum".to_string();
    assert_eq!(extract_slug(&post), "lorem--ipsum");
}

/// Used for RSS feed description field.
///
/// Many readers expect the description to be the full post, see for example,
/// <https://stackoverflow.com/a/7369487/5056635>.
pub fn extract_rss_description(post: &Post) -> String {
    content_to_html(&post.content)
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_extract_html_title_and_rss_description() {
        let post = Post {
            id: 0,
            content: "[lorem](https://example.com/lorem) ipsum".to_string(),
            created: Utc::now(),
            updated: Utc::now(),
        };
        let title = extract_html_title(&post);
        assert_eq!(title, "lorem ipsum");

        let post = Post {
            id: 0,
            content: "# Title\nipsum".to_string(),
            created: Utc::now(),
            updated: Utc::now(),
        };
        let title = extract_html_title(&post);
        assert_eq!(title, "Title");
        let description = extract_rss_description(&post);
        assert_eq!(description, "<h1>Title</h1>\n<p>ipsum</p>");
    }
}
