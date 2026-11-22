use reqwest::header::{self, HeaderMap};
use reqwest::{Client, StatusCode};

use super::category::Category;
use super::error::EngineError;
use super::model::ProbeInfo;
use super::naming;

pub struct RequestOptions<'a> {
    pub user_agent: &'a str,
    pub referer: Option<&'a str>,
}

pub fn request(client: &Client, url: &str, opts: &RequestOptions<'_>) -> reqwest::RequestBuilder {
    let mut req = client
        .get(url)
        .header(header::USER_AGENT, opts.user_agent)
        // Compressed transfer would make byte ranges refer to the encoded body.
        .header(header::ACCEPT_ENCODING, "identity");
    if let Some(r) = opts.referer {
        req = req.header(header::REFERER, r);
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
    fn content_range_total() {
        assert_eq!(parse_content_range_total("bytes 0-99/1234"), Some(1234));
        assert_eq!(parse_content_range_total("bytes 0-99/*"), None);
        assert_eq!(parse_content_range_total("items 0-1/2"), None);
    }
}
