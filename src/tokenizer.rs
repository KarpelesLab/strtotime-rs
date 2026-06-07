//! Allocation-free tokenizer.
//!
//! Port of the Go reference's `token.go`. Input is cut into maximal runs of one
//! character class; tokens borrow `&str` slices of the input (no copies). The
//! token list lives in a fixed-size stack buffer — inputs that produce more than
//! [`MAX_TOKENS`] tokens are rejected as [`Error::TooLong`].

use crate::error::Error;

/// Maximum number of tokens a single input may produce.
pub const MAX_TOKENS: usize = 96;

/// Character class of a token. Mirrors the Go reference's `tokenType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokType {
    Str,
    Number,
    Operator,
    Whitespace,
    Punctuation,
}

/// A single token: its text, class, and byte offset in the input.
#[derive(Debug, Clone, Copy)]
pub struct Token<'a> {
    pub val: &'a str,
    pub typ: TokType,
    pub pos: usize,
}

impl Token<'_> {
    const EMPTY: Token<'static> = Token {
        val: "",
        typ: TokType::Whitespace,
        pos: 0,
    };
}

/// A tokenized input held in a fixed-size buffer.
pub struct Tokens<'a> {
    buf: [Token<'a>; MAX_TOKENS],
    len: usize,
}

impl<'a> Tokens<'a> {
    /// Tokens as a slice.
    pub fn as_slice(&self) -> &[Token<'a>] {
        &self.buf[..self.len]
    }
}

/// Classify an ASCII byte. Non-ASCII bytes (>= 0x80) classify as `Str`, matching
/// "anything else is a string char" — such inputs are ultimately unparseable.
fn classify(c: u8) -> TokType {
    match c {
        b' ' | b'\t' | b'\n' | b'\r' => TokType::Whitespace,
        b'0'..=b'9' => TokType::Number,
        b'+' | b'-' | b':' | b'/' | b'.' => TokType::Operator,
        b',' | b';' | b'(' | b')' | b'[' | b']' => TokType::Punctuation,
        _ => TokType::Str,
    }
}

/// Tokenize `s`. Operates on bytes; since all token-significant characters are
/// ASCII, runs of non-ASCII bytes are grouped as `Str` tokens.
pub fn tokenize(s: &str) -> Result<Tokens<'_>, Error> {
    let mut buf = [Token::EMPTY; MAX_TOKENS];
    let mut len = 0usize;

    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Ok(Tokens { buf, len });
    }

    let mut cur = classify(bytes[0]);
    let mut start = 0usize;

    let mut i = 1;
    while i < bytes.len() {
        let nt = classify(bytes[i]);
        if nt != cur {
            if len >= MAX_TOKENS {
                return Err(Error::TooLong);
            }
            buf[len] = Token { val: &s[start..i], typ: cur, pos: start };
            len += 1;
            cur = nt;
            start = i;
        }
        i += 1;
    }
    if len >= MAX_TOKENS {
        return Err(Error::TooLong);
    }
    buf[len] = Token { val: &s[start..], typ: cur, pos: start };
    len += 1;

    Ok(Tokens { buf, len })
}
