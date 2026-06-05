use std::fmt;

// ─── Reserved system error codes (users must not use) ──────────

/// OS signal received (e.g. Ctrl+C).
pub const CODE_SIGNAL: i64 = -90000;
/// Message too short to parse.
pub const CODE_MSG_TOO_SHORT: i64 = -90001;
/// Payload length mismatch between header and body.
pub const CODE_PAYLOAD_MISMATCH: i64 = -90002;
/// Serialization or deserialization error.
pub const CODE_SERIALIZE: i64 = -90003;
/// Required state type not found in [`StateMap`](crate::StateMap).
pub const CODE_STATE_NOT_FOUND: i64 = -90004;
/// Handler execution error.
pub const CODE_HANDLER: i64 = -90005;
/// Invalid parameter (validation failed).
pub const CODE_INVALID_PARAM: i64 = -90006;
/// I/O error.
pub const CODE_IO: i64 = -90007;
/// WebSocket transport error.
pub const CODE_WS: i64 = -90008;
/// HTTP transport error.
pub const CODE_HTTP: i64 = -90009;
/// TCP transport error.
pub const CODE_TCP: i64 = -90010;
/// Long-connection handler used in HTTP mode (not supported).
pub const CODE_LONG_CONNECTION_NOT_SUPPORTED: i64 = -90011;
/// Request rejected by rate limiter.
pub const CODE_RATE_LIMITED: i64 = -90012;

const CODE_MIN: i64 = -90012;
const CODE_MAX: i64 = -90000;

// Check whether a code falls in the reserved range to prevent
// user-defined errors from colliding with system error codes.
fn is_reserved_code(code: i64) -> bool {
    (CODE_MIN..=CODE_MAX).contains(&code)
}

/// Error type for the afast framework.
///
/// All handler return types use `afast::Result<T>`, which is an alias for
/// `Result<T, Error>`. Each variant carries a numeric code and a
/// human-readable message. User-defined errors must use [`Error::custom`]
/// with codes outside the reserved range (`-90011` to `-90000`).
#[derive(Debug)]
pub enum Error {
    /// Serialization or deserialization failure.
    Serialize { message: String },
    /// Required state type not found in the [`StateMap`](crate::StateMap).
    StateNotFound { message: String },
    /// Handler execution error.
    Handler { message: String },
    /// Invalid parameter, typically from failed deserialization or validation.
    InvalidParam { code: i64, message: String },
    /// I/O error (file not found, permission denied, etc.).
    Io { message: String },
    /// WebSocket transport error.
    Ws { message: String },
    /// HTTP transport error.
    Http { message: String },
    /// TCP transport error.
    Tcp { message: String },
    /// Long-connection handler used in HTTP mode, which does not support
    /// persistent connections.
    LongConnectionNotSupported,
    /// Request rejected by rate limiter.
    RateLimited { message: String },
    /// OS signal received during shutdown.
    Signal { message: String },
    /// User-defined custom error with an arbitrary code and message.
    Custom { code: i64, message: String },
}

impl Error {
    /// Returns the numeric error code for this variant.
    pub fn code(&self) -> i64 {
        match self {
            Error::Signal { .. } => CODE_SIGNAL,
            Error::Serialize { .. } => CODE_SERIALIZE,
            Error::StateNotFound { .. } => CODE_STATE_NOT_FOUND,
            Error::Handler { .. } => CODE_HANDLER,
            Error::InvalidParam { .. } => CODE_INVALID_PARAM,
            Error::Io { .. } => CODE_IO,
            Error::Ws { .. } => CODE_WS,
            Error::Http { .. } => CODE_HTTP,
            Error::Tcp { .. } => CODE_TCP,
            Error::LongConnectionNotSupported => CODE_LONG_CONNECTION_NOT_SUPPORTED,
            Error::RateLimited { .. } => CODE_RATE_LIMITED,
            Error::Custom { code, .. } => *code,
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        match self {
            Error::Signal { message, .. }
            | Error::Serialize { message, .. }
            | Error::StateNotFound { message, .. }
            | Error::Handler { message, .. }
            | Error::InvalidParam { message, .. }
            | Error::Io { message, .. }
            | Error::Ws { message, .. }
            | Error::Http { message, .. }
            | Error::Tcp { message, .. }
            | Error::Custom { message, .. } => message,
            Error::LongConnectionNotSupported => "long connection not supported in HTTP mode",
            Error::RateLimited { message, .. } => message,
        }
    }

    /// Returns a sanitized error message safe for client responses.
    ///
    /// System-level errors (Io, Serialize, StateNotFound, Ws, Http, Tcp)
    /// are replaced with generic messages. User-defined errors (Custom,
    /// Handler, InvalidParam, RateLimited) are returned as-is.
    pub fn sanitized_message(&self) -> &str {
        match self {
            Error::Io { .. } => "internal server error",
            Error::Serialize { .. } => "request processing error",
            Error::StateNotFound { .. } => "internal configuration error",
            Error::Ws { .. } => "transport error",
            Error::Http { .. } => "transport error",
            Error::Tcp { .. } => "transport error",
            Error::Signal { .. } => "server shutting down",
            Error::LongConnectionNotSupported => "long connection not supported in HTTP mode",
            // User-defined errors: return the original message.
            Error::Handler { message, .. }
            | Error::InvalidParam { message, .. }
            | Error::RateLimited { message, .. }
            | Error::Custom { message, .. } => message,
        }
    }

    /// Creates a user-defined custom error.
    ///
    /// In debug builds, panics if `code` is in the reserved range
    /// `-90012..=-90000`. In release builds the check is skipped to
    /// prevent server crashes in production.
    ///
    /// # Example
    ///
    /// ```
    /// use afast::Error;
    /// let err = Error::custom(400, "bad request");
    /// assert_eq!(err.code(), 400);
    /// ```
    pub fn custom(code: i64, message: impl Into<String>) -> Self {
        debug_assert!(
            !is_reserved_code(code),
            "error code {} is reserved by the system",
            code
        );
        Error::Custom {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code(), self.message())
    }
}

impl std::error::Error for Error {}

// ─── AFastError trait ──────────────────────────────────────────

/// Trait for types that can be returned as handler errors.
///
/// Implement this trait on your custom error types to return them
/// directly from handler functions. The framework will call `code()`
/// and `message()` to serialize the error for the client.
///
/// [`Error`] implements this trait, so it continues to work as before.
///
/// # Example
///
/// ```ignore
/// use afast::AFastError;
///
/// enum MyError {
///     NotFound(String),
///     Forbidden,
/// }
///
/// impl AFastError for MyError {
///     fn code(&self) -> i64 {
///         match self {
///             MyError::NotFound(_) => 404,
///             MyError::Forbidden => 403,
///         }
///     }
///     fn message(&self) -> &str {
///         match self {
///             MyError::NotFound(name) => name,  // requires lifetime workaround
///             MyError::Forbidden => "forbidden",
///         }
///     }
/// }
///
/// #[handler(desc("get user"))]
/// async fn get_user(id: Data<Id>) -> afast::Result<User> {
///     find_user(id).ok_or(MyError::NotFound("user".into()))
/// }
/// ```
pub trait AFastError: Send + Sync + 'static {
    /// Returns the numeric error code sent to the client.
    fn code(&self) -> i64;

    /// Returns the human-readable error message sent to the client.
    fn message(&self) -> String;

    /// Converts this error into an [`Error`].
    ///
    /// The default implementation creates an `Error::Custom` with the
    /// trait's `code()` and `message()`. Override for more specific
    /// error variants.
    fn into_error(self) -> Error
    where
        Self: Sized,
    {
        Error::Custom {
            code: self.code(),
            message: self.message(),
        }
    }
}

impl AFastError for Error {
    fn code(&self) -> i64 {
        Error::code(self)
    }

    fn message(&self) -> String {
        Error::message(self).to_string()
    }

    fn into_error(self) -> Error {
        self
    }
}

impl From<afastdata::Error> for Error {
    fn from(e: afastdata::Error) -> Self {
        match e.kind() {
            afastdata::ErrorKind::ValidateError(code, message) => Error::InvalidParam {
                code: *code,
                message: message.clone(),
            },
            _ => Error::Serialize {
                message: e.to_string(),
            },
        }
    }
}
