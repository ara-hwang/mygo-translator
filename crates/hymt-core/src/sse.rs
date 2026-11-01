//! Server-sent-events decoding for llama.cpp's streaming chat endpoint.
//!
//! llama.cpp emits one JSON document per `data:` line followed by a blank line. The decoder
//! is incremental because TCP gives no guarantee that a chunk ends on a line boundary -- and
//! Korean or Chinese text is multi-byte, so a chunk can even split a single character.

/// One decoded event from the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SseEvent {
    /// A `data:` payload with the lines joined by `\n`.
    Data(String),
    /// The stream's explicit end marker, `data: [DONE]`.
    Done,
}

/// Incremental decoder; feed it bytes, get events back.
#[derive(Debug, Default)]
pub struct SseDecoder {
    pending: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseDecoder {
    /// Feeds a chunk of bytes and returns every event that became complete.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut consumed = 0;
        for index in 0..self.pending.len() {
            if self.pending[index] == b'\n' {
                let mut line = &self.pending[consumed..index];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                let line = String::from_utf8_lossy(line).into_owned();
                self.handle_line(&line, &mut events);
                consumed = index + 1;
            }
        }
        if consumed > 0 {
            self.pending.drain(..consumed);
        }
        events
    }

    /// Flushes a trailing line that was not newline-terminated.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.pending.is_empty() {
            let mut line = self.pending.clone();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            self.pending.clear();
            self.handle_line(&line, &mut events);
        }
        self.dispatch(&mut events);
        events
    }

    fn handle_line(&mut self, line: &str, events: &mut Vec<SseEvent>) {
        if line.is_empty() {
            self.dispatch(events);
            return;
        }
        if line.starts_with(':') {
            // Comment / keep-alive.
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "data" => self.data_lines.push(value.to_string()),
            // `event`, `id`, `retry` and unknown fields carry nothing we need.
            _ => {}
        }
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        if self.data_lines.is_empty() {
            return;
        }
        let payload = self.data_lines.join("\n");
        self.data_lines.clear();
        if payload.trim() == "[DONE]" {
            events.push(SseEvent::Done);
        } else {
            events.push(SseEvent::Data(payload));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(chunks: &[&str]) -> Vec<SseEvent> {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(decoder.push_bytes(chunk.as_bytes()));
        }
        events.extend(decoder.finish());
        events
    }

    #[test]
    fn decodes_a_simple_stream() {
        let events = decode(&["data: {\"a\":1}\n\ndata: [DONE]\n\n"]);
        assert_eq!(
            events,
            vec![SseEvent::Data("{\"a\":1}".into()), SseEvent::Done]
        );
    }

    #[test]
    fn tolerates_crlf_and_comments() {
        let events = decode(&[": ping\r\ndata: hello\r\n\r\n"]);
        assert_eq!(events, vec![SseEvent::Data("hello".into())]);
    }

    #[test]
    fn joins_multiline_data_fields() {
        let events = decode(&["data: line1\ndata: line2\n\n"]);
        assert_eq!(events, vec![SseEvent::Data("line1\nline2".into())]);
    }

    #[test]
    fn survives_a_split_in_the_middle_of_a_multibyte_character() {
        let full = "data: {\"text\":\"한국어\"}\n\n";
        let bytes = full.as_bytes();
        let split = full.find('한').unwrap() + 1; // inside the 3-byte character
        let mut decoder = SseDecoder::default();
        let mut events = decoder.push_bytes(&bytes[..split]);
        events.extend(decoder.push_bytes(&bytes[split..]));
        events.extend(decoder.finish());
        assert_eq!(events, vec![SseEvent::Data("{\"text\":\"한국어\"}".into())]);
    }

    #[test]
    fn flushes_a_final_line_without_newline() {
        let events = decode(&["data: [DONE]"]);
        assert_eq!(events, vec![SseEvent::Done]);
    }

    #[test]
    fn ignores_fields_it_does_not_need() {
        let events = decode(&["event: message\nid: 3\nretry: 100\ndata: payload\n\n"]);
        assert_eq!(events, vec![SseEvent::Data("payload".into())]);
    }
}
