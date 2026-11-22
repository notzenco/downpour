use percent_encoding::percent_decode_str;
use std::collections::HashSet;
use std::path::Path;

/// Extracts the file name from a `Content-Disposition` header, preferring the
/// RFC 5987 `filename*` form over the plain `filename` parameter.
pub fn from_content_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for part in split_params(header) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if key == "filename*" {
            // charset'language'percent-encoded
            let encoded = value.splitn(3, '\'').nth(2).unwrap_or(value);
            let decoded = percent_decode_str(unquote(encoded).as_str())
                .decode_utf8_lossy()
                .into_owned();
            if !decoded.is_empty() {
                extended = Some(decoded);
            }
        } else if key == "filename" {
            let v = unquote(value);
            if !v.is_empty() {
                plain = Some(v);
            }
        }
    }
    extended.or(plain)
}

fn split_params(header: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    for c in header.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_quotes => {
                current.push(c);
                escaped = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            ';' if !in_quotes => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        v.to_string()
    }
}

/// The last non-empty path segment of a URL, percent-decoded.
pub fn from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let segment = parsed.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode_str(segment).decode_utf8_lossy().into_owned();
    (!decoded.is_empty()).then_some(decoded)
}

pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let essence = mime.split(';').next()?.trim().to_ascii_lowercase();
    Some(match essence.as_str() {
        "application/zip" => "zip",
        "application/pdf" => "pdf",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" | "application/x-rar-compressed" => "rar",
        "application/gzip" => "gz",
        "application/x-tar" => "tar",
        "application/x-msdownload" => "exe",
        "application/x-apple-diskimage" => "dmg",
        "application/json" => "json",
        "text/plain" => "txt",
        "text/html" => "html",
        "text/csv" => "csv",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "audio/mpeg" => "mp3",
        "audio/flac" => "flac",
        "audio/ogg" => "ogg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        _ => return None,
    })
}

const MAX_NAME_BYTES: usize = 200;

/// Makes a server- or user-supplied name safe to create on Windows, macOS and Linux.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let mut cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        return "download".to_string();
    }

    let stem_upper = cleaned
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let reserved = matches!(stem_upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem_upper.starts_with("COM") || stem_upper.starts_with("LPT"))
            && stem_upper.len() == 4
            && stem_upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        cleaned.insert(0, '_');
    }

    if cleaned.len() > MAX_NAME_BYTES {
        let (stem, ext) = split_ext(&cleaned);
        let budget = MAX_NAME_BYTES.saturating_sub(ext.len());
        let mut cut = budget.min(stem.len());
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        cleaned = format!("{}{}", &stem[..cut], ext);
    }
    cleaned
}

/// Splits `name` into stem and extension (with its dot), treating `.tar.*` as one extension.
fn split_ext(name: &str) -> (&str, &str) {
    let lower = name.to_ascii_lowercase();
    for compound in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"] {
        if lower.ends_with(compound) && lower.len() > compound.len() {
            let at = name.len() - compound.len();
            return (&name[..at], &name[at..]);
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Returns `name`, or `name (n)`, such that neither the file nor its `.part`
/// exists in `dir` and no other download has claimed it.
pub fn unique_name(dir: &Path, name: &str, claimed: &HashSet<String>) -> String {
    let free = |candidate: &str| {
        !claimed.contains(candidate)
            && !dir.join(candidate).exists()
            && !dir.join(format!("{candidate}.part")).exists()
    };
    if free(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    (1..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|c| free(c))
        .expect("an unbounded range always yields a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_prefers_extended_filename() {
        let h = r#"attachment; filename="fallback.zip"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"#;
        assert_eq!(from_content_disposition(h).as_deref(), Some("résumé.pdf"));
    }

    #[test]
    fn content_disposition_handles_quotes_and_semicolons() {
        let h = r#"attachment; filename="a; b \"c\".txt""#;
        assert_eq!(
            from_content_disposition(h).as_deref(),
            Some(r#"a; b "c".txt"#)
        );
        assert_eq!(from_content_disposition("inline").as_deref(), None);
        assert_eq!(
            from_content_disposition("attachment; filename=plain.bin").as_deref(),
            Some("plain.bin")
        );
    }

    #[test]
    fn url_name_is_decoded_last_segment() {
        assert_eq!(
            from_url("https://x.org/a/My%20File.iso?x=1").as_deref(),
            Some("My File.iso")
        );
        assert_eq!(from_url("https://x.org/dir/").as_deref(), Some("dir"));
        assert_eq!(from_url("https://x.org/").as_deref(), None);
    }

    #[test]
    fn sanitize_strips_unsafe_characters_and_reserved_names() {
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("  ...  "), "download");
        let long = format!("{}.mkv", "x".repeat(500));
        let s = sanitize(&long);
        assert!(s.len() <= MAX_NAME_BYTES && s.ends_with(".mkv"));
    }

    #[test]
    fn unique_name_skips_existing_and_claimed() {
        let dir = std::env::temp_dir().join(format!("downpour-naming-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.tar.gz"), b"").unwrap();
        std::fs::write(dir.join("a (1).tar.gz.part"), b"").unwrap();
        let claimed: HashSet<String> = ["a (2).tar.gz".to_string()].into();
        assert_eq!(unique_name(&dir, "a.tar.gz", &claimed), "a (3).tar.gz");
        assert_eq!(unique_name(&dir, "b.zip", &claimed), "b.zip");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
