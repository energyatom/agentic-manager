//! A minimal Server-Sent Events decoder (event and data fields only).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Incremental decoder: feed bytes, take complete events.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
}

impl SseDecoder {
    pub fn new() -> Self {
        SseDecoder::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some((end, sep_len)) = find_boundary(&self.buf) {
            let raw: Vec<u8> = self.buf.drain(..end + sep_len).collect();
            let block = String::from_utf8_lossy(&raw[..end]).to_string();
            if let Some(ev) = parse_block(&block) {
                out.push(ev);
            }
        }
        out
    }
}

fn find_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < buf.len() && &buf[i..i + 4] == b"\r\n\r\n" {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data: Vec<String> = Vec::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data.push(value.to_string()),
            _ => {}
        }
    }
    if event.is_none() && data.is_empty() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_split_events() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"event: a\ndata: {\"x\"").is_empty());
        let evs = d.push(b":1}\n\n: comment\n\ndata: [DONE]\r\n\r\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].event.as_deref(), Some("a"));
        assert_eq!(evs[0].data, "{\"x\":1}");
        assert_eq!(evs[1].data, "[DONE]");
    }
}
