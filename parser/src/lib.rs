use std::path::PathBuf;

pub mod error;

use crate::error::Result;

pub fn read(path: impl Into<PathBuf>) -> Result<String> {
    let path = path.into();
    let string = std::fs::read_to_string(path)?;
    Ok(string)
}

pub fn parse(input: &str) -> Result<oas::OpenAPIV3> {
    let document: oas::OpenAPIV3 = match serde_json::from_str(input) {
        Ok(document) => document,
        // fallback to yaml if json parsing fails
        Err(_) => serde_yaml::from_str(input)?,
    };
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read() {
        let string = read("test-data/openapi-minimal.json").expect("Failed to read file");
        let document = parse(&string).expect("Failed to parse file");
        assert_eq!(document.info.title, "Minimal API");
        assert_eq!(document.info.version, "1.0.0");
        assert_eq!(document.paths.len(), 0);
    }

    #[test]
    fn test_read_yaml() {
        let string = read("test-data/openapi-minimal.yaml").expect("Failed to read file");
        let document = parse(&string).expect("Failed to parse file");
        assert_eq!(document.info.title, "Minimal API");
        assert_eq!(document.info.version, "1.0.0");
        assert_eq!(document.paths.len(), 0);
    }
}
