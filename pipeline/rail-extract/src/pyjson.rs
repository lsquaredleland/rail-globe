//! JSON written the way Python's json module writes it, so files from the Rust build compare byte for byte with the Python
//! pipeline's: key order as inserted (serde_json preserve_order), floats as Python's repr ("12.0", "1e-05"), strings escaped as
//! json.dumps does (ensure_ascii True or False), and Python's two separator styles.

use serde_json::{Map, Value};

/// separators and escaping of one json.dumps call
#[derive(Clone, Copy)]
pub struct Style {
    pub ascii: bool,         // ensure_ascii
    pub compact: bool,       // separators=(",", ":"); else Python's default (", ", ": ")
}

/// json.dumps(..., ensure_ascii=False, separators=(",", ":"))
pub const COMPACT: Style = Style { ascii: false, compact: true };
/// json.dump(..., ensure_ascii=False) with the default separators
pub const SPACED: Style = Style { ascii: false, compact: false };
/// json.dumps(..., sort_keys=True) defaults: ASCII escapes, default separators (keys are sorted by the caller)
pub const ASCII_SPACED: Style = Style { ascii: true, compact: false };

/// Python's repr of a float: the shortest digits that round-trip, positional between 1e-4 and 1e16, else exponent form
pub fn py_float(out: &mut String, x: f64) {
    if x == 0.0 {
        out.push_str(if x.is_sign_negative() { "-0.0" } else { "0.0" });
        return;
    }
    if !x.is_finite() {
        out.push_str(if x.is_nan() { "NaN" } else if x > 0.0 { "Infinity" } else { "-Infinity" });
        return;
    }
    let e = format!("{:e}", x); // shortest round-trip digits, e.g. "-1.2345e2", "5e-5"
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    if neg {
        out.push('-');
    }
    if (-4..16).contains(&exp) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(&digits);
                out.extend(std::iter::repeat('0').take(int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat('0').take((-exp - 1) as usize));
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exp.abs()));
    }
}

/// Python's round(x, n): the float nearest to x rounded to n decimals (ties to even on the exact binary value)
pub fn py_round(x: f64, n: usize) -> f64 {
    format!("{:.*}", n, x).parse().unwrap()
}

pub fn py_str(out: &mut String, s: &str, ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ascii && (c as u32) >= 0x80 => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn write(out: &mut String, v: &Value, st: Style) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                py_float(out, n.as_f64().unwrap());
            }
        }
        Value::String(s) => py_str(out, s, st.ascii),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(if st.compact { "," } else { ", " });
                }
                write(out, x, st);
            }
            out.push(']');
        }
        Value::Object(m) => write_map(out, m, st),
    }
}

pub fn write_map(out: &mut String, m: &Map<String, Value>, st: Style) {
    out.push('{');
    for (i, (k, x)) in m.iter().enumerate() {
        if i > 0 {
            out.push_str(if st.compact { "," } else { ", " });
        }
        py_str(out, k, st.ascii);
        out.push_str(if st.compact { ":" } else { ": " });
        write(out, x, st);
    }
    out.push('}');
}

pub fn to_string(v: &Value, st: Style) -> String {
    let mut s = String::new();
    write(&mut s, v, st);
    s
}

/// json.dumps(obj, sort_keys=True): keys sorted at every level, ASCII escapes, default separators
pub fn sorted_key(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut ks: Vec<&String> = m.keys().collect();
                ks.sort();
                Value::Object(ks.into_iter().map(|k| (k.clone(), sort(&m[k]))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            x => x.clone(),
        }
    }
    to_string(&sort(v), ASCII_SPACED)
}

/// json.dump(obj, indent=2) with ensure_ascii True (the stations report)
pub fn pretty_ascii(v: &Value) -> String {
    fn go(out: &mut String, v: &Value, ind: usize) {
        let pad = |out: &mut String, n: usize| out.extend(std::iter::repeat(' ').take(n));
        match v {
            Value::Object(m) if !m.is_empty() => {
                out.push('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    out.push_str(if i > 0 { ",\n" } else { "\n" });
                    pad(out, ind + 2);
                    py_str(out, k, true);
                    out.push_str(": ");
                    go(out, x, ind + 2);
                }
                out.push('\n');
                pad(out, ind);
                out.push('}');
            }
            Value::Array(a) if !a.is_empty() => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    out.push_str(if i > 0 { ",\n" } else { "\n" });
                    pad(out, ind + 2);
                    go(out, x, ind + 2);
                }
                out.push('\n');
                pad(out, ind);
                out.push(']');
            }
            x => write(out, x, ASCII_SPACED),
        }
    }
    let mut s = String::new();
    go(&mut s, v, 0);
    s
}

/// a float value (Python float), as opposed to Value::from(i64) for Python ints
pub fn f(x: f64) -> Value {
    Value::from(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floats_like_python() {
        let cases = [(12.0, "12.0"), (0.1, "0.1"), (1e-5, "1e-05"), (1e16, "1e+16"), (123456.789, "123456.789"), (0.0001, "0.0001"),
                     (-2.5, "-2.5"), (1.5e-7, "1.5e-07"), (114.16562, "114.16562"), (9999999999999998.0, "9999999999999998.0")];
        for (x, want) in cases {
            let mut s = String::new();
            py_float(&mut s, x);
            assert_eq!(s, want, "{x}");
        }
        assert_eq!(py_round(2.675, 2), 2.67);
        assert_eq!(py_round(0.125, 2), 0.12);
    }
}
