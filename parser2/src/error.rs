#[derive(Debug)]
pub enum ParserError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
}

impl From<std::io::Error> for ParserError {
    fn from(error: std::io::Error) -> Self {
        ParserError::Io(error)
    }
}

impl From<serde_json::Error> for ParserError {
    fn from(error: serde_json::Error) -> Self {
        ParserError::Json(error)
    }
}

impl From<serde_yaml::Error> for ParserError {
    fn from(error: serde_yaml::Error) -> Self {
        ParserError::Yaml(error)
    }
}

impl std::error::Error for ParserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParserError::Io(e) => Some(e),
            ParserError::Json(e) => Some(e),
            ParserError::Yaml(e) => Some(e),
        }
    }
}

impl std::fmt::Display for ParserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParserError::Io(e) => write!(f, "IO error: {}", e),
            ParserError::Json(e) => write!(f, "JSON error: {}", e),
            ParserError::Yaml(e) => write!(f, "YAML error: {}", e),
        }
    }
}

pub type Result<T> = std::result::Result<T, ParserError>;
