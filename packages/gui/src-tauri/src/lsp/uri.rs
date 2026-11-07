//! File identity for the language-server client: paths to `file:` URIs and back, and one comparable key for both.
//!
//! A server may spell the same file as `file:///c%3A/Users/…`, `file:///C:/Users/…` or with `%20` for a space, so a diagnostics lookup keyed on the
//! string the client sent would silently miss. Both sides go through [`path_key`] instead.

/// Resolve `.` and `..` and turn backslashes into slashes, without touching the disk.
fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/");
    let rooted = p.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") && !(parts.len() == 1 && is_drive(parts[0])) {
                    parts.pop();
                } else if !rooted && !parts.last().is_some_and(|l| is_drive(l)) {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    if rooted { format!("/{joined}") } else { joined }
}

fn is_drive(seg: &str) -> bool {
    let b = seg.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

fn encode_segment(seg: &str, out: &mut String) {
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
}

/// The `file:` URI for a filesystem path: normalized, drive letter kept as `C:`, every other special byte percent-encoded.
pub fn path_to_uri(path: &str) -> String {
    let n = normalize(path);
    let n = n.trim_start_matches('/');
    let mut out = String::from("file:///");
    for (i, seg) in n.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        if i == 0 && is_drive(seg) {
            out.push_str(seg);
        } else {
            encode_segment(seg, &mut out);
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The filesystem path a `file:` URI names (a plain path passes through).
pub fn uri_to_path(uri: &str) -> String {
    let rest = match uri.strip_prefix("file://") {
        Some(r) => percent_decode(r),
        None => return uri.replace('\\', "/"),
    };
    let rest = rest.replace('\\', "/");
    let b = rest.as_bytes();
    // `/C:/x` → `C:/x`
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        return rest[1..].to_string();
    }
    rest
}

/// One comparable identity for a path or a `file:` URI: decoded, normalized, and case-folded where the filesystem is.
pub fn path_key(path_or_uri: &str) -> String {
    let p = normalize(&uri_to_path(path_or_uri));
    if cfg!(windows) || p.as_bytes().get(1) == Some(&b':') { p.to_lowercase() } else { p }
}

/// An LSP column counts UTF-16 units by default; `char_col` counts characters of `line`.
pub fn utf16_col(line: &str, char_col: u32) -> u32 {
    line.chars().take(char_col as usize).map(|c| c.len_utf16() as u32).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_path_with_a_space_is_encoded_and_round_trips() {
        let uri = path_to_uri(r"C:\Users\v3dqn\Work\Project Versa\src\a.rs");
        assert_eq!(uri, "file:///C:/Users/v3dqn/Work/Project%20Versa/src/a.rs");
        assert_eq!(uri_to_path(&uri), "C:/Users/v3dqn/Work/Project Versa/src/a.rs");
    }

    #[test]
    fn every_spelling_a_server_may_use_gives_the_same_key() {
        let want = path_key(r"C:\Users\x\Project Versa\a.ts");
        for spelling in ["file:///C:/Users/x/Project%20Versa/a.ts", "file:///c%3A/Users/x/Project%20Versa/a.ts", "file:///c:/Users/x/Project Versa/a.ts", "C:/Users/x/Project Versa/./b/../a.ts"] {
            assert_eq!(path_key(spelling), want, "{spelling}");
        }
    }

    #[test]
    fn unix_paths_and_non_ascii_names_encode() {
        assert_eq!(path_to_uri("/home/u/a b/é.py"), "file:///home/u/a%20b/%C3%A9.py");
        assert_eq!(uri_to_path("file:///home/u/a%20b/%C3%A9.py"), "/home/u/a b/é.py");
    }

    #[test]
    fn columns_count_utf16_units() {
        assert_eq!(utf16_col("a😀b", 2), 3);
        assert_eq!(utf16_col("abc", 9), 3);
    }
}
