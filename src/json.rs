//! A small JSON parser that remembers the line and column of every value,
//! so findings can point at the exact spot in a settings file.

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub kind: Kind,
    pub line: usize,
    pub col: usize,
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match &self.kind {
            Kind::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            Kind::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            Kind::Null => "null",
            Kind::Bool(_) => "boolean",
            Kind::Num(_) => "number",
            Kind::Str(_) => "string",
            Kind::Arr(_) => "array",
            Kind::Obj(_) => "object",
        }
    }
}

#[derive(Debug)]
pub struct ParseError {
    pub msg: String,
    pub line: usize,
    pub col: usize,
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    line: usize,
    col: usize,
}

pub fn parse(text: &str) -> Result<Value, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
        line: 1,
        col: 1,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i < p.s.len() {
        return Err(p.err("unexpected trailing content"));
    }
    Ok(v)
}

impl Parser<'_> {
    fn err(&self, msg: &str) -> ParseError {
        ParseError {
            msg: msg.to_string(),
            line: self.line,
            col: self.col,
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if c & 0xC0 != 0x80 {
            self.col += 1;
        }
        Some(c)
    }
    fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.bump();
            } else {
                break;
            }
        }
    }
    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.bump();
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", c as char)))
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > 256 {
            return Err(self.err("nesting too deep"));
        }
        let (line, col) = (self.line, self.col);
        let kind = match self.peek() {
            Some(b'{') => self.object(depth)?,
            Some(b'[') => self.array(depth)?,
            Some(b'"') => Kind::Str(self.string()?),
            Some(b't') => self.word("true", Kind::Bool(true))?,
            Some(b'f') => self.word("false", Kind::Bool(false))?,
            Some(b'n') => self.word("null", Kind::Null)?,
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number()?,
            Some(_) => return Err(self.err("unexpected character")),
            None => return Err(self.err("unexpected end of input")),
        };
        Ok(Value { kind, line, col })
    }
    fn word(&mut self, w: &str, k: Kind) -> Result<Kind, ParseError> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            for _ in 0..w.len() {
                self.bump();
            }
            Ok(k)
        } else {
            Err(self.err("invalid literal"))
        }
    }
    fn number(&mut self) -> Result<Kind, ParseError> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.bump();
            } else {
                break;
            }
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        t.parse::<f64>()
            .map(Kind::Num)
            .map_err(|_| self.err("invalid number"))
    }
    fn string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.bump() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'"') => out.push(b'"'),
                    Some(b'\\') => out.push(b'\\'),
                    Some(b'/') => out.push(b'/'),
                    Some(b'b') => out.push(8),
                    Some(b'f') => out.push(12),
                    Some(b'n') => out.push(b'\n'),
                    Some(b'r') => out.push(b'\r'),
                    Some(b't') => out.push(b'\t'),
                    Some(b'u') => {
                        let mut cp = self.hex4()?;
                        if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                            self.bump();
                            self.bump();
                            let lo = self.hex4()?;
                            cp =
                                0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                        }
                        let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    _ => return Err(self.err("invalid escape")),
                },
                Some(c) if c < 0x20 => return Err(self.err("control character in string")),
                Some(c) => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| self.err("invalid UTF-8 in string"))
    }
    fn hex4(&mut self) -> Result<u32, ParseError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.bump().ok_or_else(|| self.err("bad \\u escape"))?;
            let d = (c as char)
                .to_digit(16)
                .ok_or_else(|| self.err("bad \\u escape"))?;
            v = v * 16 + d;
        }
        Ok(v)
    }
    fn array(&mut self, depth: usize) -> Result<Kind, ParseError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Kind::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => break,
                _ => return Err(self.err("expected ',' or ']' in array")),
            }
        }
        Ok(Kind::Arr(items))
    }
    fn object(&mut self, depth: usize) -> Result<Kind, ParseError> {
        self.expect(b'{')?;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Kind::Obj(items));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a quoted key (trailing comma?)"));
            }
            let k = self.string()?;
            self.ws();
            self.expect(b':')?;
            self.ws();
            let v = self.value(depth + 1)?;
            items.push((k, v));
            self.ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => break,
                _ => return Err(self.err("expected ',' or '}' in object")),
            }
        }
        Ok(Kind::Obj(items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_positions() {
        let v = parse("{\n  \"a\": [1, \"x\\u00e7\"]\n}").unwrap();
        let a = v.get("a").unwrap();
        assert_eq!(a.line, 2);
        assert_eq!(a.col, 8);
        if let Kind::Arr(items) = &a.kind {
            assert_eq!(items[1].as_str(), Some("xç"));
        } else {
            panic!("not array");
        }
    }

    #[test]
    fn reports_trailing_comma() {
        let e = parse("{\"a\": 1,}").unwrap_err();
        assert!(e.msg.contains("trailing comma"));
    }
}
