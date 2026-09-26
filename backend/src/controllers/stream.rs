//! GET /api/stream: Server-Sent Events. Each stored reading is sent as a
//! `reading` event whose data is the same JSON as GET /api/current.

use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tracing::warn;

use super::AppState;

pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = BroadcastStream::new(state.events.subscribe()).filter_map(|msg| match msg {
        Ok(scored) => match Event::default().event("reading").json_data(&scored) {
            Ok(event) => Some(Ok(event)),
            Err(e) => {
                warn!("could not serialize reading for SSE: {e}");
                None
            }
        },
        Err(BroadcastStreamRecvError::Lagged(n)) => {
            warn!("SSE client fell behind, skipped {n} readings");
            None
        }
    });
    // Comment lines every 15 s keep proxies and tunnels from closing the connection.
    Sse::new(events).keep_alive(KeepAlive::default())
}
