//! Search at `/search`.
use crate::data::Kv;
use crate::data::Post;
use crate::data::SqliteDateTime;
use crate::html::PageSettings;
use crate::html::Top;
use crate::html::page;
use crate::html::wrap_post_content;
use crate::serve::RequestResult;
use crate::serve::ServerContext;
use crate::serve::content_type;
use crate::serve::is_logged_in;
use crate::serve::response;
use axum::Router;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::routing::get;
use axum_extra::extract::CookieJar;
use rusqlite::Result;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchForm {
    pub q: Option<String>,
}

fn search_form(q: &str) -> String {
    // Escape only the displayed value; the database receives the original query.
    let q = q
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    format!(
        "
        <form action='/search' method='get'>
            <input type='text' name='q' value='{q}' />
            <button type='submit'>Search</button>
        </form>
        "
    )
}

fn search(conn: &mut rusqlite::Connection, q: &str) -> Result<Vec<Post>> {
    if q.is_empty() {
        return Ok(vec![]);
    }
    // Dropping the transaction rolls back both the index and any failed query.
    let tx = conn.transaction()?;

    // Creating a virtual table on each query to avoid having to manually keep
    // track of updates to the fts table. For small sites, creating the index on
    // each query should be fine.
    //
    // Not copying updated since it's not shown in the preview.
    let stmt = "
        CREATE VIRTUAL TABLE posts_fts USING fts5(
            id,
            created,
            content,
            content=posts,
            tokenize=trigram
        );
        ";
    tx.execute(stmt, [])?;

    let stmt = "
        INSERT INTO posts_fts (id, created, content)
        SELECT id, created, content FROM posts;
    ";
    tx.execute(stmt, [])?;

    let mut results = tx.prepare("SELECT * FROM posts_fts WHERE posts_fts MATCH ?")?;
    let results = results
        .query_map([q], |row| {
            let id: i64 = row.get("id")?;
            let created: String = row.get("created")?;
            let content: String = row.get("content")?;
            let post = Post {
                id,
                created: SqliteDateTime::from_sqlite(&created),
                updated: SqliteDateTime::from_sqlite(&created),
                content,
            };
            Ok(post)
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

async fn get_search(
    State(ctx): State<ServerContext>,
    jar: CookieJar,
    search_query: Query<SearchForm>,
) -> RequestResult {
    let is_logged_in = is_logged_in(&ctx, &jar);
    let q = search_query.q.clone().unwrap_or_default();
    let search_form = search_form(&q);
    let results = {
        let mut conn = ctx.conn()?;
        search(&mut conn, &q)
    };
    let (status, message, mut results) = match results {
        Ok(results) => (StatusCode::OK, "", results),
        Err(rusqlite::Error::SqliteFailure(_, Some(ref message)))
            if message == "unterminated string"
                || message.starts_with("fts5: syntax error")
                || message.starts_with("no such column:") =>
        {
            (
                StatusCode::BAD_REQUEST,
                "Invalid search query syntax.",
                vec![],
            )
        }
        Err(error) => {
            tracing::error!(%error, "Search failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Search is temporarily unavailable.",
                vec![],
            )
        }
    };
    let results = results
        .iter_mut()
        .map(|p| {
            let slug = crate::md::extract_slug(p);
            crate::md::preview(p, 60);
            let is_front_page_preview = true;
            wrap_post_content(p, &slug, is_front_page_preview)
        })
        .collect::<Vec<_>>();
    let results = results.join("\n");
    let message = if message.is_empty() {
        String::new()
    } else {
        format!("<p>{message}</p>")
    };
    let body = format!(
        "
        {search_form}
        {message}
        {results}
        "
    );
    let mut headers = HeaderMap::new();
    content_type(&mut headers, "text/html");
    let title = "Search";
    let extra_head = &Kv::get_or_empty_string(&*ctx.conn()?, "extra_head");
    let settings = PageSettings::new(
        title,
        Some(is_logged_in),
        None,
        false,
        Top::GoHome,
        extra_head,
    );
    let body = page(&ctx, &settings, &body).await?;
    response(status, headers, body, &ctx)
}

pub fn routes(router: &Router<ServerContext>) -> Router<ServerContext> {
    router.clone().route("/search", get(get_search))
}
