//! AWS's event stream encoding, which Bedrock's ConverseStream answers in:
//! binary frames, each with headers (the event's type) and a JSON payload,
//! checked by CRC-32.

/// One decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// `:message-type`: `event` or `exception`.
    pub message_type: String,
    /// `:event-type`, or `:exception-type` for an exception.
    pub event_type: String,
    pub payload: Vec<u8>,
}

/// Takes bytes as they come and gives back each frame once it's whole.
#[derive(Debug, Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}

/// The fixed part before the headers: total length, headers' length, and their CRC.
const PRELUDE: usize = 12;
/// The largest frame accepted (AWS's limit is 16 MiB).
const MAX_FRAME: usize = 16 << 20;

impl Decoder {
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, String> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        loop {
            if self.buffer.len() < PRELUDE {
                return Ok(frames);
            }
            let total = be32(&self.buffer[0..4]) as usize;
            let headers_len = be32(&self.buffer[4..8]) as usize;
            if !(PRELUDE + 4..=MAX_FRAME).contains(&total) || headers_len > total - PRELUDE - 4 {
                return Err("the response isn't an AWS event stream".to_owned());
            }
            if crc32(&self.buffer[0..8]) != be32(&self.buffer[8..12]) {
                return Err("the event stream is damaged".to_owned());
            }
            if self.buffer.len() < total {
                return Ok(frames);
            }
            let frame: Vec<u8> = self.buffer.drain(..total).collect();
            if crc32(&frame[..total - 4]) != be32(&frame[total - 4..]) {
                return Err("the event stream is damaged".to_owned());
            }
            let headers = &frame[PRELUDE..PRELUDE + headers_len];
            let payload = frame[PRELUDE + headers_len..total - 4].to_vec();
            let mut message_type = String::new();
            let mut event_type = String::new();
            for (name, value) in parse_headers(headers)? {
                match name.as_str() {
                    ":message-type" => message_type = value,
                    ":event-type" | ":exception-type" => event_type = value,
                    _ => {}
                }
            }
            frames.push(Frame {
                message_type,
                event_type,
                payload,
            });
        }
    }
}

/// The string headers; the others are skipped.
fn parse_headers(mut bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let damaged = || "an event's headers are damaged".to_owned();
    let mut headers = Vec::new();
    while !bytes.is_empty() {
        let name_len = *bytes.first().ok_or_else(damaged)? as usize;
        let name = bytes.get(1..1 + name_len).ok_or_else(damaged)?;
        let name = String::from_utf8_lossy(name).into_owned();
        bytes = &bytes[1 + name_len..];
        let kind = *bytes.first().ok_or_else(damaged)?;
        bytes = &bytes[1..];
        let fixed = |len: usize, bytes: &mut &[u8]| -> Result<(), String> {
            *bytes = bytes.get(len..).ok_or_else(damaged)?;
            Ok(())
        };
        match kind {
            // true, false
            0 | 1 => {}
            2 => fixed(1, &mut bytes)?,
            3 => fixed(2, &mut bytes)?,
            4 => fixed(4, &mut bytes)?,
            5 | 8 => fixed(8, &mut bytes)?,
            9 => fixed(16, &mut bytes)?,
            // bytes, string: a 16-bit length first
            6 | 7 => {
                let len = bytes.get(0..2).ok_or_else(damaged)?;
                let len = u16::from_be_bytes([len[0], len[1]]) as usize;
                let value = bytes.get(2..2 + len).ok_or_else(damaged)?;
                if kind == 7 {
                    headers.push((name, String::from_utf8_lossy(value).into_owned()));
                }
                bytes = &bytes[2 + len..];
            }
            _ => return Err(damaged()),
        }
    }
    Ok(headers)
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// CRC-32 (IEEE 802.3), as the event stream uses.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// A frame as AWS writes one, for tests.
#[cfg(test)]
pub fn encode(message_type: &str, event_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut headers = Vec::new();
    let type_header = if message_type == "exception" {
        ":exception-type"
    } else {
        ":event-type"
    };
    for (name, value) in [(":message-type", message_type), (type_header, event_type)] {
        headers.push(name.len() as u8);
        headers.extend_from_slice(name.as_bytes());
        headers.push(7);
        headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
        headers.extend_from_slice(value.as_bytes());
    }
    let total = (PRELUDE + headers.len() + payload.len() + 4) as u32;
    let mut frame = Vec::new();
    frame.extend_from_slice(&total.to_be_bytes());
    frame.extend_from_slice(&(headers.len() as u32).to_be_bytes());
    let prelude_crc = crc32(&frame);
    frame.extend_from_slice(&prelude_crc.to_be_bytes());
    frame.extend_from_slice(&headers);
    frame.extend_from_slice(payload);
    let crc = crc32(&frame);
    frame.extend_from_slice(&crc.to_be_bytes());
    frame
}
