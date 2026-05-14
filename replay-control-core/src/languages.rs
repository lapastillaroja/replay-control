/// Parse a comma-separated language tag list into trimmed tags.
///
/// Empty items are ignored. The original order is preserved so callers can
/// rank against user preferences without losing source ordering.
pub fn parse_languages(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_separated_languages() {
        assert_eq!(parse_languages("en, es,it"), vec!["en", "es", "it"]);
        assert_eq!(parse_languages(""), Vec::<&str>::new());
        assert_eq!(parse_languages("en,, ja "), vec!["en", "ja"]);
    }
}
