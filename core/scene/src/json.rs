//! Standard-library-only JSON reader. Object entries retain input order and duplicates.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

/// Deepest container nesting `read` accepts; one more level returns `ErrorKind::TooDeep`.
pub const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    UnexpectedEnd,
    UnexpectedToken,
    InvalidEscape,
    InvalidUnicode,
    ControlCharacter,
    InvalidNumber,
    NonFiniteNumber,
    TrailingCharacters,
    /// Containers nested deeper than `MAX_DEPTH`; refused so dropping the result cannot overflow the stack.
    TooDeep,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: usize,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} at byte {}", self.kind, self.offset)
    }
}

impl std::error::Error for Error {}

struct Reader<'a> {
    text: &'a str,
    pos: usize,
}

impl Reader<'_> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            offset: self.pos,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), Error> {
        match self.peek() {
            Some(found) if found == byte => {
                self.pos += 1;
                Ok(())
            }
            Some(_) => Err(self.error(ErrorKind::UnexpectedToken)),
            None => Err(self.error(ErrorKind::UnexpectedEnd)),
        }
    }

    fn hex(&mut self) -> Result<u32, Error> {
        let mut result = 0;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|b| char::from(b).to_digit(16))
                .ok_or_else(|| self.error(ErrorKind::InvalidUnicode))?;
            result = result * 16 + digit;
            self.pos += 1;
        }
        Ok(result)
    }

    fn string(&mut self) -> Result<String, Error> {
        self.expect(b'"')?;
        let mut result = String::new();
        loop {
            match self.peek() {
                None => return Err(self.error(ErrorKind::UnexpectedEnd)),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(result);
                }
                Some(0..=31) => return Err(self.error(ErrorKind::ControlCharacter)),
                Some(b'\\') => {
                    self.pos += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error(ErrorKind::UnexpectedEnd))?;
                    self.pos += 1;
                    result.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut code = self.hex()?;
                            if (0xd800..=0xdbff).contains(&code) {
                                self.expect(b'\\')
                                    .map_err(|_| self.error(ErrorKind::InvalidUnicode))?;
                                self.expect(b'u')
                                    .map_err(|_| self.error(ErrorKind::InvalidUnicode))?;
                                let low = self.hex()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err(self.error(ErrorKind::InvalidUnicode));
                                }
                                code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
                            }
                            char::from_u32(code)
                                .ok_or_else(|| self.error(ErrorKind::InvalidUnicode))?
                        }
                        _ => {
                            self.pos -= 1;
                            return Err(self.error(ErrorKind::InvalidEscape));
                        }
                    });
                }
                Some(_) => {
                    let ch = self.text[self.pos..]
                        .chars()
                        .next()
                        .ok_or_else(|| self.error(ErrorKind::UnexpectedEnd))?;
                    self.pos += ch.len_utf8();
                    result.push(ch);
                }
            }
        }
    }

    fn digits(&mut self) -> Result<(), Error> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.error(ErrorKind::InvalidNumber));
        }
        Ok(())
    }

    fn number(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error(ErrorKind::InvalidNumber));
            }
        } else {
            self.digits()?;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits()?;
        }
        let value: f64 = self.text[start..self.pos]
            .parse()
            .map_err(|_| self.error(ErrorKind::InvalidNumber))?;
        if !value.is_finite() {
            return Err(self.error(ErrorKind::NonFiniteNumber));
        }
        Ok(Value::Number(value))
    }
}

enum Frame {
    Array(Vec<Value>),
    Object(Vec<(String, Value)>, String),
}

/// Parse one complete JSON document. Containers are parsed with an explicit stack.
/// Numbers use Rust's f64 rounding (including underflow to zero); overflow is an error.
pub fn read(text: &str) -> Result<Value, Error> {
    let mut r = Reader { text, pos: 0 };
    let mut stack = Vec::new();
    let mut ready = None;
    loop {
        r.whitespace();
        if ready.is_none() {
            ready = match r.peek() {
                Some(b'[' | b'{') => {
                    let object = r.peek() == Some(b'{');
                    if stack.len() >= MAX_DEPTH {
                        return Err(r.error(ErrorKind::TooDeep));
                    }
                    r.pos += 1;
                    r.whitespace();
                    if object {
                        if r.peek() == Some(b'}') {
                            r.pos += 1;
                            Some(Value::Object(Vec::new()))
                        } else {
                            let key = r.string()?;
                            r.whitespace();
                            r.expect(b':')?;
                            stack.push(Frame::Object(Vec::new(), key));
                            continue;
                        }
                    } else if r.peek() == Some(b']') {
                        r.pos += 1;
                        Some(Value::Array(Vec::new()))
                    } else {
                        stack.push(Frame::Array(Vec::new()));
                        continue;
                    }
                }
                Some(b'"') => Some(Value::String(r.string()?)),
                Some(b'-' | b'0'..=b'9') => Some(r.number()?),
                Some(b'n' | b't' | b'f') => {
                    let (word, value) = match r.peek() {
                        Some(b'n') => ("null", Value::Null),
                        Some(b't') => ("true", Value::Bool(true)),
                        _ => ("false", Value::Bool(false)),
                    };
                    for byte in word.bytes() {
                        r.expect(byte)?;
                    }
                    Some(value)
                }
                Some(_) => return Err(r.error(ErrorKind::UnexpectedToken)),
                None => return Err(r.error(ErrorKind::UnexpectedEnd)),
            };
        }
        let Some(value) = ready.take() else {
            continue;
        };
        let Some(mut frame) = stack.pop() else {
            r.whitespace();
            return if r.peek().is_none() {
                Ok(value)
            } else {
                Err(r.error(ErrorKind::TrailingCharacters))
            };
        };
        let close = match &mut frame {
            Frame::Array(items) => {
                items.push(value);
                b']'
            }
            Frame::Object(items, key) => {
                items.push((std::mem::take(key), value));
                b'}'
            }
        };
        r.whitespace();
        if r.peek() == Some(close) {
            r.pos += 1;
            ready = Some(match frame {
                Frame::Array(items) => Value::Array(items),
                Frame::Object(items, _) => Value::Object(items),
            });
        } else {
            r.expect(b',')?;
            r.whitespace();
            if let Frame::Object(_, key) = &mut frame {
                *key = r.string()?;
                r.whitespace();
                r.expect(b':')?;
            }
            stack.push(frame);
        }
    }
}
