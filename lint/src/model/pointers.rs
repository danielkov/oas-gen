/// Escape a segment for use in a JSON Pointer (RFC 6901)
/// - `~` → `~0`
/// - `/` → `~1`
pub fn escape_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_segment() {
        assert_eq!(escape_pointer_segment("pets"), "pets");
        assert_eq!(escape_pointer_segment("/pets"), "~1pets");
        assert_eq!(escape_pointer_segment("a~b"), "a~0b");
        assert_eq!(escape_pointer_segment("a/b~c"), "a~1b~0c");
    }
}
