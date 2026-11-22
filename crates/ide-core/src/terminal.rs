//! Terminals: the person's shell, in the workspace folder, over a pseudo-terminal.

use std::sync::Arc;

use crate::IdeError;

/// The most terminals open at once.
pub const MAX_TERMINALS: usize = 10;
/// The most bytes one write to a terminal may carry (a paste, say).
pub const MAX_WRITE_BYTES: usize = 1 << 20;

/// A terminal's size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSize {
    /// Checks the size is one a terminal can have: 2–1000 columns, 1–500 rows.
    pub fn new(cols: u16, rows: u16) -> Result<Self, IdeError> {
        if !(2..=1000).contains(&cols) || !(1..=500).contains(&rows) {
            return Err(IdeError::InvalidArgument(format!(
                "a terminal can't be {cols} × {rows}"
            )));
        }
        Ok(Self { cols, rows })
    }
}

/// What a terminal reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// Text the shell wrote, decoded as UTF-8 (a character split between two
    /// reads arrives whole, in the second).
    Output(String),
    /// The shell ended; its exit code where the system gives one. Nothing follows.
    Exited { code: Option<u32> },
}

/// Where a terminal's events go, from its own threads.
pub type TerminalSink = Arc<dyn Fn(TerminalEvent) + Send + Sync>;

/// A running terminal. Dropping it ends the shell.
pub trait TerminalSession: Send + Sync {
    /// Sends keystrokes or pasted text to the shell.
    fn write(&self, data: &[u8]) -> Result<(), IdeError>;
    fn resize(&self, size: TerminalSize) -> Result<(), IdeError>;
    /// The shell's name, for the tab: `pwsh`, `zsh`.
    fn shell(&self) -> &str;
}

/// Starts terminals.
pub trait TerminalHost: Send + Sync {
    /// Starts the person's default shell in `cwd`. Events go to `sink` until
    /// the session is dropped or the shell exits.
    fn spawn(
        &self,
        cwd: &std::path::Path,
        size: TerminalSize,
        sink: TerminalSink,
    ) -> Result<Box<dyn TerminalSession>, IdeError>;
}

/// Turns a stream of bytes into text without splitting a character: what's
/// left over at the end of one chunk is kept for the next. Bytes that aren't
/// UTF-8 become U+FFFD.
#[derive(Debug, Default)]
pub struct Utf8Stream {
    carry: Vec<u8>,
}

impl Utf8Stream {
    pub fn new() -> Self {
        Self::default()
    }

    /// The text of `bytes`, with what was carried over in front of it.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.carry.extend_from_slice(bytes);
        let mut text = String::new();
        let mut rest: &[u8] = &self.carry;
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    text.push_str(valid);
                    rest = &[];
                    break;
                }
                Err(error) => {
                    let (valid, after) = rest.split_at(error.valid_up_to());
                    // `valid` is exactly what `from_utf8` checked.
                    text.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match error.error_len() {
                        // Bad bytes: one replacement character, then go on.
                        Some(len) => {
                            text.push('\u{FFFD}');
                            rest = &after[len..];
                        }
                        // A character cut off at the end: keep it for later.
                        None => {
                            rest = after;
                            break;
                        }
                    }
                }
            }
        }
        self.carry = rest.to_vec();
        text
    }

    /// What's left at the end of the stream, as text.
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.carry);
        String::from_utf8_lossy(&rest).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_split_between_reads_arrives_whole() {
        let bytes = "é😀z".as_bytes();
        let mut stream = Utf8Stream::new();
        // "é" is 2 bytes, "😀" 4: cut inside each.
        assert_eq!(stream.push(&bytes[..1]), "");
        assert_eq!(stream.push(&bytes[1..4]), "é");
        assert_eq!(stream.push(&bytes[4..]), "😀z");
        assert_eq!(stream.finish(), "");
    }

    #[test]
    fn bytes_that_arent_utf8_become_replacement_characters() {
        let mut stream = Utf8Stream::new();
        assert_eq!(stream.push(b"a\xffb\xc3"), "a\u{FFFD}b");
        assert_eq!(stream.finish(), "\u{FFFD}");
    }

    #[test]
    fn sizes() {
        assert!(TerminalSize::new(80, 24).is_ok());
        assert!(TerminalSize::new(1, 24).is_err());
        assert!(TerminalSize::new(80, 0).is_err());
        assert!(TerminalSize::new(1001, 24).is_err());
    }
}
