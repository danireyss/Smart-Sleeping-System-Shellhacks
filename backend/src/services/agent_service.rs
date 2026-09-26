//! The chat agent. The backend calls the model; the model requests tools; the
//! tools run here in-process through the services (never repositories) and
//! their results go back to the model. The model never touches the database
//! or hardware.
//!
//! Each turn streams events: `ToolCall` (name, arguments, returned data) as tools
//! run, `Token` as reply text arrives, then `Done` with the grounding result.
//! If the model is not configured or unreachable, `Offline` is sent instead and
//! the rest of the app keeps working.

use std::sync::Arc;

use chrono::{DateTime, SubsecRound, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;
use tracing::warn;

use super::{ReadingService, SleepService};
use crate::adapters::llm_client::{ChatModel, ToolCall};
use crate::domain::grounding::{self, Grounding};
use crate::domain::targets::targets;

/// Tool rounds per turn before the model must answer in text.
const MAX_TOOL_ROUNDS: usize = 4;

pub const OFFLINE_MESSAGE: &str = "Assistant offline — dashboard still live";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A previous message in the conversation, sent back by the client.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Turn {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    ToolCall { name: String, arguments: Value, result: Value },
    Token { text: String },
    Done { grounding: Grounding },
    Offline { message: String },
}

pub struct AgentService {
    /// `None` when LLM_API_KEY or LLM_MODEL is not set: every turn is offline.
    model: Option<Arc<dyn ChatModel>>,
    readings: Arc<ReadingService>,
    sleep: Arc<SleepService>,
}

impl AgentService {
    pub fn new(
        model: Option<Arc<dyn ChatModel>>,
        readings: Arc<ReadingService>,
        sleep: Arc<SleepService>,
    ) -> Self {
        Self { model, readings, sleep }
    }

    /// Runs one chat turn, sending events to `events` until `Done` or `Offline`.
    /// Stops early if the receiver is dropped (client disconnected).
    pub async fn chat(&self, message: &str, history: &[Turn], events: UnboundedSender<AgentEvent>) {
        let Some(model) = &self.model else {
            let _ = events.send(AgentEvent::Offline { message: OFFLINE_MESSAGE.into() });
            return;
        };

        let now = Utc::now().trunc_subsecs(0);
        let mut messages = vec![json!({"role": "system", "content": system_prompt(now)})];
        for turn in history {
            let role = match turn.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            messages.push(json!({"role": role, "content": turn.content}));
        }
        messages.push(json!({"role": "user", "content": message}));

        let tools = tool_definitions();
        let mut reply = String::new();
        let mut tool_results = Vec::new();

        for round in 0..=MAX_TOOL_ROUNDS {
            let allow_tools = round < MAX_TOOL_ROUNDS;
            let mut on_token = |text: &str| {
                reply.push_str(text);
                let _ = events.send(AgentEvent::Token { text: text.to_string() });
            };
            let completion = match model.complete(&messages, &tools, allow_tools, &mut on_token).await
            {
                Ok(c) => c,
                Err(e) => {
                    warn!("assistant offline: {e}");
                    let _ = events.send(AgentEvent::Offline { message: OFFLINE_MESSAGE.into() });
                    return;
                }
            };
            if completion.tool_calls.is_empty() || events.is_closed() {
                break;
            }

            messages.push(assistant_tool_message(&completion.text, &completion.tool_calls));
            for call in &completion.tool_calls {
                let arguments: Value =
                    serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
                let result = self.run_tool(&call.name, &arguments, now).await;
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call.id,
                    "content": result.to_string(),
                }));
                let _ = events.send(AgentEvent::ToolCall {
                    name: call.name.clone(),
                    arguments,
                    result: result.clone(),
                });
                tool_results.push(result);
            }
        }

        let grounding = grounding::check(&reply, &tool_results);
        if !grounding.verified {
            warn!("reply has numbers not found in tool results: {:?}", grounding.unmatched);
        }
        let _ = events.send(AgentEvent::Done { grounding });
    }

    /// Runs a tool through the services. Errors become `{"error": ...}` results
    /// so the model can say the data is missing.
    async fn run_tool(&self, name: &str, args: &Value, now: DateTime<Utc>) -> Value {
        let result = match name {
            "get_current" => self.get_current(now).await,
            "get_summary" => self.get_summary(args).await,
            "get_night_latest" => self.get_night_latest().await,
            "get_targets" => Ok(targets()),
            other => Err(format!("unknown tool {other}")),
        };
        result.unwrap_or_else(|error| json!({ "error": error }))
    }

    async fn get_current(&self, now: DateTime<Utc>) -> Result<Value, String> {
        let current = self.readings.current().await.map_err(|e| e.to_string())?;
        let Some(scored) = current else { return Err("no readings yet".into()) };
        let minutes_ago = (now - scored.reading.received_at).num_minutes().max(0);
        let mut value = serde_json::to_value(&scored).map_err(|e| e.to_string())?;
        value["minutes_since_reading"] = json!(minutes_ago);
        Ok(value)
    }

    async fn get_summary(&self, args: &Value) -> Result<Value, String> {
        let parse = |key: &str| -> Result<DateTime<Utc>, String> {
            let text = args[key].as_str().ok_or(format!("missing {key}"))?;
            DateTime::parse_from_rfc3339(text)
                .map(|t| t.with_timezone(&Utc))
                .map_err(|_| format!("{key} must be an RFC 3339 time like 2026-09-26T06:00:00Z"))
        };
        let (start, end) = (parse("start")?, parse("end")?);
        if start >= end {
            return Err("start must be before end".into());
        }
        let summary = self.readings.summary(start, end).await.map_err(|e| e.to_string())?;
        serde_json::to_value(&summary).map_err(|e| e.to_string())
    }

    async fn get_night_latest(&self) -> Result<Value, String> {
        let night = self.sleep.latest_night().await.map_err(|e| e.to_string())?;
        let Some(night) = night else { return Err("no finished sleep session yet".into()) };
        let mut value = serde_json::to_value(&night).map_err(|e| e.to_string())?;
        let (h, m) = (night.duration_minutes / 60, night.duration_minutes % 60);
        value["time_in_sleep_mode"] = json!(format!("{h}h {m}m"));
        Ok(value)
    }
}

fn assistant_tool_message(text: &str, calls: &[ToolCall]) -> Value {
    let tool_calls: Vec<Value> = calls
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments },
            })
        })
        .collect();
    json!({
        "role": "assistant",
        "content": if text.is_empty() { Value::Null } else { json!(text) },
        "tool_calls": tool_calls,
    })
}

pub fn system_prompt(now: DateTime<Utc>) -> String {
    format!(
        "You are the assistant for a bedroom sleep-environment monitor. It measures the room \
(estimated CO2, temperature, humidity) and scores the room, not the person.

Current time: {now} (UTC). Times in tool results are UTC.

Rules:
- Every number you state must come from a tool result in this turn. Before answering any \
question about readings, scores, nights, or targets, call the tools, even if earlier messages \
mention numbers. Copy numbers exactly as the tools return them. Do not calculate new numbers \
(no differences, conversions, or averages) and do not use numbered lists.
- If a tool result is missing data (an error, no readings, readings flagged during sensor \
warm-up), say so plainly. Never estimate or guess.
- Always call CO2 values \"estimated (eCO2)\".
- When recommending something, name the metric, its value, its target (from get_targets), and \
one concrete action. Describe the action in words and point to the target range exactly as \
get_targets gives it (e.g. \"cool the room into the 65-70 °F target\"); never suggest a specific \
setting, setpoint, or amount of your own.
- If asked where a target comes from, name the sources that get_targets lists for that metric \
(authors and year only). Do not describe study findings beyond the target itself.
- No medical advice and no claims about the person's sleep or health. Describe room conditions only.
- Keep replies short: two to four sentences, plain text.",
        now = now.format("%Y-%m-%dT%H:%M:%SZ")
    )
}

pub fn tool_definitions() -> Vec<Value> {
    let tool = |name: &str, description: &str, parameters: Value| {
        json!({
            "type": "function",
            "function": { "name": name, "description": description, "parameters": parameters },
        })
    };
    let no_params = json!({ "type": "object", "properties": {} });
    vec![
        tool(
            "get_current",
            "Latest reading: eCO2 (estimated), TVOC, temperature, humidity, flags, sub-scores, \
             total score and band, and minutes since the reading.",
            no_params.clone(),
        ),
        tool(
            "get_summary",
            "Statistics for a time range: avg/min/max and minutes out of target per metric, \
             and score avg/min/max with band.",
            json!({
                "type": "object",
                "properties": {
                    "start": { "type": "string", "description": "RFC 3339 UTC start, inclusive, e.g. 2026-09-26T06:00:00Z" },
                    "end": { "type": "string", "description": "RFC 3339 UTC end, exclusive" },
                },
                "required": ["start", "end"],
            }),
        ),
        tool(
            "get_night_latest",
            "Report for the most recent finished sleep session: time in sleep mode, nightly \
             score and band, completeness, per-metric stats, and the lowest-scoring metric.",
            no_params.clone(),
        ),
        tool(
            "get_targets",
            "Target ranges for each metric, score bands, and data-quality thresholds.",
            no_params,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use chrono::{Duration, TimeZone};
    use tokio::sync::mpsc;

    use super::*;
    use crate::adapters::llm_client::{BoxFuture, Completion, LlmError, TokenSink};
    use crate::domain::reading::WARM_UP_SECS;
    use crate::domain::Reading;
    use crate::repositories::{
        ReadingRepository, SessionRepository, SqliteReadingRepository, SqliteSessionRepository,
    };

    /// Returns scripted completions in order and records each call.
    #[derive(Default)]
    struct FakeModel {
        script: Mutex<VecDeque<Result<Completion, LlmError>>>,
        calls: Mutex<Vec<(Vec<Value>, bool)>>,
    }

    impl FakeModel {
        fn new(script: Vec<Result<Completion, LlmError>>) -> Arc<Self> {
            Arc::new(Self { script: Mutex::new(script.into()), ..Default::default() })
        }
    }

    impl ChatModel for FakeModel {
        fn complete<'a>(
            &'a self,
            messages: &'a [Value],
            _tools: &'a [Value],
            allow_tools: bool,
            on_token: TokenSink<'a>,
        ) -> BoxFuture<'a, Result<Completion, LlmError>> {
            self.calls.lock().unwrap().push((messages.to_vec(), allow_tools));
            let next = self.script.lock().unwrap().pop_front();
            Box::pin(async move {
                let completion = next.unwrap_or_else(|| Ok(text("(script ended)")))?;
                // Stream the text in two pieces, like a real model.
                let chars = completion.text.chars().count();
                let mid = completion.text.char_indices().nth(chars / 2).map_or(0, |(i, _)| i);
                let (a, b) = completion.text.split_at(mid);
                for piece in [a, b].into_iter().filter(|p| !p.is_empty()) {
                    on_token(piece);
                }
                Ok(completion)
            })
        }
    }

    fn text(t: &str) -> Completion {
        Completion { text: t.to_string(), tool_calls: vec![] }
    }

    fn call(name: &str, arguments: &str) -> Completion {
        Completion {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: format!("id_{name}"),
                name: name.to_string(),
                arguments: arguments.to_string(),
            }],
        }
    }

    fn t(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, hour, 0, 0).unwrap()
    }

    fn reading(at: DateTime<Utc>) -> Reading {
        Reading {
            received_at: at,
            eco2_ppm: 434.0,
            tvoc_ppb: 3.0,
            temp_f: Some(75.9),
            humidity_pct: Some(51.9),
            uptime_s: WARM_UP_SECS,
        }
    }

    /// An agent over in-memory storage holding readings every minute 02:00–04:00
    /// and one session 02:30–03:20 (50 minutes).
    fn agent(model: Option<Arc<dyn ChatModel>>) -> AgentService {
        let repo = Arc::new(SqliteReadingRepository::in_memory().unwrap());
        for m in 0..120 {
            let r = reading(t(2) + Duration::minutes(m));
            repo.save(&r, &r.flags()).unwrap();
        }
        let sessions = Arc::new(SqliteSessionRepository::in_memory().unwrap());
        sessions.start(t(2) + Duration::minutes(30)).unwrap();
        sessions.end(t(3) + Duration::minutes(20)).unwrap();
        let readings = Arc::new(ReadingService::new(repo.clone()));
        let sleep = Arc::new(SleepService::new(sessions, repo));
        AgentService::new(model, readings, sleep)
    }

    async fn run(agent: &AgentService, message: &str, history: &[Turn]) -> Vec<AgentEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        agent.chat(message, history, tx).await;
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        events
    }

    fn reply_text(events: &[AgentEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Token { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn grounding(events: &[AgentEvent]) -> &Grounding {
        match events.last() {
            Some(AgentEvent::Done { grounding }) => grounding,
            other => panic!("expected Done last, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn runs_tools_streams_reply_and_verifies_numbers() {
        let model = FakeModel::new(vec![
            Ok(call("get_current", "{}")),
            Ok(call("get_targets", "{}")),
            Ok(text("Temperature is 75.9 °F against a 65–70 °F target; try cooling the room.")),
        ]);
        let agent = agent(Some(model.clone()));
        let events = run(&agent, "How's the room?", &[]).await;

        let tools: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolCall { name, result, .. } => Some((name.as_str(), result)),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].0, "get_current");
        assert_eq!(tools[0].1["temp_f"], 75.9);
        // (100 + 41 + 100) / 3 = 80.3 with humidity 51.9% inside the 40-60% target
        assert_eq!(tools[0].1["score"]["band"], "good");
        assert!(tools[0].1["minutes_since_reading"].is_i64());
        assert_eq!(tools[1].0, "get_targets");
        assert_eq!(tools[1].1["temp_f"]["target_min"], 65.0);

        assert!(reply_text(&events).starts_with("Temperature is 75.9"));
        assert_eq!(grounding(&events), &Grounding { verified: true, checked: 3, unmatched: vec![] });

        // The model saw the tool results as tool messages with matching ids.
        let calls = model.calls.lock().unwrap();
        let last_messages = &calls.last().unwrap().0;
        let tool_msg = last_messages.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(tool_msg["tool_call_id"], "id_get_current");
        // system, user, then the assistant's tool call
        assert_eq!(last_messages[2]["tool_calls"][0]["function"]["name"], "get_current");
    }

    #[tokio::test]
    async fn flags_numbers_not_in_tool_results() {
        let model = FakeModel::new(vec![
            Ok(call("get_current", "{}")),
            Ok(text("It's about 76 °F, roughly 6 degrees too warm.")),
        ]);
        let events = run(&agent(Some(model)), "Temp?", &[]).await;
        let g = grounding(&events);
        assert!(!g.verified);
        assert_eq!(g.unmatched, vec!["76", "6"]);
    }

    #[tokio::test]
    async fn numbers_from_earlier_turns_do_not_count() {
        // The history mentions 75.9, but no tool ran this turn.
        let model = FakeModel::new(vec![Ok(text("Still 75.9 °F."))]);
        let history = [
            Turn { role: Role::User, content: "Temp?".into() },
            Turn { role: Role::Assistant, content: "It is 75.9 °F.".into() },
        ];
        let events = run(&agent(Some(model.clone())), "And now?", &history).await;
        assert_eq!(grounding(&events).unmatched, vec!["75.9"]);

        let messages = &model.calls.lock().unwrap()[0].0;
        let roles: Vec<_> = messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(messages[3]["content"], "And now?");
    }

    #[tokio::test]
    async fn night_report_includes_time_in_sleep_mode() {
        let model = FakeModel::new(vec![
            Ok(call("get_night_latest", "{}")),
            Ok(text("Time in sleep mode: 0h 50m.")),
        ]);
        let events = run(&agent(Some(model)), "Last night?", &[]).await;
        let AgentEvent::ToolCall { result, .. } = &events[0] else { panic!("{events:?}") };
        assert_eq!(result["time_in_sleep_mode"], "0h 50m");
        assert_eq!(result["duration_minutes"], 50);
        assert_eq!(result["short_session"], true);
        assert!(grounding(&events).verified);
    }

    #[tokio::test]
    async fn summary_tool_validates_arguments() {
        let model = FakeModel::new(vec![
            Ok(call("get_summary", r#"{"start":"2026-09-26T02:00:00Z","end":"2026-09-26T03:00:00Z"}"#)),
            Ok(call("get_summary", r#"{"start":"yesterday","end":"2026-09-26T03:00:00Z"}"#)),
            Ok(call("get_summary", "not json")),
            Ok(call("delete_everything", "{}")),
            Ok(text("Done.")),
        ]);
        let events = run(&agent(Some(model)), "Summary?", &[]).await;
        let results: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolCall { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results[0]["readings"], 60);
        assert_eq!(results[0]["temp_f"]["avg"], 75.9);
        assert!(results[1]["error"].as_str().unwrap().contains("RFC 3339"));
        assert_eq!(results[2]["error"], "missing start");
        assert_eq!(results[3]["error"], "unknown tool delete_everything");
    }

    #[tokio::test]
    async fn forces_a_text_answer_after_max_tool_rounds() {
        let mut script: Vec<_> = (0..MAX_TOOL_ROUNDS).map(|_| Ok(call("get_current", "{}"))).collect();
        script.push(Ok(text("Here is the answer.")));
        let model = FakeModel::new(script);
        let events = run(&agent(Some(model.clone())), "Loop?", &[]).await;

        let allowed: Vec<bool> = model.calls.lock().unwrap().iter().map(|(_, a)| *a).collect();
        assert_eq!(allowed, [true, true, true, true, false]);
        assert_eq!(reply_text(&events), "Here is the answer.");
        assert!(matches!(events.last(), Some(AgentEvent::Done { .. })));
    }

    #[tokio::test]
    async fn offline_when_not_configured() {
        let events = run(&agent(None), "Hi", &[]).await;
        assert_eq!(events, vec![AgentEvent::Offline { message: OFFLINE_MESSAGE.into() }]);
    }

    #[tokio::test]
    async fn offline_when_model_fails() {
        let model = FakeModel::new(vec![
            Ok(call("get_current", "{}")),
            Err(LlmError("connection refused".into())),
        ]);
        let events = run(&agent(Some(model)), "Hi", &[]).await;
        assert!(matches!(events.first(), Some(AgentEvent::ToolCall { .. })));
        assert_eq!(events.last(), Some(&AgentEvent::Offline { message: OFFLINE_MESSAGE.into() }));
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::Done { .. })));
    }

    #[test]
    fn system_prompt_states_the_rules() {
        let prompt = system_prompt(t(16));
        assert!(prompt.contains("2026-09-26T16:00:00Z"));
        assert!(prompt.contains("must come from a tool result in this turn"));
        assert!(prompt.contains("estimated (eCO2)"));
        assert!(prompt.contains("its target"));
        assert!(prompt.contains("never suggest a specific"));
        assert!(prompt.contains("No medical advice"));
        assert!(prompt.contains("name the sources that get_targets lists"));
    }

    #[test]
    fn defines_the_four_tools() {
        let names: Vec<_> = tool_definitions()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["get_current", "get_summary", "get_night_latest", "get_targets"]);
    }
}
