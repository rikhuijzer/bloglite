mod common;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use bloglite::serve::LoginForm;
use bloglite::serve::app;
use common::*;
use http_body_util::BodyExt;
use sha2::Digest;
use tower::util::ServiceExt;

#[tokio::test]
async fn test_upload_jumps_to_alphabetically_first_file() {
    let (mut ctx, auth) = request_cookie().await;
    ctx.args.trigger_token = None;
    let router = app(ctx.clone());
    // Include a duplicate upload.
    for filenames in [
        vec!["z.txt"],
        vec!["z.txt", "a.txt"],
        vec!["a.txt", "z.txt"],
        vec!["a.txt"],
    ] {
        let mut multipart = String::new();
        for filename in &filenames {
            multipart.push_str(&format!(
                "--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: text/plain\r\n\r\n{filename}\r\n"
            ));
        }
        multipart.push_str(
            "--boundary\r\nContent-Disposition: form-data; name=\"prefix\"\r\n\r\nexample/\r\n--boundary--\r\n",
        );
        let req = Request::builder()
            .method("POST")
            .uri("/files/add")
            .header("Cookie", format!("auth={auth}"))
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(multipart))
            .unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let filename = filenames.iter().min().unwrap();
        let sha = hex::encode(&sha2::Sha256::digest(filename.as_bytes())[..8]);
        assert_eq!(response.headers()["Location"], format!("/files#file-{sha}"));
        let req = Request::builder()
            .uri("/files")
            .header("Cookie", format!("auth={auth}"))
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains(&format!("<div id='file-{sha}'")));
        assert!(body.contains(&format!("example/{filename}")));
    }
}

#[tokio::test]
async fn test_search_escapes_query_in_form() {
    // Double quotes make this a valid FTS phrase while the apostrophe would
    // otherwise break out of the form's single-quoted value attribute.
    let query = "\"'><img src=x onerror=alert(1)>&amp;\"";
    let query = serde_urlencoded::to_string([("q", query)]).unwrap();
    let (status, body) = request_body(&format!("/search?{query}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("value='&quot;&#39;&gt;&lt;img src=x onerror=alert(1)&gt;&amp;amp;&quot;'")
    );
    assert!(!body.contains("<img src=x onerror=alert(1)>"));
}

#[tokio::test]
async fn test_search_still_returns_matches() {
    let (status, body) = request_body("/search?q=Lorem").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("value='Lorem'"));
    assert!(body.contains("/posts/1"));
}

#[tokio::test]
async fn test_search_rejects_malformed_queries_and_recovers() {
    let mut ctx = server_context().await;
    // Reuse the same connection to verify that failed requests clean it up.
    ctx.pool = bloglite::data::DbPool::memory(1).unwrap();
    bloglite::data::init(&ctx.args, &ctx.conn().unwrap());
    let router = app(ctx.clone());
    for query in ["\"", "Lorem OR", "(Lorem", "unknown:Lorem"] {
        let query = serde_urlencoded::to_string([("q", query)]).unwrap();
        let req = Request::builder()
            .uri(format!("/search?{query}"))
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("Invalid search query syntax."));
        assert!(body.contains("<form action='/search'"));

        let conn = ctx.conn().unwrap();
        assert!(conn.is_autocommit());
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'posts_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
        drop(conn);

        let req = Request::builder()
            .uri("/search?q=Lorem")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            String::from_utf8(body.to_vec())
                .unwrap()
                .contains("/posts/1")
        );
    }
}

#[tokio::test]
async fn test_search_database_failure_returns_server_error() {
    let ctx = server_context().await;
    ctx.conn().unwrap().execute("DROP TABLE posts", []).unwrap();
    let req = Request::builder()
        .uri("/search?q=Lorem")
        .body(Body::empty())
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("Search is temporarily unavailable."));
    assert!(!body.contains("no such table"));
    assert!(ctx.conn().unwrap().is_autocommit());
}

#[tokio::test]
async fn test_home() {
    let (status, body) = request_body("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<!DOCTYPE html>"));
    assert!(body.contains("Lorem"));
    assert!(body.contains("Dolor"));
    assert!(body.contains("<meta name='description'"));
    assert!(body.contains("<meta property='og:description'"));
    assert!(body.contains("<meta property='og:site_name' content='John&#39;s Weblog'/>"));
    assert!(body.contains("<meta property='og:type' content='website'/>"));
    assert!(body.contains("<meta property='og:title' content='John&#39;s Weblog'/>"));
    // Assumes that the "Code" post shows a code block in the preview.
    assert!(body.contains("/static/highlight.css"));
    // Assumes that math is shown in one of the previews.
    assert!(body.contains("<math"));
    assert!(!body.contains("katex"));
}

#[tokio::test]
async fn test_home_invalid_pages() {
    for page in [0, 2, usize::MAX / 10 + 1, usize::MAX / 10 + 2, usize::MAX] {
        let (status, _) = request_body(&format!("/?page={page}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "page={page}");
    }
    let (status, body) = request_body("/?page=1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Lorem"));
}

#[tokio::test]
async fn test_home_pagination_and_stale_page() {
    let ctx = server_context().await;
    ctx.conn()
        .unwrap()
        .execute("DELETE FROM posts", [])
        .unwrap();
    let now = chrono::Utc::now();
    for index in 0..11 {
        bloglite::data::Post::insert(
            &ctx.conn().unwrap(),
            now,
            now,
            &format!("Pagination post {index}"),
        )
        .unwrap();
    }
    let router = app(ctx.clone());
    for (uri, expected_count, has_next) in [("/", 10, true), ("/?page=2", 1, false)] {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body.matches("Pagination post").count(), expected_count);
        assert_eq!(body.contains("▶ next"), has_next);
    }
    ctx.conn()
        .unwrap()
        .execute(
            "DELETE FROM posts WHERE id = (SELECT max(id) FROM posts)",
            [],
        )
        .unwrap();
    let req = Request::builder()
        .uri("/?page=2")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_home_empty_database() {
    let ctx = server_context().await;
    ctx.conn()
        .unwrap()
        .execute("DELETE FROM posts", [])
        .unwrap();
    let router = app(ctx);
    for (uri, status) in [
        ("/", StatusCode::OK),
        ("/?page=1", StatusCode::OK),
        ("/?page=2", StatusCode::NOT_FOUND),
    ] {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), status, "{uri}");
    }
}

#[tokio::test]
async fn test_get_post() {
    let (status, body) = request_body("/posts/2/code").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Dolor"));
    assert!(body.contains("<h2 id='more'>More</h2>"));
    assert!(body.contains("/posts/2/code"));
    assert!(body.contains("<meta property='og:site_name' content='John&#39;s Weblog'/>"));
    assert!(body.contains("<meta property='og:type' content='article'/>"));
    assert!(body.contains("<meta property='og:title' content='Code'/>"));

    let (status, _body) = request_body("/posts/2").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
}

#[tokio::test]
async fn test_front_page_preview() {
    let (status, body) = request_body("/").await;
    assert_eq!(status, StatusCode::OK);
    let body = bloglite::html::minify(&body);
    println!("body:\n{body}");
    let code_area = indoc::indoc! {
        r#"
        <pre><code class="language-julia">function f(x)
            println(1)
            return x
        end

        find . -iname "*.tex" -o -iname "*.bib" | entr latexmk -pdf</code></pre>
        "#
    };
    let plain = regex::Regex::new(r"<[^>]+>")
        .unwrap()
        .replace_all(&body, "");
    let code_area = code_area.replace('"', "&quot;");
    let expected = regex::Regex::new(r"<[^>]+>")
        .unwrap()
        .replace_all(code_area.trim(), "");
    assert!(plain.contains(expected.as_ref()));
    assert!(body.contains("syntax-source"));
    assert!(body.contains("Show more"));
}

#[tokio::test]
async fn test_style() {
    let (status, body) = request_body("/static/style.css").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("body {"));
}

#[tokio::test]
async fn test_metadata() {
    let url = "/posts/1/lorem-ipsum-ut-enim-ad-minim-veniam-sit-amet-ipsum";
    let (status, body) = request_body(url).await;
    assert_eq!(status, StatusCode::OK);
    let lines = body.lines().collect::<Vec<_>>();
    let head_start = lines
        .iter()
        .position(|line| line.contains("<head>"))
        .unwrap();
    let head_end = lines
        .iter()
        .position(|line| line.contains("</head>"))
        .unwrap();
    let head = lines[head_start..head_end + 1].join("\n");
    println!("head:\n{head}");
    assert!(body.contains("<!DOCTYPE html>"));
    assert!(body.contains(
        "<title>Lorem ipsum ut enim ad minim veniam sit amet ipsum lorem con... - John&#39;s Weblog</title>"
    ));
    assert!(body.contains("<meta property='og:site_name' content='John&#39;s Weblog'/>"));
    assert!(body.contains("<meta property='article:author' content='John'/>"));
}

#[tokio::test]
async fn test_login_page() {
    let (status, body) = request_body("/login").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("username"));
}

#[tokio::test]
async fn test_login() {
    let ctx = common::server_context().await;

    // Valid login.
    let form = LoginForm {
        username: "test-admin".to_string(),
        password: "test-password".to_string(),
    };
    let form_data = serde_urlencoded::to_string(&form).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/login")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(form_data))
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get("Location").unwrap(), "/");
    let cookie = response.headers().get("Set-Cookie").unwrap();
    let cookie = cookie.to_str().unwrap();
    assert!(cookie.contains("auth="));
    assert!(cookie.contains("Secure"));
    let cookie = cookie.split(";").next().unwrap();
    let auth = cookie.split("=").nth(1).unwrap();
    println!("auth: {auth}");

    // Valid cookie.
    let req = Request::builder()
        .method("GET")
        .uri("/")
        .header("Cookie", format!("auth={auth}"))
        .body(Body::empty())
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap();
    let body: Vec<u8> = body.to_bytes().into();
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("Lorem"));
    assert!(!body.contains("login"));
    assert!(body.contains("logout"));

    // Invalid cookie.
    let req = Request::builder()
        .method("GET")
        .uri("/")
        .header("Cookie", "auth=invalid")
        .body(Body::empty())
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap();
    let body: Vec<u8> = body.to_bytes().into();
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("Lorem"));
    assert!(body.contains("login"));
    assert!(!body.contains("logout"));

    // Wrong password.
    let form = LoginForm {
        username: "test-admin".to_string(),
        password: "wrong-password".to_string(),
    };
    let form_data = serde_urlencoded::to_string(&form).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/login")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(form_data))
        .unwrap();
    let app = app(ctx);
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = response.into_body().collect().await.unwrap();
    let body: Vec<u8> = body.to_bytes().into();
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("login"));
    assert!(body.contains("Invalid username or password"));
}

#[tokio::test]
async fn test_delete_confirmation() {
    let (status, body) = request_body_logged_in("/posts/delete/1").await;
    assert_eq!(status, StatusCode::OK);
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("Are you sure you want to delete this post?"));
    assert!(body.contains("Lorem"));
}

#[tokio::test]
async fn test_post_add() {
    let (ctx, auth) = request_cookie().await;
    let form = bloglite::serve::AddPostForm {
        content: "Lorem https://example.com".to_string(),
    };
    let form_data = serde_urlencoded::to_string(&form).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/posts/add")
        .header("Cookie", format!("auth={auth}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(form_data))
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap();
    let body: Vec<u8> = body.to_bytes().into();
    let body = String::from_utf8(body).unwrap();
    println!("body:\n{body}");
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("example.com"), "redirect to the new post");
    assert!(
        // Do not auto-detect the URL as a link because there is no way for the
        // user to disable it. Furthermore, standard Markdown also does not
        // auto-detect links.
        body.contains("Lorem https://example.com"),
        "auto autolink"
    );
}

#[tokio::test]
async fn test_get_edit() {
    let (status, body) = request_body_logged_in("/posts/edit/2").await;
    let body = String::from_utf8(body).unwrap();
    assert_eq!(status, StatusCode::OK);
    println!("body:\n{body}");
    assert!(
        body.contains("# Code\n\n`Dolor sit"),
        "textarea content might be minified"
    );
}

#[tokio::test]
async fn test_post_edit() {
    let (ctx, auth) = request_cookie().await;
    let form = bloglite::serve::EditPostForm {
        content: "Lorem https://example.com".to_string(),
    };
    let form_data = serde_urlencoded::to_string(&form).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/posts/edit/2")
        .header("Cookie", format!("auth={auth}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(form_data))
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap();
    let body: Vec<u8> = body.to_bytes().into();
    let body = String::from_utf8(body).unwrap();
    println!("body:\n{body}");
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("# Code"), "text not updated");
    assert!(body.contains("https://example.com"), "text not updated");
}

#[tokio::test]
async fn test_no_crash_on_unknown_file() {
    let (status, _body) = request_body("/files/2323").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_allow_filename_suffix() {
    let url = "/files/69b83ddf8f65695f-example.txt";
    let (status, _body) = request_body(url).await;
    assert_eq!(status, StatusCode::OK);
    let url = "/files/69b83ddf8f65695f-foo.jl";
    let (status, _body) = request_body(url).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_allow_filename_suffix_after_forward_slash() {
    let url = "/files/69b83ddf8f65695f/example.txt";
    let (status, _body) = request_body(url).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_sitemap() {
    let (status, body) = request_body("/sitemap.xml").await;
    assert_eq!(status, StatusCode::OK);
    println!("body:\n{body}");
    assert!(body.contains("xml version"));
    assert!(body.contains("<urlset xmlns"));
    assert!(body.contains("<loc>/</loc>"));
    assert!(body.contains("<loc>/posts/1</loc>"));
    assert!(body.contains("<loc>/posts/2</loc>"));
    assert!(body.contains("<lastmod>"));
    assert!(body.contains("<loc>/files/69b83ddf8f65695f/example.txt</loc>"));
}

#[tokio::test]
async fn test_robots() {
    let (status, body) = request_body("/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    println!("body:\n{body}");
    assert!(body.starts_with("User-agent: *"));
    assert!(body.contains("Disallow:\n"));
    assert!(body.contains("Sitemap:"));
}

#[tokio::test]
async fn test_rss() {
    let (status, body) = request_body("/feed.xml").await;
    assert_eq!(status, StatusCode::OK);
    println!("body:\n{body}");
    assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(body.contains("<description><![CDATA[<p><a href=\"https://example.com"),);
    assert!(body.contains("<description><![CDATA[<h1>Code</h1>"));
}

#[tokio::test]
async fn test_full_rss_pages() {
    let mut ctx = server_context().await;
    ctx.args.domain = "example.com".to_string();
    ctx.conn()
        .unwrap()
        .execute("DELETE FROM posts", [])
        .unwrap();
    let now = chrono::Utc::now();
    let content = format!(
        "# Full post\n\n{}\n\n**The end**\n\n<pre>  first\n\n    second</pre>\n\n<!-- ]]> -->",
        "Long paragraph. ".repeat(100)
    );
    let mut ids = Vec::new();
    for index in 0..41 {
        let created = now + chrono::Duration::seconds(index);
        ids.push(
            bloglite::data::Post::insert(&ctx.conn().unwrap(), created, created, &content).unwrap(),
        );
    }
    // Deleted posts should not consume a page slot.
    bloglite::data::Post::delete(&ctx.conn().unwrap(), ids[20]).unwrap();
    let router = app(ctx.clone());
    for (uri, count, previous, next, first_id, last_id) in [
        ("/feed.xml", 20, false, true, ids[40], ids[21]),
        ("/feed.xml?page=1", 20, false, true, ids[40], ids[21]),
        ("/feed.xml?page=2", 20, true, false, ids[19], ids[0]),
    ] {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "application/rss+xml; charset=utf-8"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body.matches("<item>").count(), count);
        assert_eq!(body.contains("rel=\"previous\""), previous);
        assert_eq!(body.contains("rel=\"next\""), next);
        let self_url = if previous {
            "https://example.com/feed.xml?page=2"
        } else {
            "https://example.com/feed.xml"
        };
        assert!(body.contains(&format!("rel=\"self\" href=\"{self_url}\"")));
        if next {
            assert!(body.contains("rel=\"next\" href=\"https://example.com/feed.xml?page=2\""));
        }
        assert!(body.contains(&format!(
            "<guid>https://example.com/posts/{first_id}</guid>"
        )));
        assert!(body.contains(&format!("<guid>https://example.com/posts/{last_id}</guid>")));
        assert_eq!(body.matches("<strong>The end</strong>").count(), count);
        assert!(body.contains("<pre>  first\n\n    second</pre>"));
        assert!(body.contains("<!-- ]]]]><![CDATA[> -->"));
        assert!(!body.contains("Show more"));
    }
    for (page, status) in [
        ("0".to_string(), StatusCode::NOT_FOUND),
        ("3".to_string(), StatusCode::NOT_FOUND),
        (usize::MAX.to_string(), StatusCode::NOT_FOUND),
        ("abc".to_string(), StatusCode::BAD_REQUEST),
        ("-1".to_string(), StatusCode::BAD_REQUEST),
    ] {
        let req = Request::builder()
            .uri(format!("/feed.xml?page={page}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(router.clone().oneshot(req).await.unwrap().status(), status);
    }
    ctx.conn()
        .unwrap()
        .execute("DELETE FROM posts", [])
        .unwrap();
    let req = Request::builder()
        .uri("/feed.xml")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(!body.contains("<item>"));
    assert!(!body.contains("rel=\"next\""));
}

#[tokio::test]
async fn test_invalid_post_request() {
    let (status, _body) = request_body("/posts/foo").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_highlight_stylesheet() {
    let (status, css) = request_body("/static/highlight.css").await;
    assert_eq!(status, StatusCode::OK);
    assert!(css.contains(".syntax-code"));
    assert!(css.contains("prefers-color-scheme: dark"));
    assert!(css.contains(":root:not([data-theme='light'])"));
    let (_, body) = request_body("/posts/2/code").await;
    assert!(body.contains("syntax-code"));
    assert!(!body.contains("hljs"));
    assert!(!body.contains("highlight.min.js"));
    assert!(!body.contains("cdnjs.cloudflare.com"));
}
