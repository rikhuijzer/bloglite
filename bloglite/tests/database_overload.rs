#[allow(dead_code)]
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use bloglite::serve::app;
use http_body_util::BodyExt;
use std::time::{Duration, Instant};
use tower::ServiceExt;

#[tokio::test]
async fn exhausted_pool_returns_503_and_recovers() {
    let (ctx, auth) = common::request_cookie().await;
    let router = app(ctx.clone());
    let held: Vec<_> = (0..4).map(|_| ctx.pool.get().unwrap()).collect();
    assert!(ctx.pool.get_timeout(Duration::ZERO).is_err());

    // Exercise every database-using route family, including error-page rendering,
    // a write, and the backup endpoint's existing spawn_blocking path.
    for (method, uri) in [
        ("GET", "/"),
        ("GET", "/posts/1/lorem"),
        ("GET", "/search?q=Lorem"),
        ("GET", "/settings"),
        ("GET", "/files"),
        ("GET", "/files/unknown"),
        ("GET", "/feed.xml"),
        ("GET", "/sitemap.xml"),
        ("GET", "/login"),
        ("GET", "/does-not-exist"),
        ("GET", "/api/download/all.tar.xz"),
        ("PUT", "/api/settings/about"),
        ("POST", "/posts/delete/1"),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("Cookie", format!("auth={auth}"))
            .header("Authorization", "Bearer test-password")
            .body(Body::from("Updated about"))
            .unwrap();
        let start = Instant::now();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {uri}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{method} {uri} took too long"
        );
        assert_eq!(response.headers()["Retry-After"], "1");
        assert_eq!(response.headers()["Cache-Control"], "no-store");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"Server is busy. Please try again shortly.\n");
    }

    // A static asset remains available even when the database is exhausted.
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/static/style.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Once connections return, normal requests work and the rejected write has
    // not deleted the post.
    drop(held);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/posts/1/lorem")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
