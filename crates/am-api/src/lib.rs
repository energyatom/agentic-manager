//! API DTOs and events (AD-14): the shapes `/api/v1` and its one WebSocket
//! event stream speak. JSON is camelCase on the wire. Story 3.2 adds the one
//! error envelope ([`ApiError`]), run status ([`RunDto`], [`StepDto`],
//! [`RunDetailDto`]), the event ([`EventDto`]) and the two sign-in exchange
//! bodies ([`SessionBody`], [`PairBody`]). Story 3.4 adds the Inbox
//! ([`InboxItemDto`], [`InboxItemDetailDto`], [`ResolveBody`],
//! [`ResolutionDto`]) and its event ([`EVENT_INBOX_UPDATED`],
//! [`InboxUpdatedDto`]). ts-rs generation comes with the UI epics.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// The one error envelope (AD-14): every error response is this JSON
/// object. `code` is a stable kebab-case string, `message` plain words,
/// `fix` what to do (when there is one) and `details` structured context;
/// `fix` and `details` are left out when absent. No secret ever appears in
/// any field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// An envelope with a code and words, no fix and no details.
    pub fn new(code: &str, message: &str) -> ApiError {
        ApiError {
            code: code.to_string(),
            message: message.to_string(),
            fix: None,
            details: None,
        }
    }

    /// The same envelope with what to do.
    pub fn with_fix(mut self, fix: &str) -> ApiError {
        self.fix = Some(fix.to_string());
        self
    }
}

/// One run of the open project (`GET /api/v1/runs`, newest first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDto {
    pub id: String,
    pub workflow: String,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// One step of a run, in its order (`seq`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepDto {
    pub id: String,
    pub run_id: String,
    pub step_key: String,
    pub seq: i64,
    pub status: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// `GET /api/v1/runs/{id}`: the run and its steps in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetailDto {
    pub run: RunDto,
    pub steps: Vec<StepDto>,
}

/// One event of the stream: a monotonic `seq` from 1 per launch, its
/// `kind` (`run.updated`, `step.updated`, `resync`, ...) and its data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventDto {
    pub seq: u64,
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

/// The kind of the event that tells a client it missed events beyond the
/// held window: re-read run status, then follow the stream.
pub const EVENT_RESYNC: &str = "resync";
/// A run's status or times changed.
pub const EVENT_RUN_UPDATED: &str = "run.updated";
/// A step's status or times changed.
pub const EVENT_STEP_UPDATED: &str = "step.updated";

/// Story 3.4 (AD-28): an Inbox item changed (made `open`, or `resolved`).
/// Its data is [`InboxUpdatedDto`]: the item's id and status, nothing else
/// (never its payload, title, body, decision, note or run; owner condition
/// 4).
pub const EVENT_INBOX_UPDATED: &str = "inbox.updated";

/// The data of an [`EVENT_INBOX_UPDATED`] event: exactly the item's id and
/// status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InboxUpdatedDto {
    pub id: String,
    pub status: String,
}

/// One Inbox item (`GET /api/v1/inbox`, the open items; story 3.4, AD-28):
/// its kind (`review`, `question`, `failure`, ...), status (`open` or
/// `resolved`), run and step run, taint, the engine's words, a Review's
/// `maxReturns`, and once resolved the decision, who resolved it in words
/// ("You (Reviewer)") and when. Never a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItemDto {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub run_id: String,
    pub step_run_id: String,
    pub taint: bool,
    pub title: String,
    pub body: String,
    pub created_at: String,
    #[serde(default)]
    pub max_returns: Option<u32>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<String>,
}

/// `GET /api/v1/inbox/{id}`: the item, its payload's text (the producing
/// attempt's output; model text, rendered only through the sanitising
/// renderer) and, once resolved, the note a return or an answer carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItemDetailDto {
    pub item: InboxItemDto,
    #[serde(default)]
    pub payload: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// `POST /api/v1/inbox/{id}/resolve`: the decision (`approve`, `return`,
/// `reject`, `answer`, `dismiss`), the note a return or an answer needs,
/// and for approving a tainted Review the item's id typed back
/// (`confirmItemId`, owner condition 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveBody {
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_item_id: Option<String>,
}

/// A resolve's answer: the resolved item and its run as they are after the
/// server applied the decision. `applied` is `false` when the decision was
/// recorded and the engine's apply of it failed (story 3.4, review R2): the
/// decision stands, and the run waits for story 3.9's resume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionDto {
    pub item: InboxItemDto,
    pub run: RunDto,
    pub applied: bool,
}

/// `POST /api/v1/session`: the launch code from the URL fragment. `Debug`
/// never shows the code.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBody {
    pub code: String,
}

impl std::fmt::Debug for SessionBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionBody { code: <redacted> }")
    }
}

/// `POST /api/v1/pair`: the pairing code printed on the serve terminal.
/// `Debug` never shows the code.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairBody {
    pub code: String,
}

impl std::fmt::Debug for PairBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairBody { code: <redacted> }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_leaves_out_absent_fields() {
        let e = ApiError::new("not-found", "No such route.");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"code": "not-found", "message": "No such route."})
        );
        let e = e.with_fix("Check the path.");
        assert_eq!(
            serde_json::to_value(&e).unwrap()["fix"],
            serde_json::json!("Check the path.")
        );
    }

    #[test]
    fn dtos_are_camel_case() {
        let s = StepDto {
            id: "s".into(),
            run_id: "r".into(),
            step_key: "k".into(),
            seq: 1,
            status: "queued".into(),
            started_at: None,
            finished_at: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("runId").is_some() && v.get("stepKey").is_some());
    }
}
