//! The failure every use case returns: a message and, for platform effects, the kind its
//! reason code derives from. `code` is the one place a failure becomes a reason code.
use craft_platform::documents::DocumentError;
use std::io::ErrorKind;

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    kind: Option<ErrorKind>,
}

impl Failure {
    /// The stable reason code `observe` records for this failure.
    pub fn code(&self, fallback: &'static str) -> &'static str {
        match self.kind {
            Some(ErrorKind::PermissionDenied) => "ACCESS_DENIED",
            Some(ErrorKind::StorageFull) => "STORAGE_FULL",
            Some(ErrorKind::NotFound) => "NOT_FOUND",
            Some(ErrorKind::ResourceBusy | ErrorKind::AlreadyExists) => "STATE_BUSY",
            _ => fallback,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<craft_platform::Error> for Failure {
    fn from(error: craft_platform::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        craft_platform::Error::from(error).into()
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            kind: None,
        }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

impl From<DocumentError> for Failure {
    fn from(error: DocumentError) -> Self {
        Self {
            kind: error.io_kind(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_failures_record_stable_reason_codes() {
        let code = |kind| Failure::from(std::io::Error::from(kind)).code("FALLBACK");
        assert_eq!(code(ErrorKind::NotFound), "NOT_FOUND");
        assert_eq!(code(ErrorKind::PermissionDenied), "ACCESS_DENIED");
        assert_eq!(code(ErrorKind::AlreadyExists), "STATE_BUSY");
        assert_eq!(code(ErrorKind::InvalidData), "FALLBACK");
        assert_eq!(
            Failure::from(craft_platform::Error::busy("busy")).code("FALLBACK"),
            "STATE_BUSY"
        );
        assert_eq!(Failure::from("message").code("FALLBACK"), "FALLBACK");
    }
}
