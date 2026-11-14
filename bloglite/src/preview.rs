use crate::ServeArgs;
use crate::data::{DbPool, Kv, Post, cleanup_content, init_site_settings};
use crate::html::{PageSettings, Top, page};
use crate::serve::{
    RequestResult, ServerContext, content_type, prepare_new_post_content, render_post,
    shutdown_signal, static_routes,
};
use axum::Extension;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::get;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::io;
use std::net::Ipv4Addr;
use std::path::PathBuf;

#[derive(Clone)]
struct Draft {
    path: PathBuf,
    created: DateTime<Utc>,
}

impl Draft {
    async fn read(&self) -> io::Result<String> {
        tokio::fs::read_to_string(&self.path).await
    }
}

fn revision(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

async fn get_revision(Extension(draft): Extension<Draft>) -> impl IntoResponse {
    let revision = match draft.read().await {
        Ok(content) => revision(&content),
        Err(_) => "unavailable".to_string(),
    };
    ([(header::CACHE_CONTROL, "no-store")], revision)
}

async fn get_preview(
    State(ctx): State<ServerContext>,
    Extension(draft): Extension<Draft>,
) -> RequestResult {
    let content = draft.read().await;
    let revision = match &content {
        Ok(content) => revision(content),
        Err(_) => "unavailable".to_string(),
    };
    let reload = format!(
        r#"<script>
        async function checkDraft() {{
            try {{
                const response = await fetch('/_preview/revision');
                if (response.ok && (await response.text()) !== '{revision}') {{
                    window.location.reload();
                    return;
                }}
            }} catch {{
                // Try again if the preview server is temporarily unreachable.
            }}
            window.setTimeout(checkDraft, 1000);
        }}
        window.setTimeout(checkDraft, 1000);
        </script>"#
    );
    let mut response = match content {
        Ok(content) => {
            let post = Post {
                id: 1,
                created: draft.created,
                updated: draft.created,
                content: cleanup_content(&prepare_new_post_content(&content)),
            };
            render_post(&ctx, &post, None, &reload).await?
        }
        Err(_) => {
            let settings =
                PageSettings::new("Draft unavailable", None, None, false, Top::GoHome, &reload);
            let body = page(&ctx, &settings, "<p>Could not read the draft as UTF-8. This page will reload when the draft is available.</p>").await?;
            (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
        }
    };
    content_type(response.headers_mut(), "text/html; charset=utf-8");
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok(response)
}

fn files_origin(host: &str) -> io::Result<String> {
    let invalid_host = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "--host must be a hostname or an HTTP or HTTPS origin, such as blog.example.com",
        )
    };
    let url = if host.contains("://") {
        reqwest::Url::parse(host)
    } else {
        reqwest::Url::parse(&format!("https://{host}"))
    }
    .map_err(|_| invalid_host())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid_host());
    }
    Ok(url.origin().ascii_serialization())
}

pub async fn run(
    path: PathBuf,
    port: u16,
    file_host: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_host = file_host.map(files_origin).transpose()?;
    let draft = Draft {
        path,
        created: Utc::now(),
    };
    draft.read().await.map_err(|err| {
        io::Error::new(
            err.kind(),
            format!("Could not read {}: {err}", draft.path.display()),
        )
    })?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let addr = listener.local_addr()?;

    let pool = DbPool::memory(1)?;
    {
        let conn = pool.get()?;
        Kv::create_table(&conn)?;
        init_site_settings(&conn, "", "localhost");
    }
    let ctx = ServerContext::new(
        ServeArgs {
            production: false,
            port: addr.port(),
            database_path: String::new(),
            username: String::new(),
            password: None,
            password_file: None,
            domain: String::new(),
            html_lang: "en".into(),
            log_level: "info".into(),
            trigger_token: None,
            trigger_owner_repo: None,
            trigger_host: String::new(),
            trigger_branch: String::new(),
            trigger_workflow_id: String::new(),
        },
        String::new(),
        pool,
        bloglite_auth::generate_salt(),
    )
    .await;

    let expected_host = addr.to_string();
    let localhost = format!("localhost:{}", addr.port());
    let app = static_routes()
        .route("/", get(get_preview))
        .route("/posts/1", get(get_preview))
        .route("/posts/1/{slug}", get(get_preview))
        .route("/_preview/revision", get(get_revision));
    let app = if let Some(origin) = file_host {
        app.route(
            "/files/{*path}",
            get(move |uri: Uri| async move {
                let Some(path) = uri.path_and_query() else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                match HeaderValue::try_from(format!("{origin}{path}")) {
                    Ok(location) => (
                        StatusCode::TEMPORARY_REDIRECT,
                        [(header::LOCATION, location)],
                    )
                        .into_response(),
                    Err(_) => StatusCode::BAD_REQUEST.into_response(),
                }
            }),
        )
    } else {
        app
    };
    let app = app
        .layer(Extension(draft))
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            // Keep other hostnames from reaching the draft via DNS rebinding.
            let host = request.headers().get(header::HOST);
            let valid_host = host
                .is_some_and(|host| host == expected_host.as_str() || host == localhost.as_str());
            async move {
                if valid_host {
                    next.run(request).await
                } else {
                    StatusCode::FORBIDDEN.into_response()
                }
            }
        }))
        .with_state(ctx);
    println!("Previewing at http://{addr}/");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}
