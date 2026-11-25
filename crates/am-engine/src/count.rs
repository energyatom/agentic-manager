//! The prompt-token count a reservation is priced from (story 3.7, AD-9;
//! owner Q2): UTF-8 bytes / 3, rounded up, over every system part, every
//! message part (call ids included, review M7) and each offered tool's
//! definition.
//!
//! The Stack has no tokenizer, so "counted" is this fixed rule. It
//! over-counts English (about four characters a token) and is close for
//! Devanagari and CJK, so it seldom under-reserves; a wrong guess moves
//! only the reservation, never the settled spend (settled from the
//! provider's usage). A file part counts by its bytes until epic 12's
//! per-type estimate (AD-31).

use am_domain::{ChatRequest, Part, UntrustedContent, UntrustedData};

/// Bytes per counted token.
const BYTES_PER_TOKEN: u64 = 3;

/// The counted prompt tokens of `request`.
pub fn prompt_tokens(request: &ChatRequest) -> u64 {
    let system: usize = request.system.iter().map(|i| i.text().len()).sum();
    let messages: usize = request
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .map(part_bytes)
        .sum();
    let tools: usize = request
        .tools
        .iter()
        .map(|t| t.name.len() + t.description.len() + t.input_schema_json.len())
        .sum();
    let bytes = (system + messages + tools) as u64;
    bytes.div_ceil(BYTES_PER_TOKEN)
}

fn part_bytes(part: &Part) -> usize {
    match part {
        Part::Instruction(i) => i.text().len(),
        Part::Trusted(t) => t.label().len() + t.text().len(),
        Part::Untrusted(d) => data_bytes(d),
        Part::AssistantText(text) => text.len(),
        // Review M7: the call ids are sent too, so they count.
        Part::ToolCall(call) => call.id.len() + call.name.len() + call.arguments_json.len(),
        Part::ToolResult { call_id, content } => call_id.len() + data_bytes(content),
        Part::EngineResult { call_id, result } => {
            call_id.len() + result.label().len() + result.text().len()
        }
    }
}

fn data_bytes(data: &UntrustedData) -> usize {
    data.label().len()
        + match data.content() {
            UntrustedContent::Text(text) => text.len(),
            UntrustedContent::File(file) => file.name.len() + file.bytes.len(),
        }
}
