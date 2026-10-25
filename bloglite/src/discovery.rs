//! Discovery protocols such as sitemap.xml, rss and robots.
use crate::data::Post;
use crate::files::File;
use crate::serve::RequestResult;
use crate::serve::ServerContext;
use crate::serve::content_type;
use crate::serve::response;
use crate::settings::Settings;
use axum::Router;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::get;

fn rfc822_datetime(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

fn xml_header() -> &'static str {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
}

#[test]
fn test_xml_header() {
    // Ensure last symbol is a newline and thus trimmed.
    assert_ne!(xml_header(), xml_header().trim());
}

fn escape_xml(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}

#[test]
fn test_escape_xml() {
    assert_eq!(escape_xml("foo&bar"), "foo&amp;bar");
}

fn rss_item(post: &Post, base: &str) -> Option<String> {
    if post.is_deleted() {
        return None;
    }

    let title = escape_xml(&crate::md::extract_html_title(post));
    let description = crate::md::extract_rss_description(post).replace("]]>", "]]]]><![CDATA[>");
    let url = escape_xml(&format!("{base}/posts/{}", post.id));
    let created = rfc822_datetime(&post.created);
    Some(format!(
        "
        <item>
        <title>{title}</title>
        <link>{url}</link>
        <guid>{url}</guid>
        <pubDate>{created}</pubDate>
        <description><![CDATA[{description}]]</description>
        </item>
        "
    ))
}

#[test]
fn test_deleted_post_is_excluded_from_rss() {
    let now = chrono::Utc::now();
    let post = Post {
        id: 1,
        created: now,
        updated: now,
        content: "<DELETED>".to_string(),
    };
    assert_eq!(rss_item(&post, "https://example.com"), None);
}

fn feed_url(base: &str, page: usize) -> String {
    if page == 1 {
        format!("{base}/feed.xml")
    } else {
        format!("{base}/feed.xml?page={page}")
    }
}

async fn rss(
    ctx: &ServerContext,
    posts: &[Post],
    page: usize,
    has_next: bool,
) -> RequestResult<String> {
    let settings = Settings::from_db(&*ctx.conn()?).unwrap();
    let site_name = escape_xml(&settings.site_name);
    let author_name = escape_xml(&settings.author_name);
    let base = ctx.base_url();
    let mut body = String::new();
    body.push_str(xml_header());
    body.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n");
    body.push_str("<channel>\n");
    body.push_str(&format!("<title>{site_name}</title>\n"));
    body.push_str(&format!("<link>{}</link>\n", escape_xml(&base)));
    body.push_str(&format!(
        "<description>Posts by {author_name}</description>\n"
    ));
    let mut links = vec![("self", page), ("first", 1)];
    if page > 1 {
        links.push(("previous", page - 1));
    }
    if has_next {
        links.push(("next", page + 1));
    }
    for (rel, page) in links {
        let href = escape_xml(&feed_url(&base, page));
        body.push_str(&format!(
            "<atom:link rel=\"{rel}\" href=\"{href}\" type=\"application/rss+xml\"/>\n"
        ));
    }
    for post in posts {
        if let Some(entry) = rss_item(post, &base) {
            body.push_str(&entry);
        }
    }
    body.push_str("</channel>\n");
    body.push_str("</rss>\n");
    // Preserve whitespace in full post HTML, including preformatted text.
    Ok(body)
}

#[derive(serde::Deserialize)]
struct FeedPagination {
    page: Option<usize>,
}

async fn get_rss(
    State(ctx): State<ServerContext>,
    Query(pagination): Query<FeedPagination>,
) -> RequestResult {
    let posts = Post::list(&*ctx.conn()?).unwrap();
    let page = pagination.page.unwrap_or(1);
    let page_size = 20;
    let Some(start) = page.checked_sub(1).and_then(|p| p.checked_mul(page_size)) else {
        return crate::serve::not_found(State(ctx)).await;
    };
    if page > 1 && start >= posts.len() {
        return crate::serve::not_found(State(ctx)).await;
    }
    let end = start.saturating_add(page_size).min(posts.len());
    let body = rss(&ctx, &posts[start..end], page, end < posts.len()).await?;
    let mut headers = HeaderMap::new();
    // Forces download in Firefox unfortunately:
    // https://www.petefreitag.com/blog/content-type-xml-feeds/
    content_type(&mut headers, "application/rss+xml; charset=utf-8");
    response(StatusCode::OK, headers, body, &ctx)
}

async fn get_robots(State(ctx): State<ServerContext>) -> RequestResult {
    let base = ctx.base_url();
    let sitemap_url = format!("{base}/sitemap.xml");
    let body = format!(
        "
        User-agent: *
        Disallow:
        Sitemap: {sitemap_url}
        "
    );
    let body = crate::html::minify(&body);
    let mut headers = HeaderMap::new();
    content_type(&mut headers, "text/plain; charset=utf-8");
    response(StatusCode::OK, headers, body, &ctx)
}

fn w3_datetime(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn sitemap(ctx: &ServerContext, posts: &[Post]) -> RequestResult<String> {
    let mut body = String::new();
    body.push_str(xml_header());
    body.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    let base = ctx.base_url();
    body.push_str(&format!("<url><loc>{base}/</loc></url>\n"));
    for post in posts {
        let url = format!("{base}/posts/{}", post.id);
        let url = crate::html::url_encode(&url);
        let updated = w3_datetime(&post.updated);
        let entry = format!(
            "
            <url>
            <loc>{url}</loc>
            <lastmod>{updated}</lastmod>
            </url>
            "
        );
        body.push_str(&entry);
    }
    let files = File::list(&*ctx.conn()?).unwrap();
    for file in files {
        let sha = &file.sha;
        let filename = file.filename_without_prefix();
        let filename = crate::html::url_encode(&filename);
        let url = format!("{base}/files/{sha}/{filename}");
        let entry = format!(
            "
            <url>
            <loc>{url}</loc>
            </url>
            "
        );
        body.push_str(&entry);
    }
    body.push_str("</urlset>\n");
    Ok(crate::html::minify(&body))
}

async fn get_sitemap(State(ctx): State<ServerContext>) -> RequestResult {
    let posts = Post::list(&*ctx.conn()?).unwrap();
    let body = sitemap(&ctx, &posts)?;
    let mut headers = HeaderMap::new();
    content_type(&mut headers, "text/xml");
    response(StatusCode::OK, headers, body, &ctx)
}

pub fn routes(router: &Router<ServerContext>) -> Router<ServerContext> {
    router
        .clone()
        .route("/feed.xml", get(get_rss))
        .route("/robots.txt", get(get_robots))
        .route("/sitemap.xml", get(get_sitemap))
}
