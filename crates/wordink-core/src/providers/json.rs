//! Just enough JSON for provider messages, hand-written to keep the wasm
//! small (KTD1). Numbers are validated but not kept: no provider field the
//! core reads is numeric.

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number,
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

const NULL: Value = Value::Null;

impl Value {
    /// Object member, or `Null` when absent or not an object.
    pub(crate) fn get(&self, key: &str) -> &Value {
        match self {
            Value::Object(members) => members
                .iter()
                .find(|(k, _)| k == key)
                .map_or(&NULL, |(_, v)| v),
            _ => &NULL,
        }
    }

    /// Array element, or `Null`.
    pub(crate) fn at(&self, index: usize) -> &Value {
        match self {
            Value::Array(items) => items.get(index).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub(crate) fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Nesting limit, so a hostile message cannot exhaust the stack.
const MAX_DEPTH: u32 = 64;

/// Parses a complete JSON document.
pub(crate) fn parse(text: &str) -> Option<Value> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.ws();
        let hit = self.peek() == Some(b);
        if hit {
            self.i += 1;
        }
        hit
    }

    fn lit(&mut self, word: &[u8], v: Value) -> Option<Value> {
        let end = self.i + word.len();
        (self.s.get(self.i..end) == Some(word)).then(|| {
            self.i = end;
            v
        })
    }

    fn value(&mut self, depth: u32) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.ws();
        match self.peek()? {
            b'n' => self.lit(b"null", Value::Null),
            b't' => self.lit(b"true", Value::Bool(true)),
            b'f' => self.lit(b"false", Value::Bool(false)),
            b'"' => self.string().map(Value::Str),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                if !self.eat(b']') {
                    loop {
                        items.push(self.value(depth + 1)?);
                        if self.eat(b']') {
                            break;
                        }
                        if !self.eat(b',') {
                            return None;
                        }
                    }
                }
                Some(Value::Array(items))
            }
            b'{' => {
                self.i += 1;
                let mut members = Vec::new();
                if !self.eat(b'}') {
                    loop {
                        self.ws();
                        let key = self.string()?;
                        if !self.eat(b':') {
                            return None;
                        }
                        members.push((key, self.value(depth + 1)?));
                        if self.eat(b'}') {
                            break;
                        }
                        if !self.eat(b',') {
                            return None;
                        }
                    }
                }
                Some(Value::Object(members))
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.i;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.i += 1;
        }
        let digits = &self.s[start..self.i];
        digits
            .iter()
            .any(u8::is_ascii_digit)
            .then_some(Value::Number)
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = core::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
        self.i += 4;
        u32::from_str_radix(h, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if self.peek() != Some(b'"') {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while !matches!(self.peek()?, b'"' | b'\\') {
                self.i += 1;
            }
            // Input is a &str, and we only split at ASCII bytes.
            out.push_str(core::str::from_utf8(&self.s[start..self.i]).ok()?);
            let b = self.s[self.i];
            self.i += 1;
            if b == b'"' {
                return Some(out);
            }
            let esc = self.peek()?;
            self.i += 1;
            match esc {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\u{8}'),
                b'f' => out.push('\u{c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    let mut c = self.hex4()?;
                    if (0xD800..0xDC00).contains(&c)
                        && self.s.get(self.i..self.i + 2) == Some(b"\\u")
                    {
                        self.i += 2;
                        let low = self.hex4()?;
                        c = 0x10000 + ((c - 0xD800) << 10) + (low.checked_sub(0xDC00)? & 0x3FF);
                    }
                    out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                }
                _ => return None,
            }
        }
    }
}

/// Appends `s` as a quoted JSON string.
pub(crate) fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u00");
                out.push(char::from(b"0123456789abcdef"[(c as usize) >> 4]));
                out.push(char::from(b"0123456789abcdef"[(c as usize) & 15]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_documents() {
        let v = parse(r#" {"a":[1,-2.5e3,{"b":true}],"c":null,"d":"x"} "#).unwrap();
        assert_eq!(v.get("a").at(2).get("b").bool(), Some(true));
        assert_eq!(v.get("c"), &Value::Null);
        assert_eq!(v.get("d").str(), Some("x"));
        assert_eq!(v.get("missing").at(0).str(), None);
    }

    #[test]
    fn unescapes_strings() {
        let v = parse(r#""q\"b\\s\/n\nt\tué 😀""#).unwrap();
        assert_eq!(v.str(), Some("q\"b\\s/n\nt\tué 😀"));
        assert_eq!(parse("\"caf\u{e9}\"").unwrap().str(), Some("café"));
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "",
            "{",
            "[1,]",
            r#"{"a" 1}"#,
            "tru",
            r#""open"#,
            "1 2",
            r#""\x""#,
            "-",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        let deep = "[".repeat(1000) + &"]".repeat(1000);
        assert_eq!(parse(&deep), None);
    }

    #[test]
    fn quote_round_trips() {
        let s = "say \"hi\"\\\n\t\u{1} é";
        let mut out = String::new();
        quote(s, &mut out);
        assert_eq!(parse(&out).unwrap().str(), Some(s));
    }
}
