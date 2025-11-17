use std::path::PathBuf;

pub mod error;

pub fn read(path: impl Into<PathBuf>) -> Result<String, error::ParserError> {
    let path = path.into();
    let string = std::fs::read_to_string(path)?;
    Ok(string)
}

pub fn parse(input: &str) -> Result<oas3::Spec, error::ParserError> {
    let document: oas3::Spec = match serde_json::from_str(input) {
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
    fn test_parse() {
        let file = std::fs::read_to_string("test-data/unkey.yml").expect("Failed to read file");
        let document = parse(&file).expect("Failed to parse file");
        assert_eq!(document.info.title, "Unkey API");
        assert_eq!(document.info.version, "2.0.0");
        assert_eq!(document.paths.iter().len(), 1);
    }
}
