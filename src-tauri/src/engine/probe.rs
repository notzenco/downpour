use reqwest::header::{self, HeaderMap};
use reqwest::{Client, StatusCode};
use std::collections::BTreeMap;

use super::category::Category;
use super::error::EngineError;
use super::model::ProbeInfo;
use super::naming;

pub struct RequestOptions<'a> {
    pub user_agent: &'a str,
    pub referer: Option<&'a str>,
    pub headers: &'a BTreeMap<String, String>,
}

/// Headers the engine sets itself; a caller overriding them would corrupt ranges or framing.
const RESERVED_HEADERS: [&str; 9] = [
    "host",
    "range",
    "accept-encoding",
    "content-length",
    "connection",
    "transfer-encoding",
    "te",
    "upgrade",
    "keep-alive",
];

/// Keeps only headers that are valid and not reserved by the engine.
pub fn sanitize_headers(headers: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(k, v)| {
            let name = k.trim().to_ascii_lowercase();
            !RESERVED_HEADERS.contains(&name.as_str())
                && !name.starts_with("proxy-")
                && header::HeaderName::from_bytes(name.as_bytes()).is_ok()
                && header::HeaderValue::from_str(v.trim()).is_ok()
        })
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

pub fn request(client: &Client, url: &str, opts: &RequestOptions<'_>) -> reqwest::RequestBuilder {
    // Compressed transfer would make byte ranges refer to the encoded body.
    let mut req = client.get(url).header(header::ACCEPT_ENCODING, "identity");
    if !opts
        .headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("user-agent"))
    {
        req = req.header(header::USER_AGENT, opts.user_agent);
    }
    if let Some(r) = opts.referer {
        req = req.header(header::REFERER, r);
    }
    for (name, value) in opts.headers {
        if let (Ok(n), Ok(v)) = (
            header::HeaderName::from_bytes(name.as_bytes()),
            header::HeaderValue::from_str(value),
        ) {
            req = req.header(n, v);
        }
    }
    req
}

/// Opens the URL with an open-ended range request and reads only the headers,
/// which reveals size, range support and the server's file name in one round trip.
pub async fn probe(
    client: &Client,
    url: &str,
    opts: &RequestOptions<'_>,
) -> Result<ProbeInfo, EngineError> {
    let resp = request(client, url, opts)
        .header(header::RANGE, "bytes=0-")
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(EngineError::Http(status.as_u16()));
    }
    let final_url = resp.url().to_string();
    let headers = resp.headers().clone();
    drop(resp);

    let (total_size, resumable) = size_and_ranges(status, &headers);
    let content_type = header_str(&headers, header::CONTENT_TYPE);
    let mut file_name = header_str(&headers, header::CONTENT_DISPOSITION)
        .and_then(|cd| naming::from_content_disposition(&cd))
        .or_else(|| naming::from_url(&final_url))
        .or_else(|| naming::from_url(url))
        .unwrap_or_else(|| "download".to_string());
    if !file_name.contains('.')
        && let Some(ext) = content_type.as_deref().and_then(naming::extension_for_mime)
    {
        file_name = format!("{file_name}.{ext}");
    }
    let file_name = naming::sanitize(&file_name);

    Ok(ProbeInfo {
        url: url.to_string(),
        final_url,
        category: Category::from_file_name(&file_name),
        file_name,
        total_size,
        resumable,
        content_type,
        etag: header_str(&headers, header::ETAG),
        last_modified: header_str(&headers, header::LAST_MODIFIED),
    })
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_string)
}

fn size_and_ranges(status: StatusCode, headers: &HeaderMap) -> (Option<u64>, bool) {
    if status == StatusCode::PARTIAL_CONTENT {
        let total = header_str(headers, header::CONTENT_RANGE)
            .and_then(|cr| parse_content_range_total(&cr));
        return (total, total.is_some());
    }
    let length = header_str(headers, header::CONTENT_LENGTH).and_then(|v| v.trim().parse().ok());
    // A 200 is a valid reply to `bytes=0-` (it is the whole file), so trust Accept-Ranges.
    let accepts =
        header_str(headers, header::ACCEPT_RANGES).is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
    (length, accepts && length.is_some())
}

/// Parses the complete length from `bytes 0-99/1234`; `*` means unknown.
pub fn parse_content_range_total(value: &str) -> Option<u64> {
    value
        .trim()
        .strip_prefix("bytes")?
        .trim()
        .split('/')
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_headers_cannot_override_engine_headers() {
        let input: BTreeMap<String, String> = [
            ("Cookie", " session=abc "),
            ("User-Agent", "Mozilla/5.0"),
            ("Range", "bytes=0-1"),
            ("accept-encoding", "gzip"),
            ("Proxy-Authorization", "x"),
            ("Bad Name", "x"),
            ("X-Ok", "line\nbreak"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let kept: Vec<(String, String)> = sanitize_headers(&input).into_iter().collect();
        assert_eq!(
            kept,
            vec![
                ("Cookie".to_string(), "session=abc".to_string()),
                ("User-Agent".to_string(), "Mozilla/5.0".to_string()),
            ]
        );
    }

    #[test]
    fn custom_user_agent_replaces_the_default() {
        let client = Client::new();
        let headers: BTreeMap<String, String> =
            [("user-agent".to_string(), "Browser/1".to_string())].into();
        let opts = RequestOptions {
            user_agent: "Downpour/1",
            referer: None,
            headers: &headers,
        };
        let req = request(&client, "http://example.invalid/", &opts)
            .build()
            .unwrap();
        let agents: Vec<_> = req.headers().get_all(header::USER_AGENT).iter().collect();
        assert_eq!(agents, ["Browser/1"]);
    }

    #[test]
    fn content_range_total() {
        assert_eq!(parse_content_range_total("bytes 0-99/1234"), Some(1234));
        assert_eq!(parse_content_range_total("bytes 0-99/*"), None);
        assert_eq!(parse_content_range_total("items 0-1/2"), None);
    }
}
