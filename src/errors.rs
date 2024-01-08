#[derive(Debug)]
pub enum KemonoError {
    Reqwest(reqwest::Error),
    Generic(String),
}

impl From<reqwest::Error> for KemonoError {
    fn from(e: reqwest::Error) -> Self {
        KemonoError::Reqwest(e)
    }
}

impl From<String> for KemonoError {
    fn from(e: String) -> Self {
        KemonoError::Generic(e)
    }
}
