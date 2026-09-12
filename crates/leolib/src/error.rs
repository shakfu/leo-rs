//! One error type for the whole crate.
//!
//! Every fallible function here answers with this. The variants are the
//! distinctions a caller acts on -- a missing file is offered a create, an
//! unread one a prompt, an encoding this port cannot write neither -- rather
//! than the places in the code that raise them. Leo has no equivalent: it
//! reports through `g.error` and returns a flag, so the wording below is this
//! port's, not a port of anything.

/// What went wrong. The `Display` text is meant to be shown as it is.
#[derive(Debug)]
pub enum Error {
    /// The OS refused the file.
    Io {
        path: String,
        source: std::io::Error,
    },
    /// A file the outline names is not on disk.
    NotFound { path: String },
    /// The bytes are not UTF-8. See [`crate::external::read_file_to_string`].
    NotUtf8 { path: String, byte: usize },
    /// An `@encoding`, an `@+leo` header field, or a `leo_file_encoding`,
    /// naming an encoding this port cannot write.
    UnsupportedEncoding { encoding: String },
    /// The file has no `@+leo` sentinel, so there is no tree in it to read.
    NotAnExternalFile { path: String },
    /// The file is not XML, or is XML that is not an outline.
    NotALeoFile { detail: String },
    /// The `.leo` file's XML is malformed.
    BadXml { detail: String },
    /// No `@auto` importer, or one whose tree would not write the file back.
    Import { path: String, detail: String },
    /// The writer could not produce the file: an undefined section
    /// reference, or several `@section-delims`.
    Write { detail: String },
    /// Writing would discard a file this outline never read.
    RefusedOverwrite { path: String },
    /// Something this port does not do, such as `@shadow`.
    Unsupported { detail: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{path}: {source}"),
            Error::NotFound { path } => write!(f, "not found: {path}"),
            Error::NotUtf8 { path, byte } => write!(
                f,
                "{path}: not UTF-8 (byte {byte}); this port reads and writes UTF-8 only"
            ),
            Error::UnsupportedEncoding { encoding } => write!(
                f,
                "encoding {encoding} is not supported; this port reads and writes UTF-8 only"
            ),
            Error::NotAnExternalFile { path } => write!(f, "not a valid external file: {path}"),
            Error::NotALeoFile { detail } => write!(f, "not a readable .leo file: {detail}"),
            Error::BadXml { detail } => write!(f, "bad XML in .leo file: {detail}"),
            Error::Import { path, detail } => write!(f, "{path}: {detail}"),
            Error::Write { detail } => write!(f, "{detail}"),
            Error::RefusedOverwrite { path } => write!(
                f,
                "refusing to overwrite a file this outline has not read: {path}"
            ),
            Error::Unsupported { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Error {
    /// An `std::io::Error` with the path that produced it, which the OS's own
    /// message leaves out.
    pub fn io(path: &str, source: std::io::Error) -> Self {
        match source.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound {
                path: path.to_string(),
            },
            _ => Error::Io {
                path: path.to_string(),
                source,
            },
        }
    }

    /// The error for bytes that are not UTF-8.
    pub fn not_utf8(path: &str, e: &std::str::Utf8Error) -> Self {
        Error::NotUtf8 {
            path: path.to_string(),
            byte: e.valid_up_to(),
        }
    }
}

/// Fallible operations in this crate.
pub type Result<T> = std::result::Result<T, Error>;
