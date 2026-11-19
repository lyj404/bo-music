//! Desktop signing behavior, retained for compatibility with the existing client.

const SALT: &str = "kuwotest";

pub fn form_url_encode(pairs: &[(String, String)]) -> String {
    fn component(input: &str) -> String {
        let mut out = String::new();
        for b in input.bytes() {
            match b {
                b' ' => out.push('+'),
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                    out.push(b as char)
                }
                _ => {
                    use std::fmt::Write;
                    write!(out, "%{b:02X}").expect("writing to String cannot fail");
                }
            }
        }
        out
    }
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", component(k), component(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub fn sign_raw(path: &str, encoded_query: &str, body: Option<&str>) -> String {
    let mut chars: Vec<u8> = encoded_query
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .collect();
    chars.sort_unstable();
    let mut seed = String::from(SALT);
    for c in chars {
        seed.push(c as char);
    }
    if let Some(body) = body.filter(|s| !s.is_empty()) {
        seed.push_str(&format!("{:x}", md5::compute(format!("{body}{SALT}"))));
    }
    seed.push_str(path);
    format!("{:x}", md5::compute(seed))
}

#[cfg(test)]
#[path = "signer_tests.rs"]
mod tests;
