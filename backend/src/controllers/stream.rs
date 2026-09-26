//! GET /api/stream: Server-Sent Events.
//! - `reading`: each stored reading, same JSON as GET /api/current
//! - `sleep`: a session that just started (`ended_at` null) or ended, same JSON
//!   as GET /api/sleep/current

use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tracing::warn;

use super::AppState;
use crate::domain::LiveEvent;

pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = BroadcastStream::new(state.events.subscribe()).filter_map(|msg| {
        let event = match msg {
            Ok(LiveEvent::Reading(scored)) => Event::default().event("reading").json_data(&scored),
            Ok(LiveEvent::Sleep(session)) => Event::default().event("sleep").json_data(&session),
            Err(BroadcastStreamRecvError::Lagged(n)) => {
                warn!("SSE client fell behind, skipped {n} events");
                return None;
            }
        };
        match event {
            Ok(event) => Some(Ok(event)),
            Err(e) => {
                warn!("could not serialize SSE event: {e}");
                None
            }
        }
    });
    // Comment lines every 15 s keep proxies and tunnels from closing the connection.
    Sse::new(events).keep_alive(KeepAlive::default())
}
