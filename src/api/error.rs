use thiserror::Error;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Error)]
pub enum ApiError {
    /// The request never got a response: DNS, TLS, refused connection or timeout.
    #[error("couldn't reach the server: {0}")]
    Network(#[source] reqwest::Error),

    /// 401/403. The key was revoked or never valid.
    #[error("the server rejected the API key")]
    Unauthorized,

    #[error("server returned {status} for {endpoint}: {message}")]
    Status {
        endpoint: String,
        status: u16,
        message: String,
    },

    #[error("unexpected response from {endpoint}: {source}")]
    Decode {
        endpoint: String,
        #[source]
        source: serde_json::Error,
    },
}

impl ApiError {
    /// True when retrying later could succeed, as opposed to a request the server
    /// will keep refusing.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Status { status, .. } => *status >= 500 || *status == 429,
            Self::Unauthorized | Self::Decode { .. } => false,
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        // Media URLs carry the API key as a query parameter, so never let one reach a
        // log line through the error's Display.
        Self::Network(err.without_url())
    }
}
