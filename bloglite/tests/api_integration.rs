mod common;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use bloglite::serve::ServerContext;
use bloglite::serve::app;
use chrono::Utc;
use common::*;
use http_body_util::BodyExt;
use std::io::Cursor;
use std::io::Read;
use tar::Archive;
use tar::Entry;
use tower::util::ServiceExt;
use xz2::read::XzDecoder;

#[tokio::test]
async fn test_api() {
    let (status, _body) = request_body("/api").await;
    assert_eq!(status, StatusCode::OK);
}

fn auth_header(ctx: &ServerContext) -> String {
    let auth = ctx.password.clone();
    format!("Bearer {auth}")
}

pub async fn request_body_authenticated(uri: &str) -> (StatusCode, Vec<u8>) {
    let ctx = server_context().await;
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("Authorization", auth_header(&ctx))
        .body(Body::empty())
        .unwrap();
    let response = app(ctx.clone()).oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap();
    let body: Vec<u8> = body.to_bytes().into();
    (status, body)
}

#[tokio::test]
async fn test_update_about() {
    let ctx = server_context().await;
    let uri = "/api/settings/about";
    let body = "test";
    let req = Request::builder()
        .method("PUT")
        .uri(uri)
        .header("Authorization", auth_header(&ctx))
        .body(Body::from(body))
        .unwrap();
    let router = app(ctx.clone());
    let response = router.oneshot(req).await.unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_download_all() {
    let uri = "/api/download/all.tar.xz";
    let (status, body) = request_body_authenticated(uri).await;
    assert_eq!(status, StatusCode::OK);

    let body = Cursor::new(body);
    let decompressed = XzDecoder::new(body);
    let mut ar = Archive::new(decompressed);
    // Do not collect the entries because it moves the stream pointer.
    let mut entries = ar.entries().unwrap();
    fn path<T: std::io::Read>(entry: &Entry<T>) -> String {
        entry.path().unwrap().to_str().unwrap().to_string()
    }
    let mut first = entries.next().unwrap().unwrap();
    // SQLite is 1-indexed.
    assert!(path(&first).contains("posts/1.md"));

    let mut content = String::new();
    first.read_to_string(&mut content).unwrap();
    println!("content:\n{content}");
    let lines = content.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "---");
    let today = Utc::now().format("%Y-%m-%d").to_string();
    assert!(lines[1].contains(&format!("created: '{today}")));
    assert!(lines[2].contains(&format!("updated: '{today}")));
    assert_eq!(lines[3], "---");
    assert!(lines[4].is_empty());
    assert!(lines[5].contains("Lorem"));

    let second = entries.next().unwrap().unwrap();
    assert!(path(&second).contains("posts/2.md"));

    let mut third = entries.next().unwrap().unwrap();
    assert!(path(&third).contains("settings/settings.toml"));
    let mut content = String::new();
    third.read_to_string(&mut content).unwrap();
    println!("settings.toml content:\n{content}");
    assert!(content.contains(r#"author_name = "John""#));

    let mut fourth = entries.next().unwrap().unwrap();
    assert!(path(&fourth).contains("files/example.txt"));
    let mut content = String::new();
    fourth.read_to_string(&mut content).unwrap();
    assert_eq!(content, "example");

    assert!(entries.next().is_none());
}
