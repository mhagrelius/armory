//! Reading the Lua that WoW writes.
//!
//! An addon cannot open a socket or write a file of its own — the sandbox
//! strips `io`, `os`, `debug`, `require` and `loadfile` — so SavedVariables is
//! the only way data leaves the game. The client writes it at logout or
//! `/reload`, as plain Lua source, and something outside has to read it.
//!
//! This is not a Lua interpreter and must not become one. SavedVariables is a
//! generated file in a narrow shape: assignments of table literals containing
//! numbers, strings, booleans, `nil`, and more tables. Nothing in it calls a
//! function, and anything that appears to is a file we should refuse rather
//! than evaluate. Refusing is the point — this parses input that a game wrote
//! into a directory an addon manager also writes to.

use std::collections::BTreeMap;

/// A value out of a SavedVariables file.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
    /// Lua tables are one structure with two halves: a positional array part
    /// and a keyed hash part. Keeping them apart matches how the file is
    /// written and how it is read back.
    Table {
        array: Vec<Value>,
        map: BTreeMap<Key, Value>,
    },
}

/// A table key. Lua allows numbers and strings, and SavedVariables uses both —
/// achievement ids arrive as numbers, character names as strings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// Stored as the text of the number so the key can be ordered and compared
    /// without worrying about float equality.
    Number(String),
    Str(String),
}

impl Key {
    pub fn as_str(&self) -> &str {
        match self {
            Key::Number(text) | Key::Str(text) => text,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        self.as_str().parse().ok()
    }
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        self.as_f64().map(|number| number as u32)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// Look a key up in the hash part of a table.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Table { map, .. } => map
                .get(&Key::Str(key.to_string()))
                .or_else(|| map.get(&Key::Number(key.to_string()))),
            _ => None,
        }
    }

    /// The hash part, for walking a table whose keys are data.
    pub fn entries(&self) -> impl Iterator<Item = (&Key, &Value)> {
        match self {
            Value::Table { map, .. } => Some(map.iter()),
            _ => None,
        }
        .into_iter()
        .flatten()
    }

    /// The array part.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Table { array, .. } => array,
            _ => &[],
        }
    }
}

/// Why a SavedVariables file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset, so a bad file can be pointed at rather than described.
    pub at: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.message, self.at)
    }
}

/// Read a whole SavedVariables file: a sequence of `Name = value` assignments.
pub fn parse(source: &str) -> Result<BTreeMap<String, Value>, ParseError> {
    let mut parser = Parser {
        bytes: source.as_bytes(),
        at: 0,
        depth: 0,
    };
    let mut globals = BTreeMap::new();

    loop {
        parser.skip_trivia();
        if parser.at >= parser.bytes.len() {
            return Ok(globals);
        }
        let name = parser.name()?;
        parser.skip_trivia();
        parser.expect(b'=')?;
        let value = parser.value()?;
        globals.insert(name, value);
        parser.skip_trivia();
        // The writer does not emit statement separators, but a hand-edited file
        // might.
        if parser.peek() == Some(b';') {
            parser.at += 1;
        }
    }
}

/// How deep a table may nest before the file is refused.
///
/// AllTheThings-class data is wide rather than deep; nothing WoW writes gets
/// near this. The limit exists so a malformed or hostile file cannot recurse
/// the parser into a stack overflow, which is a crash rather than an error.
const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            at: self.at,
        })
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// Skip whitespace and `--` comments, including `--[[ long ]]` ones.
    fn skip_trivia(&mut self) {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.at += 1;
            }
            if self.bytes[self.at..].starts_with(b"--") {
                self.at += 2;
                if self.bytes[self.at..].starts_with(b"[[") {
                    self.at += 2;
                    while self.at < self.bytes.len() && !self.bytes[self.at..].starts_with(b"]]") {
                        self.at += 1;
                    }
                    self.at = (self.at + 2).min(self.bytes.len());
                } else {
                    while self.peek().is_some_and(|byte| byte != b'\n') {
                        self.at += 1;
                    }
                }
                continue;
            }
            return;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            self.error(format!("expected `{}`", byte as char))
        }
    }

    fn name(&mut self) -> Result<String, ParseError> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.at += 1;
        }
        if start == self.at {
            return self.error("expected a variable name");
        }
        Ok(String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned())
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        self.skip_trivia();
        match self.peek() {
            Some(b'{') => self.table(),
            Some(b'"') | Some(b'\'') => Ok(Value::Str(self.string()?)),
            Some(b'[') if self.bytes[self.at..].starts_with(b"[[") => {
                Ok(Value::Str(self.long_string()?))
            }
            Some(byte) if byte.is_ascii_digit() || byte == b'-' || byte == b'.' => self.number(),
            Some(_) => self.keyword(),
            None => self.error("expected a value"),
        }
    }

    fn keyword(&mut self) -> Result<Value, ParseError> {
        let rest = &self.bytes[self.at..];
        if rest.starts_with(b"true") {
            self.at += 4;
            Ok(Value::Bool(true))
        } else if rest.starts_with(b"false") {
            self.at += 5;
            Ok(Value::Bool(false))
        } else if rest.starts_with(b"nil") {
            self.at += 3;
            Ok(Value::Nil)
        } else {
            // Anything else here is a function call, a concatenation or an
            // identifier — none of which SavedVariables contains, and none of
            // which this is willing to evaluate.
            self.error("expected a table, string, number, boolean or nil")
        }
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        // Hex, which the writer emits for a few fields.
        if self.bytes[self.at..].starts_with(b"0x") || self.bytes[self.at..].starts_with(b"0X") {
            self.at += 2;
            let digits = self.at;
            while self.peek().is_some_and(|byte| byte.is_ascii_hexdigit()) {
                self.at += 1;
            }
            let text = String::from_utf8_lossy(&self.bytes[digits..self.at]).into_owned();
            return match u64::from_str_radix(&text, 16) {
                Ok(number) => Ok(Value::Number(number as f64)),
                Err(_) => self.error("a hex number that will not parse"),
            };
        }
        while self.peek().is_some_and(|byte| {
            byte.is_ascii_digit() || byte == b'.' || byte == b'e' || byte == b'E' || byte == b'+'
        }) {
            self.at += 1;
        }
        // A trailing `-` in an exponent, which the loop above stops short of.
        if matches!(
            self.bytes.get(self.at.wrapping_sub(1)),
            Some(b'e') | Some(b'E')
        ) && self.peek() == Some(b'-')
        {
            self.at += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();
        match text.parse::<f64>() {
            Ok(number) => Ok(Value::Number(number)),
            Err(_) => self.error(format!("`{text}` is not a number")),
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        let quote = self.peek().expect("a quote");
        self.at += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return self.error("a string that never ends"),
                Some(byte) if byte == quote => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escaped = match self.peek() {
                        None => return self.error("a string that ends in a backslash"),
                        Some(b'n') => '\n',
                        Some(b't') => '\t',
                        Some(b'r') => '\r',
                        Some(b'\\') => '\\',
                        Some(b'"') => '"',
                        Some(b'\'') => '\'',
                        // `\ddd`, which is how the writer escapes bytes above
                        // ASCII — and WoW is full of them, in every realm name
                        // with an accent in it.
                        Some(byte) if byte.is_ascii_digit() => {
                            let start = self.at;
                            let mut digits = 0;
                            while digits < 3 && self.peek().is_some_and(|b| b.is_ascii_digit()) {
                                self.at += 1;
                                digits += 1;
                            }
                            let text = String::from_utf8_lossy(&self.bytes[start..self.at]);
                            let byte: u8 = match text.parse() {
                                Ok(byte) => byte,
                                Err(_) => return self.error("a byte escape out of range"),
                            };
                            // Pushed as a raw byte rather than a char, so a
                            // multi-byte UTF-8 sequence written as three
                            // escapes reassembles correctly.
                            unsafe { out.as_mut_vec().push(byte) };
                            continue;
                        }
                        Some(byte) => byte as char,
                    };
                    out.push(escaped);
                    self.at += 1;
                }
                Some(_) => {
                    let start = self.at;
                    while self
                        .peek()
                        .is_some_and(|byte| byte != quote && byte != b'\\')
                    {
                        self.at += 1;
                    }
                    out.push_str(&String::from_utf8_lossy(&self.bytes[start..self.at]));
                }
            }
        }
    }

    fn long_string(&mut self) -> Result<String, ParseError> {
        self.at += 2;
        let start = self.at;
        while self.at < self.bytes.len() && !self.bytes[self.at..].starts_with(b"]]") {
            self.at += 1;
        }
        if self.at >= self.bytes.len() {
            return self.error("a long string that never ends");
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned();
        self.at += 2;
        Ok(text)
    }

    fn table(&mut self) -> Result<Value, ParseError> {
        if self.depth >= MAX_DEPTH {
            return self.error("a table nested past anything the game writes");
        }
        self.depth += 1;
        self.expect(b'{')?;

        let mut array = Vec::new();
        let mut map = BTreeMap::new();

        loop {
            self.skip_trivia();
            match self.peek() {
                None => return self.error("a table that never closes"),
                Some(b'}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Value::Table { array, map });
                }
                Some(b',') | Some(b';') => {
                    self.at += 1;
                }
                Some(b'[') => {
                    // `["key"] = value` or `[123] = value`.
                    self.at += 1;
                    self.skip_trivia();
                    let key = match self.peek() {
                        Some(b'"') | Some(b'\'') => Key::Str(self.string()?),
                        _ => match self.number()? {
                            Value::Number(number) => Key::Number(format_number(number)),
                            _ => return self.error("a table key that is not a string or number"),
                        },
                    };
                    self.skip_trivia();
                    self.expect(b']')?;
                    self.skip_trivia();
                    self.expect(b'=')?;
                    let value = self.value()?;
                    map.insert(key, value);
                }
                Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => {
                    // Either `key = value` unquoted, or a bare `nil`/`true`/
                    // `false` sitting in the array part.
                    //
                    // WoW's serializer pads a sparse array with `nil` — a table
                    // keyed by mount id starting at 6 comes out as five `nil`s
                    // and then the entries — so this is not a corner case, it is
                    // most of a real collections dump. Reading `nil` as an
                    // unquoted key and then demanding `=` is what a first
                    // version of this did, and it failed on every file the game
                    // actually writes.
                    let start = self.at;
                    let name = self.name()?;
                    self.skip_trivia();

                    if self.peek() == Some(b'=') {
                        self.at += 1;
                        let value = self.value()?;
                        map.insert(Key::Str(name), value);
                    } else {
                        self.at = start;
                        let value = self.keyword()?;
                        array.push(value);
                    }
                }
                Some(_) => {
                    // A positional entry.
                    let value = self.value()?;
                    array.push(value);
                }
            }
        }
    }
}

/// Render a number the way a key should read: `123`, not `123.0`.
fn format_number(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        format!("{number}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_variables_file_reads_into_its_globals() {
        let source = r#"
ArmoryCollectorDB = {
	["version"] = 1,
	["characters"] = {
		["Somechar-Emerald Dream"] = {
			["level"] = 80,
			["gold"] = 123456,
		},
	},
}
"#;
        let globals = parse(source).expect("parsed");
        let db = globals.get("ArmoryCollectorDB").expect("the table");
        assert_eq!(db.get("version").and_then(Value::as_u32), Some(1));

        let character = db
            .get("characters")
            .and_then(|characters| characters.get("Somechar-Emerald Dream"))
            .expect("a character");
        assert_eq!(character.get("level").and_then(Value::as_u32), Some(80));
    }

    #[test]
    fn numeric_keys_survive_as_numbers() {
        // Achievement ids are numeric keys, and reading them as strings would
        // mean re-parsing every one at every lookup.
        let globals = parse(r#"X = { [4956] = "Aeltor", [1234] = "Somechar" }"#).expect("parsed");
        let table = globals.get("X").expect("the table");

        let ids: Vec<u32> = table
            .entries()
            .filter_map(|(key, _)| key.as_u32())
            .collect();
        assert_eq!(ids, [1234, 4956]);
        assert_eq!(table.get("4956").and_then(Value::as_str), Some("Aeltor"));
    }

    #[test]
    fn both_halves_of_a_table_are_kept_apart() {
        // Lua tables are an array part and a hash part in one structure. The
        // file uses both and conflating them loses ordering.
        let globals = parse(r#"X = { "first", "second", ["name"] = "third" }"#).expect("parsed");
        let table = globals.get("X").expect("the table");
        assert_eq!(table.items().len(), 2);
        assert_eq!(table.items()[0].as_str(), Some("first"));
        assert_eq!(table.get("name").and_then(Value::as_str), Some("third"));
    }

    #[test]
    fn multi_byte_escapes_reassemble_into_utf8() {
        // The writer escapes anything above ASCII as `\ddd` bytes, and every
        // realm name with an accent arrives this way. Decoding each escape as a
        // char would produce mojibake instead of the realm's name.
        let globals = parse(r#"X = { ["realm"] = "Kh\195\182l" }"#).expect("parsed");
        assert_eq!(
            globals["X"].get("realm").and_then(Value::as_str),
            Some("Khöl")
        );
    }

    #[test]
    fn comments_are_skipped_in_both_forms() {
        let source = r#"
-- a line comment
X = { --[[ a long one ]] ["a"] = 1 }
"#;
        let globals = parse(source).expect("parsed");
        assert_eq!(globals["X"].get("a").and_then(Value::as_u32), Some(1));
    }

    #[test]
    fn unquoted_keys_and_the_usual_scalars_all_read() {
        let globals =
            parse(r#"X = { enabled = true, off = false, missing = nil, ratio = -1.5e2 }"#)
                .expect("parsed");
        let table = &globals["X"];
        assert_eq!(table.get("enabled").and_then(Value::as_bool), Some(true));
        assert_eq!(table.get("off").and_then(Value::as_bool), Some(false));
        assert_eq!(table.get("missing"), Some(&Value::Nil));
        assert_eq!(table.get("ratio").and_then(Value::as_f64), Some(-150.0));
    }

    #[test]
    fn a_sparse_array_padded_with_nil_reads() {
        // What WoW actually writes for a table keyed by mount id: positional
        // holes up to the first entry, then the entries. A parser that reads
        // `nil` as an unquoted key fails on every real collections dump.
        let globals = parse("X = { nil, nil, { \"Brown Horse\", 1 }, }").expect("parsed");
        let table = &globals["X"];
        assert_eq!(table.items().len(), 3);
        assert_eq!(table.items()[0], Value::Nil);
        assert_eq!(table.items()[2].items()[0].as_str(), Some("Brown Horse"));
    }

    #[test]
    fn bare_booleans_in_the_array_part_read_too() {
        // Same failure mode as `nil`: they start with a letter.
        let globals = parse("X = { true, false, nil }").expect("parsed");
        assert_eq!(globals["X"].items().len(), 3);
        assert_eq!(globals["X"].items()[0], Value::Bool(true));
    }

    #[test]
    fn an_unquoted_key_still_works_beside_them() {
        let globals = parse("X = { nil, enabled = true, nil }").expect("parsed");
        assert_eq!(globals["X"].items().len(), 2);
        assert_eq!(globals["X"].get("enabled"), Some(&Value::Bool(true)));
    }

    #[test]
    fn the_hybrid_shape_the_serializer_actually_emits_reads() {
        // Real dumps switch part-way: positional while the ids are dense, then
        // keyed once they are not.
        let globals = parse("X = { nil, { \"a\" }, [382] = { \"b\" }, }").expect("parsed");
        assert_eq!(globals["X"].items().len(), 2);
        assert_eq!(
            globals["X"].get("382").expect("keyed").items()[0].as_str(),
            Some("b")
        );
    }

    #[test]
    fn anything_that_is_not_data_is_refused_rather_than_evaluated() {
        // This reads a file out of a directory addon managers also write to.
        // A function call is not something to be clever about.
        assert!(parse(r#"X = os.execute("rm -rf /")"#).is_err());
        assert!(parse(r#"X = { [1] = loadstring("...") }"#).is_err());
    }

    #[test]
    fn a_truncated_file_is_an_error_and_not_a_panic() {
        // WoW has historically truncated SavedVariables on a hard exit, so this
        // is a real file that really turns up.
        let error = parse(r#"X = { ["a"] = { ["b"] = 1,"#).expect_err("truncated");
        assert!(error.message.contains("never closes"), "{error}");
    }

    #[test]
    fn a_pathologically_nested_file_is_refused_before_it_overflows_the_stack() {
        // An error is a message; a stack overflow is a crash.
        let source = format!("X = {}{}", "{".repeat(500), "}".repeat(500));
        assert!(parse(&source).is_err());
    }

    #[test]
    fn an_empty_file_has_no_globals_and_is_not_an_error() {
        assert!(parse("").expect("parsed").is_empty());
        assert!(parse("\n-- nothing here\n").expect("parsed").is_empty());
    }

    #[test]
    fn several_globals_in_one_file_all_arrive() {
        // The per-character files declare one table per saved variable.
        let globals = parse("A = { 1 }\nB = { 2 }\n").expect("parsed");
        assert_eq!(globals.len(), 2);
        assert!(globals.contains_key("A"));
        assert!(globals.contains_key("B"));
    }
}
