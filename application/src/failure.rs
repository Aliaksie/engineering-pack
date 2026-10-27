//! The failure every use case returns: a message and, for I/O, the kind its reason code
//! and Node-style telemetry code derive from.
use craft_platform::documents::DocumentError;
use std::io::ErrorKind;

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    io: Option<ErrorKind>,
}

impl Failure {
    pub(crate) fn from_io(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
            io: Some(error.kind()),
        }
    }
    pub(crate) fn from_error_code(code: &str) -> Self {
        let io = match code {
            "ENOENT" => Some(ErrorKind::NotFound),
            "EACCES" => Some(ErrorKind::PermissionDenied),
            "ENOTDIR" => Some(ErrorKind::NotADirectory),
            _ => None,
        };
        Self {
            message: code.into(),
            io,
        }
    }
    /// The stable reason code `observe` records for this failure.
    pub fn code(&self, fallback: &'static str) -> &'static str {
        match self.io {
            Some(ErrorKind::PermissionDenied) => "ACCESS_DENIED",
            Some(ErrorKind::StorageFull) => "STORAGE_FULL",
            Some(ErrorKind::NotFound) => "NOT_FOUND",
            Some(ErrorKind::ResourceBusy | ErrorKind::AlreadyExists) => "STATE_BUSY",
            _ => fallback,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, io: None }
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
            io: error.io_kind(),
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_failures_record_stable_reason_codes() {
        let code = |kind| Failure::from_io(std::io::Error::from(kind)).code("FALLBACK");
        assert_eq!(code(ErrorKind::NotFound), "NOT_FOUND");
        assert_eq!(code(ErrorKind::PermissionDenied), "ACCESS_DENIED");
        assert_eq!(code(ErrorKind::AlreadyExists), "STATE_BUSY");
        assert_eq!(code(ErrorKind::InvalidData), "FALLBACK");
        assert_eq!(
            Failure::from_error_code("ENOTDIR").code("FALLBACK"),
            "FALLBACK"
        );
        assert_eq!(
            Failure::from_error_code("EACCES").code("FALLBACK"),
            "ACCESS_DENIED"
        );
        assert_eq!(Failure::from("message").code("FALLBACK"), "FALLBACK");
    }
}
