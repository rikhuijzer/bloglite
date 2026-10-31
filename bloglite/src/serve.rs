use crate::ServeArgs;
use crate::data;
use crate::data::DbPool;
use crate::data::Kv;
use crate::data::Post;
use crate::html::PageSettings;
use crate::html::Top;
use crate::html::page;
use crate::html::wrap_post_content;
use crate::pool::PooledConnection;
use axum::Form;
use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Response;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::routing::get;
use axum::routing::post;
use axum_extra::extract::CookieJar;
use bloglite_auth::Login;
use bloglite_auth::Salt;
use chrono::Utc;
use http_body_util::BodyExt;
use rusqlite::Connection;
use serde::Deserialize;
use serde::Serialize;

pub type RequestResult<T = Response<Body>> = Result<T, DatabaseUnavailable>;

#[derive(Debug)]
pub struct DatabaseUnavailable;

impl IntoResponse for DatabaseUnavailable {
    fn into_response(self) -> Response<Body> {
        // This response must not render the normal layout: that also needs SQLite.
        (
            StatusCode::SERVICE_UNAVAILABLE,
            [("Retry-After", "1"), ("Cache-Control", "no-store")],
            "Server is busy. Please try again shortly.\n",
        )
            .into_response()
    }
}

#[derive(Clone)]
pub struct ServerContext {
    pub args: ServeArgs,
    pub password: String,
    pub pool: DbPool,
    pub salt: Salt,
}

impl ServerContext {
    pub async fn new(args: ServeArgs, password: String, pool: DbPool, salt: Salt) -> Self {
        Self {
            args,
            password,
            pool,
            salt,
        }
    }
    /// Request handlers fail with 503 if no connection becomes free within 250 ms.
    pub fn conn(&self) -> RequestResult<PooledConnection> {
        self.pool
            .get_timeout(std::time::Duration::from_millis(250))
            .map_err(|error| {
                tracing::warn!(%error, "Database connection unavailable");
                DatabaseUnavailable
            })
    }
    /// Returns the base URL of the server.
    ///
    /// For example, if the domain is "example.com", the base URL will be
    /// "https://example.com".
    pub fn base_url(&self) -> String {
        if self.args.domain.is_empty() {
            "".to_string()
        } else {
            let domain = &self.args.domain;
            let domain = domain.trim();
            let domain = domain.trim_end_matches('/');
            format!("https://{domain}")
        }
    }
}

pub fn response<D: Sized>(
    status: StatusCode,
    headers: HeaderMap,
    body: D,
    ctx: &ServerContext,
) -> RequestResult
where
    Body: From<D>,
{
    let mut response: Response<Body> = Response::default();
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    // Prevent framing the page in an iframe.
    response
        .headers_mut()
        .insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
    if ctx.args.production {
        response.headers_mut().insert(
            "Strict-Transport-Security",
            HeaderValue::from_static("max-age=604800; preload"), // 1 week.
        );
    }
    *response.body_mut() = Body::from(body);
    Ok(response)
}

pub async fn error(
    ctx: &ServerContext,
    status: StatusCode,
    title: &str,
    msg: &str,
) -> RequestResult {
    let body = msg.to_string();
    let headers = HeaderMap::new();
    let settings = PageSettings::new(title, None, None, false, Top::GoHome, "");
    let body = format!(
        "
        <div style='text-align: center;'>
            <h1>{title}</h1>
            <p>{body}</p>
        </div>
        "
    );
    let body = page(ctx, &settings, &body).await?;
    response(status, headers, body, ctx)
}

pub async fn internal_server_error(ctx: &ServerContext, msg: &str) -> RequestResult {
    error(
        ctx,
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal Server Error",
        msg,
    )
    .await
}

pub async fn unauthorized(ctx: &ServerContext) -> RequestResult {
    error(
        ctx,
        StatusCode::UNAUTHORIZED,
        "Unauthorized",
        "Not logged in",
    )
    .await
}

pub fn response_json<D>(status: StatusCode, body: D, ctx: &ServerContext) -> RequestResult
where
    D: serde::Serialize,
    Body: From<D>,
{
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    response(status, headers, body, ctx)
}

pub fn is_logged_in(ctx: &ServerContext, jar: &CookieJar) -> bool {
    let login = Login {
        username: Some(ctx.args.username.clone()),
        password: Some(ctx.password.clone()),
    };
    bloglite_auth::is_logged_in(&ctx.salt, &login, jar)
}

async fn list_posts(ctx: &ServerContext, page: usize) -> RequestResult<Option<(bool, String)>> {
    let posts = match Post::list(&*ctx.conn()?) {
        Ok(posts) => posts,
        Err(_) => return Ok(Some((false, "Database error".to_string()))),
    };
    // Set this to 1 to test the logic locally.
    let results_per_page = 10;
    let Some(start) = page
        .checked_sub(1)
        .and_then(|p| p.checked_mul(results_per_page))
    else {
        return Ok(None);
    };
    // The first page remains valid even when there are no posts.
    if page > 1 && start >= posts.len() {
        return Ok(None);
    }
    let Some(end) = start.checked_add(results_per_page) else {
        return Ok(None);
    };
    let has_next = end < posts.len();
    let end = std::cmp::min(end, posts.len());
    let mut posts = posts[start..end].to_vec();
    let posts = posts
        .iter_mut()
        .map(|post| {
            let slug = crate::md::extract_slug(post);
            crate::md::preview(post, 600);
            wrap_post_content(post, &slug, true)
        })
        .collect::<Vec<String>>();
    Ok(Some((has_next, posts.join("\n"))))
}

#[derive(Debug, Deserialize)]
struct Pagination {
    /// One-based page number.
    ///
    /// One-based since this index is visible to readers who are probably more
    /// familiar with one-based numbering.
    page: Option<usize>,
}

async fn get_posts(
    State(ctx): State<ServerContext>,
    jar: CookieJar,
    pagination: Query<Pagination>,
) -> RequestResult {
    let is_logged_in = Some(is_logged_in(&ctx, &jar));
    let show_about = pagination.page.is_none();
    let current_page = pagination.page.unwrap_or(1);
    let extra_head = Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let extra_head = format!(
        "
        <meta property='og:type' content='website'/>
        {}
        ",
        extra_head
    );
    let top = if show_about {
        Top::Homepage
    } else {
        Top::GoHome
    };
    let settings = PageSettings::new("", is_logged_in, None, show_about, top, &extra_head);
    let Some((has_next, posts)) = list_posts(&ctx, current_page).await? else {
        return not_found(State(ctx)).await;
    };
    let prev_link = if current_page == 1 {
        ""
    } else {
        let prev_page = current_page - 1;
        let href = if prev_page == 1 {
            "/"
        } else {
            &format!("/?page={prev_page}")
        };
        &format!("<a class='unstyled-link' href='{href}'>◀ prev</a>")
    };
    let next_link = if has_next {
        let next_page = current_page + 1;
        &format!("<a class='unstyled-link' href='/?page={next_page}'>▶ next</a>")
    } else {
        ""
    };
    let body = &format!(
        "
        {posts}
        <div style='display: flex; justify-content: space-between;'>
            <p>
                {prev_link}
            </p>
            <p>
                {next_link}
            </p>
        </div>
        "
    );
    let body = page(&ctx, &settings, body).await?;
    tracing::info!("\"GET / HTTP/1.1\" 200");
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

pub fn content_type(headers: &mut HeaderMap, content_type: &str) {
    let val = HeaderValue::from_str(content_type).unwrap();
    headers.insert("Content-Type", val);
}

pub fn enable_caching(headers: &mut HeaderMap, max_age: u32) {
    // `must-revalidate` avoids stale responses when disconnected.
    let src = format!("public, max-age={max_age}, must-revalidate");
    let val = HeaderValue::from_str(&src).unwrap();
    headers.insert(axum::http::header::CACHE_CONTROL, val);
}

async fn get_style(State(ctx): State<ServerContext>) -> RequestResult {
    let body = crate::html::minify(&format!(
        "{}\n{}",
        include_str!("static/style.css"),
        include_str!("static/vendor/mathml.css")
    ));
    let mut headers = HeaderMap::new();
    content_type(&mut headers, "text/css");
    enable_caching(&mut headers, 600);
    response(StatusCode::OK, headers, body, &ctx)
}

async fn get_script(State(ctx): State<ServerContext>) -> RequestResult {
    let body = crate::html::minify(include_str!("static/script.js"));
    let mut headers = HeaderMap::new();
    content_type(&mut headers, "text/javascript");
    enable_caching(&mut headers, 600);
    response(StatusCode::OK, headers, body, &ctx)
}

async fn get_math_font() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "font/woff2"),
            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("static/vendor/LibertinusMath-Regular.woff2").as_slice(),
    )
}

async fn get_highlight_style() -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/css"),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=600, must-revalidate",
            ),
        ],
        crate::highlight::stylesheet(),
    )
}

async fn get_nodefer(State(ctx): State<ServerContext>) -> RequestResult {
    let body = crate::html::minify(include_str!("static/nodefer.js"));
    let mut headers = HeaderMap::new();
    content_type(&mut headers, "text/javascript");
    enable_caching(&mut headers, 600);
    response(StatusCode::OK, headers, body, &ctx)
}

async fn get_delete(
    State(ctx): State<ServerContext>,
    Path(id): Path<i64>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return not_found(State(ctx.clone())).await;
    }
    let post = Post::get(&*ctx.conn()?, id);
    let post = match post {
        Ok(post) => post,
        Err(_) => return not_found(State(ctx.clone())).await,
    };
    let extra_head = &Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let title = crate::md::extract_html_title(&post);
    let settings = PageSettings::new(
        &title,
        Some(is_logged_in),
        None,
        false,
        Top::GoHome,
        extra_head,
    );
    let delete_button = indoc::formatdoc! {r#"
        <div class='medium-text' style='text-align: center; font-weight: bold;'>
            <p>Are you sure you want to delete this post? This action cannot be undone.</p>
            <form action='/posts/delete/{id}' method='post'>
                <button type='submit'>delete</button>
            </form>
            <br>
        </div>
    "#};
    let body = format!("{}\n{}", delete_button, wrap_post_content(&post, "", false));
    let body = page(&ctx, &settings, &body).await?;
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

async fn get_edit(
    State(ctx): State<ServerContext>,
    Path(id): Path<i64>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    let post = Post::get(&*ctx.conn()?, id);
    let post = match post {
        Ok(post) => post,
        Err(_) => return not_found(State(ctx)).await,
    };
    let title = crate::md::extract_html_title(&post);
    let title = format!("Edit '{title}'");
    let body = crate::html::edit_post_form(&post);
    let extra_head = Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let settings = PageSettings::new(
        &title,
        Some(is_logged_in),
        None,
        false,
        Top::GoBack,
        &extra_head,
    );
    let body = page(&ctx, &settings, &body).await?;
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

fn iso8601(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

async fn get_post_with_slug(
    State(ctx): State<ServerContext>,
    Path((id, _slug)): Path<(i64, String)>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    let post = Post::get(&*ctx.conn()?, id);
    let post = match post {
        Ok(post) => post,
        Err(_) => return not_found(State(ctx)).await,
    };
    if post.is_deleted() {
        return not_found(State(ctx)).await;
    }
    let title = crate::md::extract_html_title(&post);
    let author = Kv::get(&*ctx.conn()?, "author_name").unwrap();
    let author = String::from_utf8(author).unwrap();
    // Open Graph uses ISO 8601 according to <https://ogp.me/>.
    let created = iso8601(&post.created);
    let updated = iso8601(&post.updated);
    let slug = crate::md::slug_from_title(&title);
    let canonical = format!("{}/posts/{}/{slug}", ctx.base_url(), post.id);
    let extra_head = Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let extra_head = indoc::formatdoc! {r#"
        <meta property='article:author' content='{author}'/>
        <meta property='article:published_time' content='{created}'/>
        <meta property='article:modified_time' content='{updated}'/>
        <meta property='og:url' content='{canonical}'/>
        <meta property='og:type' content='article'/>
        <link rel='canonical' href='{canonical}'/>
        {}
    "#, &extra_head};
    let description = crate::md::extract_html_description(&post);
    let settings = PageSettings::new(
        &title,
        Some(is_logged_in),
        Some(&description),
        false,
        Top::GoHome,
        &extra_head,
    );
    let mut body = wrap_post_content(&post, &slug, false);
    if is_logged_in {
        body = format!("{}\n{body}", crate::html::edit_post_buttons(&ctx, &post));
    }
    let body = page(&ctx, &settings, &body).await?;
    // Can safely assume HTTP/1.1 because we're not handling TLS.
    tracing::info!("\"GET /posts/{id} HTTP/1.1\" 200");
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

async fn get_post(
    State(ctx): State<ServerContext>,
    Path(id): Path<String>,
    jar: CookieJar,
) -> RequestResult {
    let id = match id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => return not_found(State(ctx)).await,
    };
    let post = Post::get(&*ctx.conn()?, id);
    let post = match post {
        Ok(post) => post,
        Err(_) => return not_found(State(ctx)).await,
    };
    if post.is_deleted() {
        return not_found(State(ctx)).await;
    }
    let slug = crate::md::extract_slug(&post);
    // Slug can be empty when all slug characters have been stripped.
    if slug.is_empty() {
        return get_post_with_slug(State(ctx), Path((id, slug)), jar).await;
    }
    let url = crate::html::post_link(&post, &slug);
    // Same behavior as Reddit. Any slug is accepted and then redirected to the
    // right page. I couldn't figure out the Reddit status code, but permanent
    // redirect seems suitable.
    let mut headers = HeaderMap::new();
    let loc = match HeaderValue::from_str(&url) {
        Ok(loc) => loc,
        Err(e) => {
            tracing::error!("Failed to set redirect to '{url}': {e}");
            return response(
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                format!("Failed to set redirect URL to '{url}'"),
                &ctx,
            );
        }
    };
    headers.insert("Location", loc);
    response(StatusCode::PERMANENT_REDIRECT, headers, "", &ctx)
}

pub async fn not_found(State(ctx): State<ServerContext>) -> RequestResult {
    let body = indoc::indoc! {"
        <div style='text-align: center; margin-top: 100px;'>
            <h1>Not found</h1>
            <p>The page you are looking for does not exist.</p>
        </div>
    "};
    let extra_head = Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let settings = PageSettings::new("not found", None, None, false, Top::GoHome, &extra_head);
    let body = page(&ctx, &settings, body).await?;
    response::<String>(StatusCode::NOT_FOUND, HeaderMap::new(), body, &ctx)
}

fn is_https(headers: &HeaderMap) -> bool {
    if let Some(proto) = headers.get("X-Forwarded-Proto") {
        return proto == "https";
    }
    false
}

async fn get_login(State(ctx): State<ServerContext>, headers: HeaderMap) -> RequestResult {
    let error = if ctx.args.production && !is_https(&headers) {
        Some(
            "Did not find X-Forwarded-Proto header which suggests the request is not over HTTPS. Login will probably fail. Use a reverse proxy like Caddy with HTTPS to fix this.",
        )
    } else {
        None
    };
    let body = crate::html::login(&ctx, error).await?;
    tracing::info!("\"GET /login HTTP/1.1\" 200");
    response::<String>(StatusCode::OK, HeaderMap::new(), body, &ctx)
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

async fn post_login(
    State(ctx): State<ServerContext>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> RequestResult {
    let actual = Login {
        username: Some(ctx.args.username.clone()),
        password: Some(ctx.password.clone()),
    };
    let received = Login {
        username: Some(form.username),
        password: Some(form.password),
    };
    let new_jar = bloglite_auth::handle_login(&ctx.salt, &actual, &received, jar.clone());
    match new_jar {
        Some(jar) => {
            tracing::info!("\"POST /login HTTP/1.1\" 200");
            Ok((jar, Redirect::to("/")).into_response())
        }
        None => {
            let body = crate::html::login(&ctx, Some("Invalid username or password"));
            tracing::info!("\"POST /login HTTP/1.1\" 401");
            response::<String>(
                StatusCode::UNAUTHORIZED,
                HeaderMap::new(),
                body.await?,
                &ctx,
            )
        }
    }
}

async fn get_logout(State(_ctx): State<ServerContext>, jar: CookieJar) -> (CookieJar, Redirect) {
    let updated_jar = bloglite_auth::handle_logout(jar.clone());
    tracing::info!("\"GET /logout HTTP/1.1\" 200");
    (updated_jar, Redirect::to("/"))
}

async fn post_delete(
    State(ctx): State<ServerContext>,
    Path(id): Path<i64>,
    jar: CookieJar,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return response(
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            "Unauthorized",
            &ctx,
        );
    }
    Post::delete(&*ctx.conn()?, id).unwrap();
    crate::trigger::trigger_github_backup(&ctx).await;
    Ok(Redirect::to("/").into_response())
}

#[derive(Debug, Deserialize, Serialize)]
pub struct EditPostForm {
    pub content: String,
}

/// Return a 303 redirect to the given url.
///
/// This is used after a `POST` request to indicate that the resource has been
/// updated and the client should fetch the updated resource with a `GET`
/// request.
pub fn see_other(ctx: &ServerContext, url: &str) -> RequestResult {
    let mut headers = HeaderMap::new();
    let dst = HeaderValue::from_str(url).unwrap();
    headers.insert("Location", dst);
    response(StatusCode::SEE_OTHER, headers, "", ctx)
}

/// Trim a given string and ensure it ends with a newline.
pub fn trim_newline_suffix(s: &str) -> String {
    format!("{}\n", s.trim())
}

async fn post_edit(
    State(ctx): State<ServerContext>,
    jar: CookieJar,
    Path(id): Path<i64>,
    req: Request,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return not_found(State(ctx)).await;
    }
    let extra_head = &Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let settings = PageSettings::new("", Some(is_logged_in), None, false, Top::GoBack, extra_head);
    let (_, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|_err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read request body",
            )
                .into_response()
        })
        .unwrap()
        .to_bytes();
    let bytes = bytes.to_vec();
    let input = String::from_utf8(bytes).unwrap();
    let publish = input.contains("publish=Publish");
    let form = serde_urlencoded::from_str::<EditPostForm>(&input).unwrap();
    let created = match Post::get(&*ctx.conn()?, id) {
        Ok(post) => post.created,
        Err(_) => Utc::now(),
    };
    let post = Post {
        id,
        created,
        updated: Utc::now(),
        content: trim_newline_suffix(&form.content),
    };
    if publish {
        let post = post.update(&*ctx.conn()?);
        if post.is_err() {
            return response(
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                format!("Failed to update post: {}", post.err().unwrap()),
                &ctx,
            );
        };
        let url = format!("/posts/{}", id);
        crate::trigger::trigger_github_backup(&ctx).await;
        see_other(&ctx, &url)
    } else {
        let preview = crate::html::wrap_post_content(&post, "", false);
        let body = page(&ctx, &settings, &preview).await?;
        response(StatusCode::OK, HeaderMap::new(), body, &ctx)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AddPostForm {
    pub content: String,
}

/// H2-H6 headings (for example, `## Heading`) cause problems since the hash
/// ends up into the Location header and handled incorrectly by the browser.  To
/// solve, we just replace multiple hashes at the start of the string with a
/// single hash.
fn fix_invalid_heading_issue_179(content: &str) -> String {
    let number_of_hashes = content.chars().take_while(|c| *c == '#').count();
    if number_of_hashes == 0 {
        return content.to_string();
    }
    format!("#{}", &content[number_of_hashes..])
}

#[test]
fn test_fix_invalid_heading_issue_179_reduces() {
    let content = "### Heading";
    let fixed = fix_invalid_heading_issue_179(content);
    assert_eq!(fixed, "# Heading");
}

#[test]
fn test_fix_invalid_heading_issue_179_does_not_always_add_hash() {
    let content = "Heading";
    let fixed = fix_invalid_heading_issue_179(content);
    assert_eq!(fixed, "Heading");
}

async fn post_add(State(ctx): State<ServerContext>, jar: CookieJar, req: Request) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    if !is_logged_in {
        return not_found(State(ctx)).await;
    }
    let extra_head = &Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let settings = PageSettings::new("", Some(is_logged_in), None, false, Top::GoBack, extra_head);
    let (_, body) = req.into_parts();
    let bytes = body
        .collect()
        .await
        .map_err(|_err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read request body",
            )
                .into_response()
        })
        .unwrap()
        .to_bytes();
    let bytes = bytes.to_vec();
    let input = String::from_utf8(bytes).unwrap();
    let publish = input.contains("publish=Publish");
    let form = serde_urlencoded::from_str::<AddPostForm>(&input).unwrap();
    if publish {
        let now = Utc::now();
        let content = trim_newline_suffix(&form.content);
        let content = fix_invalid_heading_issue_179(&content);
        let post_id = Post::insert(&*ctx.conn()?, now, now, &content);
        if let Err(_e) = post_id {
            return response(
                StatusCode::INTERNAL_SERVER_ERROR,
                HeaderMap::new(),
                "Failed to insert post",
                &ctx,
            );
        };
        let url = "/?reset_forms=true";
        crate::trigger::trigger_github_backup(&ctx).await;
        see_other(&ctx, url)
    } else {
        let post = Post {
            id: 0,
            created: Utc::now(),
            updated: Utc::now(),
            content: form.content,
        };
        let is_front_page_preview = false;
        let preview = crate::html::wrap_post_content(&post, "", is_front_page_preview);
        let body = page(&ctx, &settings, &preview).await?;
        response(StatusCode::OK, HeaderMap::new(), body, &ctx)
    }
}

async fn get_webfinger(State(ctx): State<ServerContext>) -> RequestResult {
    let body = crate::ap::webfinger(&ctx).await;
    let body = match body {
        Some(body) => body,
        None => return not_found(State(ctx)).await,
    }
    .to_string();
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/jrd+json; charset=utf-8"),
    );
    response::<String>(StatusCode::OK, headers, body, &ctx)
}

pub fn app(ctx: ServerContext) -> Router {
    let router = Router::new()
        .route("/", get(get_posts))
        .route("/posts/delete/{id}", get(get_delete))
        .route("/posts/delete/{id}", post(post_delete))
        .route("/posts/edit/{id}", get(get_edit))
        .route("/posts/edit/{id}", post(post_edit))
        .route("/posts/add", post(post_add))
        .route("/posts/{id}", get(get_post))
        .route("/posts/{id}/", get(get_post))
        .route("/posts/{id}/{slug}", get(get_post_with_slug))
        .route("/posts/{id}/{slug}/", get(get_post_with_slug))
        .route("/login", get(get_login))
        .route("/login", post(post_login))
        .route("/logout", get(get_logout))
        .route("/static/highlight.css", get(get_highlight_style))
        .route("/static/style.css", get(get_style))
        .route("/static/LibertinusMath-Regular.woff2", get(get_math_font))
        .route("/static/script.js", get(get_script))
        .route("/static/nodefer.js", get(get_nodefer))
        .route("/.well-known/webfinger", get(get_webfinger));
    let router = crate::api::routes(&router);
    let router = crate::discovery::routes(&router);
    let router = crate::files::routes(&router);
    let router = crate::search::routes(&router);
    let router = crate::settings::routes(&router);
    let router = router.fallback(not_found);
    // Files larger than this will be rejected during upload.
    let limit = 15 * 1024 * 1024;
    router.with_state(ctx).layer(DefaultBodyLimit::max(limit))
}

/// Return the salt by either generating a new one or reading it from the db.
///
/// Re-using the salt between sessions allows users to keep logged in even when
/// the server restarts.
fn obtain_salt(args: &ServeArgs, conn: &Connection) -> Salt {
    if args.production {
        let salt = data::Kv::get(conn, "salt");
        match salt {
            Ok(salt) => salt.try_into().unwrap(),
            Err(_) => {
                let salt = bloglite_auth::generate_salt();
                data::Kv::insert(conn, "salt", &salt).unwrap();
                salt
            }
        }
    } else {
        // Allow the login to persist across restarts.
        b"nblVMlxYtvt0rxo3BML3zw".to_owned()
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("Shutdown signal received; finishing active requests");
}

pub async fn run(args: &ServeArgs, password: String) {
    let pool = data::connect(args).unwrap();
    let conn = pool.get().unwrap();
    data::init(args, &conn);
    let salt = obtain_salt(args, &conn);
    drop(conn);
    let ctx = ServerContext::new(args.clone(), password, pool, salt).await;
    let app = app(ctx);
    // Listen on both IPv4 and IPv6. This seems to not be necessary behind the
    // Caddy reverse proxy for IPv6 to work, but could probably be useful for
    // other situations.
    let addr = format!("[::]:{}", args.port);
    let addr = addr.parse::<std::net::SocketAddr>().unwrap();
    tracing::info!("Listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}
