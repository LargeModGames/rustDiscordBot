//! Formatting utilities for AI responses in Discord.
//!
//! This module provides helpers to format AI response metadata (like citations)
//! into Discord-friendly markdown format.

use super::models::Citation;

/// Maximum number of citations to display to avoid message spam.
const MAX_CITATIONS: usize = 5;

/// Formats citations for Discord display.
///
/// Returns a formatted string with citation links, or `None` if there are no citations.
/// Limits output to 5 citations to avoid message spam.
///
/// # Example Output
/// ```text
/// Sources:
/// - [Article Title](https://example.com)
/// - [Another Source](https://example.org)
/// ```
pub fn format_citations_for_discord(citations: &[Citation]) -> Option<String> {
    if citations.is_empty() {
        return None;
    }

    let formatted: Vec<String> = citations
        .iter()
        .take(MAX_CITATIONS)
        .map(|citation| {
            let title = citation.title.as_deref().unwrap_or("Source");
            format!("- [{}]({})", title, citation.url)
        })
        .collect();

    let mut result = String::from("Sources:\n");
    result.push_str(&formatted.join("\n"));

    // Add note if we truncated
    if citations.len() > MAX_CITATIONS {
        result.push_str(&format!(
            "\n_...and {} more sources_",
            citations.len() - MAX_CITATIONS
        ));
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_citations() {
        assert_eq!(format_citations_for_discord(&[]), None);
    }

    #[test]
    fn test_single_citation() {
        let citations = vec![Citation {
            title: Some("Test Article".to_string()),
            url: "https://example.com".to_string(),
        }];
        let result = format_citations_for_discord(&citations).unwrap();
        assert!(result.contains("Sources:"));
        assert!(result.contains("[Test Article](https://example.com)"));
    }

    #[test]
    fn test_citation_without_title() {
        let citations = vec![Citation {
            title: None,
            url: "https://example.com".to_string(),
        }];
        let result = format_citations_for_discord(&citations).unwrap();
        assert!(result.contains("[Source](https://example.com)"));
    }

    #[test]
    fn test_max_citations_limit() {
        let citations: Vec<Citation> = (0..10)
            .map(|i| Citation {
                title: Some(format!("Article {}", i)),
                url: format!("https://example{}.com", i),
            })
            .collect();
        let result = format_citations_for_discord(&citations).unwrap();
        // Should only contain first 5
        assert!(result.contains("Article 0"));
        assert!(result.contains("Article 4"));
        assert!(!result.contains("Article 5"));
        assert!(result.contains("...and 5 more sources"));
    }
}
