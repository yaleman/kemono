#[derive(Debug)]
pub enum KemonoError {
    Reqwest(reqwest::Error),
    Generic(String),
    SerdeJson(serde_json::Error),
    RateLimited,
    GetPostsError(String),
}

impl core::fmt::Display for KemonoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KemonoError::Reqwest(e) => write!(f, "Reqwest error: {}", e),
            KemonoError::Generic(e) => write!(f, "Generic error: {}", e),
            KemonoError::SerdeJson(e) => write!(f, "SerdeJson error: {}", e),
            KemonoError::RateLimited => write!(f, "Rate limited"),
            KemonoError::GetPostsError(e) => write!(f, "Error getting posts: {}", e),
        }
    }
}

impl From<reqwest::Error> for KemonoError {
    fn from(e: reqwest::Error) -> Self {
        KemonoError::Reqwest(e)
    }
}

impl KemonoError {
    pub fn from_stringable(e: impl ToString) -> Self {
        KemonoError::Generic(e.to_string())
    }
}

impl From<String> for KemonoError {
    fn from(e: String) -> Self {
        KemonoError::Generic(e)
    }
}

impl From<serde_json::Error> for KemonoError {
    fn from(e: serde_json::Error) -> Self {
        KemonoError::SerdeJson(e)
    }
}
