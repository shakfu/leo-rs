//! JSON text as Python's `json.dumps` writes it.
//!
//! nbformat writes a notebook with `sort_keys=True, indent=1,
//! ensure_ascii=False`; jupytext writes cell metadata with the defaults.
//! Keys are sorted here explicitly: serde_json keeps insertion order when
//! another crate in the build enables its `preserve_order` feature.

use serde_json::Value;

/// A notebook as nbformat's `writes` spells it, without the final newline.
pub fn dumps_notebook(v: &Value) -> String {
    let mut out = String::new();
    pretty(v, 0, &mut out);
    out
}

fn pretty(v: &Value, depth: usize, out: &mut String) {
    match v {
        Value::Array(a) if !a.is_empty() => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(depth + 1, out);
                pretty(x, depth + 1, out);
            }
            out.push('\n');
            indent(depth, out);
            out.push(']');
        }
        Value::Object(m) if !m.is_empty() => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(depth + 1, out);
                string(k, false, out);
                out.push_str(": ");
                pretty(&m[k], depth + 1, out);
            }
            out.push('\n');
            indent(depth, out);
            out.push('}');
        }
        _ => scalar(v, false, out),
    }
}

fn indent(depth: usize, out: &mut String) {
    out.extend(std::iter::repeat_n(' ', depth));
}

/// `json.dumps(v)`: one line, `", "` and `": "` separators, ASCII only.
/// Keys keep the map's order, as Python keeps a dict's.
pub fn dumps(v: &Value) -> String {
    let mut out = String::new();
    compact(v, &mut out);
    out
}

fn compact(v: &Value, out: &mut String) {
    match v {
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                compact(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                string(k, true, out);
                out.push_str(": ");
                compact(x, out);
            }
            out.push('}');
        }
        _ => scalar(v, true, out),
    }
}

fn scalar(v: &Value, ascii: bool, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match n.as_f64() {
            Some(f) if !n.is_i64() && !n.is_u64() => out.push_str(&float_repr(f)),
            _ => out.push_str(&n.to_string()),
        },
        Value::String(s) => string(s, ascii, out),
        // Collections reach here only when empty.
        Value::Array(_) => out.push_str("[]"),
        Value::Object(_) => out.push_str("{}"),
    }
}

fn string(s: &str, ascii: bool, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (ascii && !(' '..='~').contains(&c)) => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Python's `repr` of a float: the shortest digits that read back, in
/// exponent form below 1e-4 or from 1e16.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let e = format!("{:e}", x.abs());
    let (mantissa, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits = mantissa.replace('.', "");
    let decpt = exp + 1;
    let body = if decpt <= -4 || decpt > 16 {
        let m = match digits.len() {
            1 => digits.clone(),
            _ => format!("{}.{}", &digits[..1], &digits[1..]),
        };
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{m}e{sign}{:02}", exp.abs())
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt as usize >= digits.len() {
        format!("{digits}{}.0", "0".repeat(decpt as usize - digits.len()))
    } else {
        let d = decpt as usize;
        format!("{}.{}", &digits[..d], &digits[d..])
    };
    match x < 0.0 {
        true => format!("-{body}"),
        false => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_are_spelled_as_python_spells_them() {
        let cases = [
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1.5, "1.5"),
            (-2.0, "-2.0"),
            (123.456, "123.456"),
            (1.2345e-7, "1.2345e-07"),
            (6.02e23, "6.02e+23"),
        ];
        for (x, want) in cases {
            assert_eq!(float_repr(x), want, "{x}");
        }
    }

    #[test]
    fn strings_escape_as_python_does() {
        let v = Value::String("a\"\\\n\u{1}\u{e9}\u{1f600}".to_string());
        assert_eq!(dumps(&v), "\"a\\\"\\\\\\n\\u0001\\u00e9\\ud83d\\ude00\"");
        assert_eq!(dumps_notebook(&v), "\"a\\\"\\\\\\n\\u0001\u{e9}\u{1f600}\"");
    }

    #[test]
    fn a_notebook_is_indented_by_one_with_sorted_keys() {
        let v: Value = serde_json::from_str(r#"{"b": [1, {}], "a": {"c": []}}"#).unwrap();
        assert_eq!(
            dumps_notebook(&v),
            "{\n \"a\": {\n  \"c\": []\n },\n \"b\": [\n  1,\n  {}\n ]\n}"
        );
    }
}
