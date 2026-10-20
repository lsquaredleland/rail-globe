//! JSON writing helpers. Output matches the old cpp/filter.cpp (git commit 80b9620) byte for byte where it can (coordinates, escaping, key order).

/// append a non-negative integer
#[inline]
pub fn uint(out: &mut Vec<u8>, mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[i..]);
}

#[inline]
pub fn int(out: &mut Vec<u8>, v: i64) {
    if v < 0 {
        out.push(b'-');
    }
    uint(out, v.unsigned_abs());
}

/// 1e-7 degrees -> filter.cpp num(): printf("%.5f") of the double, trailing zeros and point trimmed, "-0" written as "0".
/// Integer rounding gives the same digits except on an exact decimal tie (|v| % 100 == 50), where printf rounds the binary
/// value of the double; those go through the float formatter (which is exact, like printf).
pub fn num(out: &mut Vec<u8>, v: i32) {
    let a = (v as i64).abs();
    if a % 100 == 50 {
        let s = format!("{:.5}", v as f64 / 1e7);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        out.extend_from_slice(if s == "-0" { b"0" } else { s.as_bytes() });
        return;
    }
    let r = (a + 50) / 100; // units of 1e-5, rounded half away from zero
    if r == 0 {
        out.push(b'0');
        return;
    }
    if v < 0 {
        out.push(b'-');
    }
    uint(out, (r / 100_000) as u64);
    let mut frac = r % 100_000;
    if frac != 0 {
        let mut digits = 5;
        while frac % 10 == 0 {
            frac /= 10;
            digits -= 1;
        }
        out.push(b'.');
        let s = frac.to_string();
        for _ in s.len()..digits {
            out.push(b'0');
        }
        out.extend_from_slice(s.as_bytes());
    }
}

/// a coordinate as Python's json writes round(x, 5): like `num`, but a whole number keeps ".0"
pub fn pynum(out: &mut Vec<u8>, v: i32) {
    let start = out.len();
    num(out, v);
    if !out[start..].contains(&b'.') {
        out.extend_from_slice(b".0");
    }
}

/// [lon,lat]
#[inline]
pub fn pt(out: &mut Vec<u8>, lat: i32, lon: i32) {
    out.push(b'[');
    num(out, lon);
    out.push(b',');
    num(out, lat);
    out.push(b']');
}

/// a JSON string, escaped as filter.cpp jstr() does (other bytes, including UTF-8, pass through)
pub fn js(out: &mut Vec<u8>, s: &[u8]) {
    out.push(b'"');
    let mut start = 0;
    for (i, &c) in s.iter().enumerate() {
        if c >= 0x20 && c != b'"' && c != b'\\' {
            continue;
        }
        out.extend_from_slice(&s[start..i]);
        match c {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            _ => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.extend_from_slice(b"\\u00");
                out.push(HEX[(c >> 4) as usize]);
                out.push(HEX[(c & 15) as usize]);
            }
        }
        start = i + 1;
    }
    out.extend_from_slice(&s[start..]);
    out.push(b'"');
}

pub fn js_opt(out: &mut Vec<u8>, s: Option<&[u8]>) {
    match s {
        Some(s) => js(out, s),
        None => out.extend_from_slice(b"null"),
    }
}

// ---------------------------------------------------------------- a small ordered JSON value, written like Python's json.dump(indent=2)

pub enum J {
    Null,
    Int(i64),
    /// a number already written as JSON text (a fixed-decimals float)
    Raw(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn obj(v: Vec<(&str, J)>) -> J {
        J::Obj(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
    pub fn s(v: impl Into<String>) -> J {
        J::Str(v.into())
    }
    pub fn opt(v: Option<String>) -> J {
        v.map_or(J::Null, J::Str)
    }

    pub fn write_pretty(&self, out: &mut Vec<u8>, ind: usize) {
        match self {
            J::Null => out.extend_from_slice(b"null"),
            J::Int(i) => int(out, *i),
            J::Raw(s) => out.extend_from_slice(s.as_bytes()),
            J::Str(s) => py_str(out, s),
            J::Arr(v) if v.is_empty() => out.extend_from_slice(b"[]"),
            J::Obj(v) if v.is_empty() => out.extend_from_slice(b"{}"),
            J::Arr(v) => {
                out.push(b'[');
                for (i, x) in v.iter().enumerate() {
                    out.extend_from_slice(if i > 0 { b",\n" } else { b"\n" });
                    out.extend(std::iter::repeat_n(b' ', ind + 2));
                    x.write_pretty(out, ind + 2);
                }
                out.push(b'\n');
                out.extend(std::iter::repeat_n(b' ', ind));
                out.push(b']');
            }
            J::Obj(v) => {
                out.push(b'{');
                for (i, (k, x)) in v.iter().enumerate() {
                    out.extend_from_slice(if i > 0 { b",\n" } else { b"\n" });
                    out.extend(std::iter::repeat_n(b' ', ind + 2));
                    py_str(out, k);
                    out.extend_from_slice(b": ");
                    x.write_pretty(out, ind + 2);
                }
                out.push(b'\n');
                out.extend(std::iter::repeat_n(b' ', ind));
                out.push(b'}');
            }
        }
    }
}

/// a string as Python's json writes it with ensure_ascii=False
pub fn py_str(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            c if (c as u32) < 0x20 => out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes()),
            c => {
                let mut b = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            }
        }
    }
    out.push(b'"');
}
