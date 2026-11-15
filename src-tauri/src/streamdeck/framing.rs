//! Framing of the channel: UTF-8 text, one JSON object per line.

use serde_json::{Map, Value};

/// Longest line either side may send, in bytes, not counting the newline.
pub const MAX_LINE_BYTES: usize = 65_536;

const NEWLINE: u8 = b'\n';

/// A line that breaks the framing rules. The connection is closed over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    LineTooLong,
    InvalidUtf8,
}

/// What a chunk of bytes completed.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// One whole line, without its newline.
    Line(String),
    /// The stream no longer follows the framing. Nothing comes after it.
    Broken(FramingError),
}

/// Cuts the byte stream of the pipe into lines. A chunk can hold several
/// lines or a part of one, and a multi-byte character can be split across
/// chunks, so the bytes are kept as they arrive and decoded one whole line at
/// a time.
#[derive(Default)]
pub struct LineDecoder {
    pending: Vec<u8>,
}

impl LineDecoder {
    /// The lines this chunk completed, in order, up to and including the
    /// first one that breaks the framing.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        let mut rest = chunk;
        while let Some(end) = rest.iter().position(|byte| *byte == NEWLINE) {
            let tail = &rest[..end];
            rest = &rest[end + 1..];
            match self.take_line(tail) {
                Ok(line) => frames.push(Frame::Line(line)),
                Err(error) => {
                    frames.push(Frame::Broken(error));
                    return frames;
                }
            }
        }
        if self.pending.len() + rest.len() > MAX_LINE_BYTES {
            self.pending.clear();
            frames.push(Frame::Broken(FramingError::LineTooLong));
            return frames;
        }
        self.pending.extend_from_slice(rest);
        frames
    }

    fn take_line(&mut self, tail: &[u8]) -> Result<String, FramingError> {
        if self.pending.len() + tail.len() > MAX_LINE_BYTES {
            self.pending.clear();
            return Err(FramingError::LineTooLong);
        }
        let mut line = std::mem::take(&mut self.pending);
        line.extend_from_slice(tail);
        String::from_utf8(line).map_err(|_| FramingError::InvalidUtf8)
    }
}

/// One message as the bytes of one line, newline included.
pub fn encode_line(message: &Map<String, Value>) -> Result<Vec<u8>, FramingError> {
    // serde_json escapes line breaks inside strings, so the only newline is
    // the one added here. A map with string keys always serializes.
    let mut bytes = serde_json::to_vec(message).map_err(|_| FramingError::LineTooLong)?;
    if bytes.len() > MAX_LINE_BYTES {
        return Err(FramingError::LineTooLong);
    }
    bytes.push(NEWLINE);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(frames: Vec<Frame>) -> Vec<String> {
        frames
            .into_iter()
            .map(|frame| match frame {
                Frame::Line(line) => line,
                Frame::Broken(error) => panic!("unexpected {error:?}"),
            })
            .collect()
    }

    #[test]
    fn a_chunk_can_hold_several_lines() {
        let mut decoder = LineDecoder::default();
        assert_eq!(
            lines(decoder.push(b"{\"a\":1}\n{\"b\":2}\n")),
            ["{\"a\":1}", "{\"b\":2}"]
        );
    }

    #[test]
    fn a_line_can_arrive_in_pieces() {
        let mut decoder = LineDecoder::default();
        assert!(decoder.push(b"{\"a\"").is_empty());
        assert!(decoder.push(b":").is_empty());
        assert_eq!(lines(decoder.push(b"1}\n{\"b\"")), ["{\"a\":1}"]);
        assert_eq!(lines(decoder.push(b":2}\n")), ["{\"b\":2}"]);
    }

    #[test]
    fn a_character_split_across_chunks_is_decoded_whole() {
        let text = "{\"name\":\"Zo\u{eb} \u{1f600}\"}\n".as_bytes();
        for split in 1..text.len() - 1 {
            let mut decoder = LineDecoder::default();
            let mut frames = decoder.push(&text[..split]);
            frames.extend(decoder.push(&text[split..]));
            assert_eq!(lines(frames), ["{\"name\":\"Zo\u{eb} \u{1f600}\"}"]);
        }
    }

    #[test]
    fn an_empty_line_is_a_line() {
        let mut decoder = LineDecoder::default();
        assert_eq!(lines(decoder.push(b"\n")), [""]);
    }

    #[test]
    fn a_line_of_exactly_the_limit_is_accepted() {
        let mut decoder = LineDecoder::default();
        let mut text = vec![b'a'; MAX_LINE_BYTES];
        text.push(NEWLINE);
        assert_eq!(lines(decoder.push(&text))[0].len(), MAX_LINE_BYTES);
    }

    #[test]
    fn a_line_one_byte_over_the_limit_is_refused() {
        let mut decoder = LineDecoder::default();
        let mut text = vec![b'a'; MAX_LINE_BYTES + 1];
        text.push(NEWLINE);
        assert_eq!(
            decoder.push(&text),
            [Frame::Broken(FramingError::LineTooLong)]
        );
    }

    #[test]
    fn a_line_that_never_ends_is_refused_before_it_is_complete() {
        let mut decoder = LineDecoder::default();
        let chunk = vec![b'a'; 40_000];
        assert!(decoder.push(&chunk).is_empty());
        assert_eq!(
            decoder.push(&chunk),
            [Frame::Broken(FramingError::LineTooLong)]
        );
    }

    #[test]
    fn a_line_that_is_not_utf8_is_refused() {
        let mut decoder = LineDecoder::default();
        assert_eq!(
            decoder.push(b"{\"a\":\"\xff\"}\n"),
            [Frame::Broken(FramingError::InvalidUtf8)]
        );
    }

    #[test]
    fn the_lines_before_a_broken_one_are_kept() {
        let mut decoder = LineDecoder::default();
        assert_eq!(
            decoder.push(b"{\"a\":1}\n\xff\n{\"b\":2}\n"),
            [
                Frame::Line("{\"a\":1}".into()),
                Frame::Broken(FramingError::InvalidUtf8)
            ]
        );
    }

    #[test]
    fn a_message_is_encoded_as_one_line() {
        let mut message = Map::new();
        message.insert("text".into(), Value::from("two\nlines"));
        let bytes = encode_line(&message).unwrap();
        assert_eq!(bytes.iter().filter(|byte| **byte == NEWLINE).count(), 1);
        assert_eq!(bytes.last(), Some(&NEWLINE));
        let mut decoder = LineDecoder::default();
        let decoded: Value = serde_json::from_str(&lines(decoder.push(&bytes))[0]).unwrap();
        assert_eq!(decoded["text"], "two\nlines");
    }

    #[test]
    fn a_message_longer_than_a_line_is_not_encoded() {
        let mut message = Map::new();
        message.insert("text".into(), Value::from("a".repeat(MAX_LINE_BYTES)));
        assert_eq!(encode_line(&message), Err(FramingError::LineTooLong));
    }
}
