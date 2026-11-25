//! Epic 3 story 7, review round 1, M7 (AD-9; owner Q2): the prompt-token
//! count covers every byte a request carries, call ids included: a
//! `ToolCall`'s id, a `ToolResult`'s and an `EngineResult`'s `call_id`.
//! Pinned by the byte-difference rule through
//! `am_engine::count::prompt_tokens`: 300 more bytes in one of them count
//! exactly 100 more tokens. Written before the fix (tester, 2026-11-25).
#![forbid(unsafe_code)]

use am_domain::rows::{RowStatus, RowWritten};
use am_domain::{
    ChatRequest, EngineResult, Instruction, Message, Part, Role, ToolCall, UntrustedData,
};
use am_engine::count::prompt_tokens;

/// A request whose assistant turn called a tool with the id `call`, whose
/// tool result answers `result`, and whose table write's engine result
/// answers `written`.
fn request(call: &str, result: &str, written: &str) -> ChatRequest {
    let row = RowWritten {
        row_id: uuid::Uuid::nil(),
        version: 1,
        status: RowStatus::Inserted,
    };
    ChatRequest {
        model: "m".into(),
        system: vec![Instruction::authored("You are the seat of step work.")],
        messages: vec![
            Message {
                role: Role::User,
                parts: vec![Part::Instruction(Instruction::authored("Do the work."))],
            },
            Message {
                role: Role::Assistant,
                parts: vec![Part::ToolCall(ToolCall {
                    id: call.into(),
                    name: "lookup".into(),
                    arguments_json: r#"{"word":"one"}"#.into(),
                })],
            },
            Message {
                role: Role::User,
                parts: vec![
                    Part::ToolResult {
                        call_id: result.into(),
                        content: UntrustedData::text("tool:lookup", "a page"),
                    },
                    Part::EngineResult {
                        call_id: written.into(),
                        result: EngineResult::row_written(&row),
                    },
                ],
            },
        ],
        tools: vec![],
        max_output_tokens: 64,
    }
}

/// M7: each call id counts by its UTF-8 bytes: 300 more bytes in a tool
/// call's id, a tool result's `call_id` or an engine result's `call_id`
/// count exactly 100 more prompt tokens.
#[test]
fn call_ids_count_by_their_bytes() {
    let short = "call-1";
    let long = format!("call-1{}", "x".repeat(300));
    let base = prompt_tokens(&request(short, short, short));
    for (what, req) in [
        ("a tool call's id", request(&long, short, short)),
        ("a tool result's call_id", request(short, &long, short)),
        ("an engine result's call_id", request(short, short, &long)),
    ] {
        assert_eq!(
            prompt_tokens(&req),
            base + 100,
            "{what}: 300 more bytes are 100 more tokens"
        );
    }
    assert_eq!(
        prompt_tokens(&request(&long, &long, &long)),
        base + 300,
        "all three"
    );
}
