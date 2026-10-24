//! API endpoints at `/api`.
use crate::data::Post;
use crate::files::File;
use crate::serve::ServerContext;
use crate::serve::response;
use crate::serve::response_json;
use crate::settings::Settings;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::Response;
use axum::http::StatusCode;
use axum::http::header::HeaderMap;
use axum::http::header::HeaderValue;
use axum::routing::get;
use axum::routing::put;
use serde_json::json;
use std::io::Read;
use subtle::ConstantTimeEq;
use tar::Builder;
use tar::Header;
use xz2::read::XzEncoder;

async fn get_api(State(ctx): State<ServerContext>) -> Response<Body> {
    let domain = ctx.base_url();
    let body = json!({
        "download_all_url": format!("{domain}/api/download/all.tar.xz"),
    })
    .to_string();
    response_json(StatusCode::OK, body, &ctx)
}

fn is_authenticated(ctx: &ServerContext, headers: &HeaderMap) -> bool {
    let header = if let Some(cookie) = headers.get("Authorization") {
        cookie
    } else {
        return false;
    };
    let parts = header
        .to_str()
        .unwrap()
        .split_ascii_whitespace()
        .collect::<Vec<&str>>();
    if parts.len() != 2 {
        return false;
    }
    if parts[0] != "Bearer" {
        return false;
    }
    let token = parts[1];
    token.as_bytes().ct_eq(ctx.password.as_bytes()).into()
}

fn error(ctx: &ServerContext, status: StatusCode, message: &str) -> Response<Body> {
    let body = json!({
        "status": status.as_u16(),
        "message": message,
    })
    .to_string();
    response_json(status, body, ctx)
}

fn unauthorized(ctx: &ServerContext) -> Response<Body> {
    error(ctx, StatusCode::UNAUTHORIZED, "unauthorized")
}

struct SiteData<'a> {
    posts: &'a [Post],
    settings: &'a Settings,
    files: &'a [File],
}

fn create_archive(site_data: &SiteData) -> Vec<u8> {
    let mut ar = Builder::new(Vec::new());

    for post in site_data.posts {
        let mut header = Header::new_gnu();
        let path = format!("posts/{}.md", post.id);
        header.set_path(&path).unwrap();
        // Without this, the file is not even readable by the user.
        header.set_mode(0o644);
        // Using `---` for the frontmatter because that is yaml and the GitHub
        // Markdown renderer supports it. `+++` is toml in Hugo but not
        // supported by the GitHub renderer.
        let content = indoc::formatdoc! {"
            ---
            created: '{}'
            updated: '{}'
            ---

            {}
        ", post.created, post.updated, post.content};
        let data = content.as_bytes();
        header.set_size(data.len() as u64);
        header.set_cksum();
        ar.append_data(&mut header, &path, data).unwrap();
    }

    let mut header = Header::new_gnu();
    // Putting it in `settings/` to be more consistent with the other files that
    // are also put in directories.
    let path = "settings/settings.toml";
    header.set_path(path).unwrap();
    header.set_mode(0o644);
    let data = toml::to_string(&site_data.settings).unwrap();
    let data = data.as_bytes();
    header.set_size(data.len() as u64);
    header.set_cksum();
    ar.append_data(&mut header, path, data).unwrap();

    for file in site_data.files {
        let mut header = Header::new_gnu();
        let path = format!("files/{}", file.filename);
        header.set_path(&path).unwrap();
        header.set_mode(0o644);
        let data = file.data.as_ref();
        header.set_size(data.len() as u64);
        header.set_cksum();
        ar.append_data(&mut header, &path, data).unwrap();
    }

    ar.into_inner().unwrap()
}

fn compress(data: &[u8]) -> Vec<u8> {
    let mut compressor = XzEncoder::new(data, 6);
    let mut compressed = Vec::new();
    compressor.read_to_end(&mut compressed).unwrap();
    compressed
}

fn download_all(ctx: &ServerContext) -> Response<Body> {
    let conn = ctx.conn();
    let posts = Post::list(&conn);
    let posts = if let Ok(posts) = posts {
        posts
    } else {
        return error(
            ctx,
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to get posts",
        );
    };
    let settings = Settings::from_db(&conn);
    let settings = if let Ok(settings) = settings {
        settings
    } else {
        return error(
            ctx,
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to get settings",
        );
    };
    let files = File::list(&conn);
    drop(conn);
    let files = if let Ok(files) = files {
        files
    } else {
        return error(
            ctx,
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to get files",
        );
    };
    let site_data = SiteData {
        posts: &posts,
        settings: &settings,
        files: &files,
    };
    let data = create_archive(&site_data);
    let body = compress(&data);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/octet-stream"),
    );
    response::<Vec<u8>>(StatusCode::OK, headers, body, ctx)
}

async fn get_download_all(State(ctx): State<ServerContext>, headers: HeaderMap) -> Response<Body> {
    if !is_authenticated(&ctx, &headers) {
        return unauthorized(&ctx);
    }
    tokio::task::spawn_blocking({
        let ctx = ctx.clone();
        move || download_all(&ctx)
    })
    .await
    .unwrap()
}

async fn update_about(
    State(ctx): State<ServerContext>,
    headers: HeaderMap,
    body: String,
) -> Response<Body> {
    if !is_authenticated(&ctx, &headers) {
        return unauthorized(&ctx);
    }
    let settings = Settings::from_db(&ctx.conn());
    if let Ok(settings) = settings {
        // Avoid update and backup trigger when no change to avoid infinite loop.
        if settings.about.trim() == body.trim() {
            tracing::info!("ignoring about update because no change");
            return response_json(StatusCode::OK, "ok", &ctx);
        }
    }
    let about = Settings::set_about(&ctx.conn(), &body);
    if let Err(e) = about {
        return error(
            &ctx,
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to update about: {e}"),
        );
    }
    crate::trigger::trigger_github_backup(&ctx).await;
    response_json(StatusCode::OK, "ok", &ctx)
}

pub fn routes(router: &Router<ServerContext>) -> Router<ServerContext> {
    router
        .clone()
        .route("/api", get(get_api))
        .route("/api/download/all.tar.xz", get(get_download_all))
        .route("/api/settings/about", put(update_about))
}
