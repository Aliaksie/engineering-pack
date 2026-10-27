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
    /// The Node-style code of an I/O failure, for a caller-owned telemetry envelope.
    pub(crate) fn node_code(&self) -> Option<&'static str> {
        match self.io {
            Some(ErrorKind::PermissionDenied) => Some("EACCES"),
            Some(ErrorKind::StorageFull) => Some("ENOSPC"),
            Some(ErrorKind::NotFound) => Some("ENOENT"),
            Some(ErrorKind::NotADirectory) => Some("ENOTDIR"),
            Some(ErrorKind::ResourceBusy) => Some("EBUSY"),
            Some(ErrorKind::AlreadyExists) => Some("EEXIST"),
            _ => None,
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
