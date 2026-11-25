//! Server-sent events, read as they arrive: what Anthropic, OpenAI (and
//! Azure and Ollama, which speak its API) and Gemini stream.

/// One event: its `event:` name (empty when it had none) and its `data:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub data: String,
}

/// Takes bytes as they come and gives back each event once it's whole.
#[derive(Debug, Default)]
pub struct Reader {
    buffer: Vec<u8>,
    name: String,
    data: Vec<String>,
}

impl Reader {
    /// The events `chunk` completes.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Event> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() || !self.name.is_empty() {
                    events.push(Event {
                        name: std::mem::take(&mut self.name),
                        data: std::mem::take(&mut self.data).join("\n"),
                    });
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => value.clone_into(&mut self.name),
                "data" => self.data.push(value.to_owned()),
                _ => {}
            }
        }
        events
    }

    /// An event the stream ended in without the blank line after it.
    pub fn finish(&mut self) -> Option<Event> {
        let rest = std::mem::take(&mut self.buffer);
        let mut events = self.feed(&[rest.as_slice(), b"\n\n"].concat());
        events.pop()
    }
}
