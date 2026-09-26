//! Serves the web UI: `frontend/dist` (built on a dev machine with `npm run
//! build` and committed) is embedded in the binary at compile time, so the
//! board needs no Node and the UI works offline. Unknown non-file paths get
//! `index.html` so client-side routes (/sleep, /last-night) load the app.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use super::ApiError;

#[derive(RustEmbed)]
#[folder = "../frontend/dist"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return ApiError::NotFound("no such API endpoint").into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Assets::get(path) {
        return file_response(path, file);
    }
    // A path that looks like a file (has an extension) and wasn't found is a 404;
    // anything else is a client-side route.
    let looks_like_file = path.rsplit('/').next().is_some_and(|name| name.contains('.'));
    match (looks_like_file, Assets::get("index.html")) {
        (false, Some(index)) => file_response("index.html", index),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn file_response(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    // Vite puts content-hashed files under assets/, so they can be cached forever.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        file.data,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use crate::controllers::test_support::{app, get_text};

    #[tokio::test]
    async fn serves_the_app_for_client_routes() {
        let app = app(&[]);
        for path in ["/", "/sleep", "/last-night"] {
            let (status, content_type, body) = get_text(&app, path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(content_type.starts_with("text/html"), "{path}: {content_type}");
            assert!(body.contains("<div id=\"root\">"), "{path}");
        }
    }

    #[tokio::test]
    async fn missing_files_and_api_paths_are_404() {
        let app = app(&[]);
        let (status, _, _) = get_text(&app, "/assets/missing.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, content_type, body) = get_text(&app, "/api/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(content_type.starts_with("application/json"));
        assert!(body.contains("no such API endpoint"));
    }
}
