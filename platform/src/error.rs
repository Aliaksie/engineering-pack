//! The failure platform effects return: the message callers show and, when the cause is
//! an operating-system or storage condition, the kind callers map to a reason code.
use std::{fmt, io::ErrorKind};

#[derive(Debug)]
pub struct Error {
    message: String,
    kind: Option<ErrorKind>,
}

impl Error {
    /// A writer or lock already holds the resource.
    pub fn busy(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: Some(ErrorKind::ResourceBusy),
        }
    }
    pub fn kind(&self) -> Option<ErrorKind> {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self {
            kind: Some(error.kind()),
            message: error.to_string(),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        let kind = match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
                Some(ErrorKind::ResourceBusy)
            }
            Some(ErrorCode::DiskFull) => Some(ErrorKind::StorageFull),
            Some(ErrorCode::PermissionDenied | ErrorCode::ReadOnly) => {
                Some(ErrorKind::PermissionDenied)
            }
            _ => None,
        };
        Self {
            message: error.to_string(),
            kind,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        error.to_string().into()
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self {
            message,
            kind: None,
        }
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_message_and_the_storage_kind() {
        let io = std::io::Error::from(ErrorKind::NotFound);
        let message = io.to_string();
        let error = Error::from(io);
        assert_eq!(error.to_string(), message);
        assert_eq!(error.kind(), Some(ErrorKind::NotFound));
        assert_eq!(Error::from("invalid").kind(), None);
        assert_eq!(Error::busy("busy").kind(), Some(ErrorKind::ResourceBusy));
        let full = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
            None,
        );
        assert_eq!(Error::from(full).kind(), Some(ErrorKind::StorageFull));
    }
}
