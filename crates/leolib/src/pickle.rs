//! The subset of Python's pickle format that Leo's uA blobs use.
//!
//! Leo stores the unknown attributes of nodes a `.leo` file does not otherwise
//! hold in a `descendentVnodeUnknownAttributes` attribute: a dict of
//! `{archived position: {uA name: value}}`, pickled with protocol 1 and
//! hexlified (`fc.pickle`). Rebuilding that attribute after an edit means
//! reading and writing the format, which is why this module exists.
//!
//! It is not a pickle implementation. [`loads`] refuses anything outside the
//! opcodes Leo's own outlines use, and [`dumps`] writes what CPython's
//! protocol 1 writes for the same value, byte for byte, so a blob that comes
//! back unchanged goes out unchanged. A value this module cannot read leaves
//! the blob where it was: see `Outline::invalidate_descendent_uas`.
//!
//! Every opcode in the 79 blobs of a leo-editor checkout is here. Floats,
//! tuples, sets and pickled class instances are not, because none of them
//! appeared; the caller falls back rather than guesses.

/// A Python value from a uA blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<Value>),
    /// Insertion-ordered, as Python's dict is: the order decides the bytes.
    Dict(Vec<(Value, Value)>),
}

impl Value {
    /// The value under `key`, for a `Dict` with string keys.
    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(items) => items
                .iter()
                .find(|(k, _)| matches!(k, Value::Str(s) if s == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string, for a `Str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The pairs, for a `Dict`.
    pub fn as_dict(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Dict(items) => Some(items),
            _ => None,
        }
    }
}

/// What a pickle this module cannot read has in it.
#[derive(Debug, PartialEq, Eq)]
pub struct Unsupported {
    pub detail: String,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported pickle: {}", self.detail)
    }
}

type Result<T> = std::result::Result<T, Unsupported>;

fn oops(detail: impl Into<String>) -> Unsupported {
    Unsupported {
        detail: detail.into(),
    }
}

// --- Reading ---------------------------------------------------------------

/// The hex string Leo writes, as a value.
pub fn unhexlify_loads(hex: &str) -> Result<Value> {
    if !hex.len().is_multiple_of(2) {
        return Err(oops("odd number of hex digits"));
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| oops(e.to_string()))?;
        bytes.push(byte);
    }
    loads(&bytes)
}

/// A pickled value, or what stopped the read.
///
/// The machine is Python's: opcodes push onto a stack, `MARK` records where a
/// group began, and the memo holds every object a later opcode may name again.
pub fn loads(bytes: &[u8]) -> Result<Value> {
    let mut stack: Vec<Value> = Vec::new();
    let mut marks: Vec<usize> = Vec::new();
    let mut memo: Vec<Value> = Vec::new();
    let mut i = 0usize;
    let put = |memo: &mut Vec<Value>, n: usize, v: Value| {
        if memo.len() <= n {
            memo.resize(n + 1, Value::None);
        }
        memo[n] = v;
    };
    while i < bytes.len() {
        let op = bytes[i];
        i += 1;
        match op {
            b'.' => {
                // STOP. Trailing bytes are not this module's business.
                return stack.pop().ok_or_else(|| oops("STOP on an empty stack"));
            }
            b'(' => marks.push(stack.len()),
            b'}' => stack.push(Value::Dict(Vec::new())),
            b']' => stack.push(Value::List(Vec::new())),
            b'N' => stack.push(Value::None),
            0x88 => stack.push(Value::Bool(true)),
            0x89 => stack.push(Value::Bool(false)),
            0x80 => {
                // PROTO: the version, which changes no opcode used here.
                i += 1;
            }
            b'K' | b'M' | b'J' => {
                let width = match op {
                    b'K' => 1,
                    b'M' => 2,
                    _ => 4,
                };
                let raw = take(bytes, &mut i, width)?;
                let n = match op {
                    b'K' => raw[0] as i64,
                    b'M' => u16::from_le_bytes([raw[0], raw[1]]) as i64,
                    _ => i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64,
                };
                stack.push(Value::Int(n));
            }
            b'I' => {
                // INT: a decimal line. Protocol 1 spells a bool this way.
                let line = take_line(bytes, &mut i)?;
                stack.push(match line.as_str() {
                    "00" => Value::Bool(false),
                    "01" => Value::Bool(true),
                    s => Value::Int(s.parse().map_err(|_| oops(format!("INT {s:?}")))?),
                });
            }
            b'L' => {
                // LONG: a decimal line with an `L` after it, which is how
                // protocol 1 writes an integer too wide for BININT.
                let line = take_line(bytes, &mut i)?;
                let digits = line.strip_suffix('L').unwrap_or(&line);
                stack.push(Value::Int(
                    digits
                        .parse()
                        .map_err(|_| oops(format!("LONG {digits:?}")))?,
                ));
            }
            b'X' | 0x8c | b'U' | b'T' => {
                // BINUNICODE and SHORT_BINUNICODE are text. BINSTRING and
                // SHORT_BINSTRING are Python 2 bytes, which is how a `.leo`
                // file written before Leo moved to Python 3 spells one.
                let width = if op == b'X' || op == b'T' { 4 } else { 1 };
                let raw = take(bytes, &mut i, width)?;
                let len = match width {
                    1 => raw[0] as usize,
                    _ => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize,
                };
                let text = take(bytes, &mut i, len)?;
                let s = std::str::from_utf8(text)
                    .map_err(|e| oops(format!("string is not UTF-8: {e}")))?;
                stack.push(Value::Str(s.to_string()));
            }
            b'q' | b'r' => {
                let width = if op == b'q' { 1 } else { 4 };
                let raw = take(bytes, &mut i, width)?;
                let n = match width {
                    1 => raw[0] as usize,
                    _ => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize,
                };
                let top = stack.last().ok_or_else(|| oops("PUT on an empty stack"))?;
                put(&mut memo, n, top.clone());
            }
            b'h' | b'j' => {
                let width = if op == b'h' { 1 } else { 4 };
                let raw = take(bytes, &mut i, width)?;
                let n = match width {
                    1 => raw[0] as usize,
                    _ => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize,
                };
                let v = memo.get(n).cloned().ok_or_else(|| oops("GET past memo"))?;
                stack.push(v);
            }
            b's' => {
                let value = stack.pop().ok_or_else(|| oops("SETITEM without value"))?;
                let key = stack.pop().ok_or_else(|| oops("SETITEM without key"))?;
                set_items(&mut stack, vec![(key, value)])?;
            }
            b'u' => {
                let items = pop_mark(&mut stack, &mut marks, "SETITEMS")?;
                if items.len() % 2 != 0 {
                    return Err(oops("SETITEMS with an odd group"));
                }
                let pairs = items
                    .chunks(2)
                    .map(|kv| (kv[0].clone(), kv[1].clone()))
                    .collect();
                set_items(&mut stack, pairs)?;
            }
            b'a' => {
                let value = stack.pop().ok_or_else(|| oops("APPEND without value"))?;
                append_items(&mut stack, vec![value])?;
            }
            b'e' => {
                let items = pop_mark(&mut stack, &mut marks, "APPENDS")?;
                append_items(&mut stack, items)?;
            }
            other => {
                return Err(oops(format!(
                    "opcode {:?} at byte {}",
                    other as char,
                    i - 1
                )))
            }
        }
    }
    Err(oops("no STOP opcode"))
}

fn take<'a>(bytes: &'a [u8], i: &mut usize, n: usize) -> Result<&'a [u8]> {
    let end = i.checked_add(n).ok_or_else(|| oops("length overflow"))?;
    let slice = bytes.get(*i..end).ok_or_else(|| oops("truncated"))?;
    *i = end;
    Ok(slice)
}

fn take_line(bytes: &[u8], i: &mut usize) -> Result<String> {
    let start = *i;
    while *i < bytes.len() && bytes[*i] != b'\n' {
        *i += 1;
    }
    if *i >= bytes.len() {
        return Err(oops("unterminated line"));
    }
    let line = std::str::from_utf8(&bytes[start..*i])
        .map_err(|e| oops(e.to_string()))?
        .to_string();
    *i += 1;
    Ok(line)
}

fn pop_mark(stack: &mut Vec<Value>, marks: &mut Vec<usize>, op: &str) -> Result<Vec<Value>> {
    let mark = marks
        .pop()
        .ok_or_else(|| oops(format!("{op} without MARK")))?;
    if mark > stack.len() {
        return Err(oops(format!("{op} past the stack")));
    }
    Ok(stack.split_off(mark))
}

fn set_items(stack: &mut [Value], pairs: Vec<(Value, Value)>) -> Result<()> {
    match stack.last_mut() {
        Some(Value::Dict(items)) => {
            for (key, value) in pairs {
                match items.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = value,
                    None => items.push((key, value)),
                }
            }
            Ok(())
        }
        _ => Err(oops("SETITEM on something that is not a dict")),
    }
}

fn append_items(stack: &mut [Value], values: Vec<Value>) -> Result<()> {
    match stack.last_mut() {
        Some(Value::List(items)) => {
            items.extend(values);
            Ok(())
        }
        _ => Err(oops("APPEND on something that is not a list")),
    }
}

// --- Writing ---------------------------------------------------------------

/// The value as the hex string Leo writes, as `fc.pickle` spells it.
pub fn dumps_hexlify(v: &Value) -> String {
    dumps(v).iter().map(|b| format!("{b:02x}")).collect()
}

/// The value as CPython's `pickle.dumps(v, protocol=1)` writes it.
///
/// Protocol 1 because that is what Leo writes, and matching it byte for byte
/// is what keeps a blob that nothing changed out of the diff. A dict or list
/// of one item takes `SETITEM`/`APPEND` where a longer one takes a `MARK` and
/// `SETITEMS`/`APPENDS`, and every string, dict and list is memoized in the
/// order it is written, as CPython's `save` does.
pub fn dumps(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    let mut memo = 0usize;
    save(&mut out, &mut memo, v);
    out.push(b'.');
    out
}

fn save(out: &mut Vec<u8>, memo: &mut usize, v: &Value) {
    match v {
        Value::None => out.push(b'N'),
        // Protocol 1 has no NEWTRUE: a bool goes out as an INT line.
        Value::Bool(b) => out.extend_from_slice(if *b { b"I01\n" } else { b"I00\n" }),
        Value::Int(n) => save_int(out, *n),
        Value::Str(s) => {
            out.push(b'X');
            out.extend_from_slice(&(s.len() as u32).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
            save_memo(out, memo);
        }
        Value::List(items) => {
            out.push(b']');
            save_memo(out, memo);
            match items.len() {
                0 => {}
                1 => {
                    save(out, memo, &items[0]);
                    out.push(b'a');
                }
                _ => {
                    out.push(b'(');
                    for item in items {
                        save(out, memo, item);
                    }
                    out.push(b'e');
                }
            }
        }
        Value::Dict(items) => {
            out.push(b'}');
            save_memo(out, memo);
            match items.len() {
                0 => {}
                1 => {
                    save(out, memo, &items[0].0);
                    save(out, memo, &items[0].1);
                    out.push(b's');
                }
                _ => {
                    out.push(b'(');
                    for (key, value) in items {
                        save(out, memo, key);
                        save(out, memo, value);
                    }
                    out.push(b'u');
                }
            }
        }
    }
}

fn save_int(out: &mut Vec<u8>, n: i64) {
    match n {
        0..=0xff => {
            out.push(b'K');
            out.push(n as u8);
        }
        0x100..=0xffff => {
            out.push(b'M');
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        _ if i32::try_from(n).is_ok() => {
            out.push(b'J');
            out.extend_from_slice(&(n as i32).to_le_bytes());
        }
        // Outside BININT: LONG, as protocol 1 writes an integer this wide.
        _ => {
            out.push(b'L');
            out.extend_from_slice(n.to_string().as_bytes());
            out.extend_from_slice(b"L\n");
        }
    }
}

fn save_memo(out: &mut Vec<u8>, memo: &mut usize) {
    let n = *memo;
    *memo += 1;
    if n < 256 {
        out.push(b'q');
        out.push(n as u8);
    } else {
        out.push(b'r');
        out.extend_from_slice(&(n as u32).to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blobs taken from a leo-editor checkout, with what Python unpickles
    /// them to. `leo/test/test.leo`, `leo/dist/leoDist.leo`.
    const REAL: &[&str] = &[
        "7d710058010000003071017d7102580b0000007374725f6c656f5f706f7371035804000000332c3131710473732e",
        "7d7100285803000000302e3171017d7102580b0000005f5f626f6f6b6d61726b7371037d7104580700000069735f6475706571054930300a73735803000000302e3271067d7107580b0000005f5f626f6f6b6d61726b7371087d7109580700000069735f64757065710a4930300a73735803000000302e35710b7d710c580b0000005f5f626f6f6b6d61726b73710d7d710e580700000069735f64757065710f4930300a73735804000000302e313171107d7111580b0000005f5f626f6f6b6d61726b7371127d7113580700000069735f6475706571144930300a7373752e",
    ];

    fn str_key(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    #[test]
    fn a_real_blob_reads_as_python_unpickles_it() {
        let got = unhexlify_loads(REAL[0]).unwrap();
        assert_eq!(
            got,
            Value::Dict(vec![(
                str_key("0"),
                Value::Dict(vec![(str_key("str_leo_pos"), str_key("3,11"))])
            )])
        );

        let got = unhexlify_loads(REAL[1]).unwrap();
        let items = got.as_dict().unwrap();
        let keys: Vec<&str> = items.iter().map(|(k, _)| k.as_str().unwrap()).collect();
        assert_eq!(keys, vec!["0.1", "0.2", "0.5", "0.11"]);
        assert_eq!(
            items[0].1.get("__bookmarks").unwrap().get("is_dupe"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn a_real_blob_is_written_back_byte_for_byte() {
        // CPython's `pickle.dumps(value, protocol=1)` on these values gives
        // the same bytes, so a blob nothing changed leaves no diff.
        for hex in REAL {
            let value = unhexlify_loads(hex).unwrap();
            assert_eq!(dumps_hexlify(&value), *hex);
        }
    }

    #[test]
    fn every_supported_value_survives_a_round_trip() {
        let value = Value::Dict(vec![
            (str_key("none"), Value::None),
            (str_key("true"), Value::Bool(true)),
            (str_key("false"), Value::Bool(false)),
            (str_key("small"), Value::Int(7)),
            (str_key("medium"), Value::Int(300)),
            (str_key("large"), Value::Int(70_000)),
            (str_key("huge"), Value::Int(i64::MAX)),
            (str_key("negative"), Value::Int(-5)),
            (str_key("text"), str_key("caf\u{e9} \" < &")),
            (str_key("empty list"), Value::List(Vec::new())),
            (str_key("one"), Value::List(vec![Value::Int(1)])),
            (
                str_key("many"),
                Value::List(vec![Value::Int(1), str_key("two"), Value::None]),
            ),
            (str_key("empty dict"), Value::Dict(Vec::new())),
        ]);
        assert_eq!(loads(&dumps(&value)).unwrap(), value);
    }

    #[test]
    fn a_python_2_string_reads_as_text() {
        // SHORT_BINSTRING, as a .leo file written by Python 2 Leo spells it.
        let mut bytes = vec![
            b'}', b'q', 0, b'U', 1, b'k', b'q', 1, b'U', 1, b'v', b'q', 2,
        ];
        bytes.extend_from_slice(b"s.");
        assert_eq!(
            loads(&bytes).unwrap(),
            Value::Dict(vec![(str_key("k"), str_key("v"))])
        );
    }

    #[test]
    fn a_memo_reference_reads_as_the_object_it_names() {
        // BINGET, which CPython emits for an object it has already written.
        let mut bytes = vec![b'}', b'q', 0, b'('];
        bytes.extend_from_slice(&[b'U', 1, b'a', b'q', 1]); // "a", memo 1
        bytes.extend_from_slice(&[b'U', 1, b'x', b'q', 2]); // "x", memo 2
        bytes.extend_from_slice(&[b'U', 1, b'b', b'q', 3]); // "b", memo 3
        bytes.extend_from_slice(&[b'h', 2]); // the same "x"
        bytes.extend_from_slice(b"u.");
        assert_eq!(
            loads(&bytes).unwrap(),
            Value::Dict(vec![
                (str_key("a"), str_key("x")),
                (str_key("b"), str_key("x")),
            ])
        );
    }

    #[test]
    fn a_value_this_module_cannot_read_is_refused() {
        // GLOBAL: a pickled class instance, which nothing here can rebuild.
        let err = loads(b"cleo.core.leoNodes\nVNode\nq\x00.").unwrap_err();
        assert!(err.to_string().contains("unsupported pickle"), "{err}");
        assert!(loads(b"}q\x00").is_err(), "no STOP");
        assert!(unhexlify_loads("7d7").is_err(), "odd hex");
    }
}
