use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("server responded with HTTP {0}")]
    Http(u16),
    #[error("{0}")]
    Network(String),
    #[error("disk error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the server stopped accepting resumable requests")]
    RangeUnsupported,
    #[error("download not found")]
    NotFound,
    #[error("cancelled")]
    Cancelled,
    /// The server turned away an extra connection while others were working.
    #[error("the server refused another connection")]
    Refused,
}

impl EngineError {
    /// Client errors such as 404 or 403 will not fix themselves on retry;
    /// 408 and 429 are the exceptions.
    pub fn is_retryable(&self) -> bool {
        match self {
            EngineError::Http(code) => *code >= 500 || *code == 408 || *code == 429,
            EngineError::Network(_) => true,
            _ => false,
        }
    }
}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(status) = e.status() {
            return EngineError::Http(status.as_u16());
        }
        let mut msg = e.to_string();
        let mut source = std::error::Error::source(&e);
        while let Some(s) = source {
            msg = format!("{msg}: {s}");
            source = s.source();
        }
        EngineError::Network(msg)
    }
}

impl Serialize for EngineError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
