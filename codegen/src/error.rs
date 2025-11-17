//! Error types for code generation.

use std::fmt;

/// Result type for generation operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during code generation.
#[derive(Debug)]
pub enum Error {
    /// Error registering or rendering a template.
    TemplateError(Box<dyn std::error::Error + Send + Sync>),

    /// Generator not found for the specified language.
    GeneratorNotFound(String),

    /// Plugin loading failed.
    PluginLoadError(String),

    /// Invalid UTF-8 in generated content.
    Utf8Error(std::str::Utf8Error),

    /// I/O error.
    IoError(std::io::Error),

    /// Validation error.
    ValidationError(String),

    /// Custom error for generator-specific issues.
    Custom(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TemplateError(e) => write!(f, "Template error: {}", e),
            Error::GeneratorNotFound(lang) => {
                write!(f, "Generator not found for language: {}", lang)
            }
            Error::PluginLoadError(e) => write!(f, "Plugin load error: {}", e),
            Error::Utf8Error(e) => write!(f, "UTF-8 error: {}", e),
            Error::IoError(e) => write!(f, "I/O error: {}", e),
            Error::ValidationError(e) => write!(f, "Validation error: {}", e),
            Error::Custom(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::TemplateError(e) => Some(e.as_ref()),
            Error::Utf8Error(e) => Some(e),
            Error::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Self {
        Error::Utf8Error(e)
    }
}
