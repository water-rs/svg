//! The name an SVG document gives itself.

use alloc::string::String;
use alloc::vec::Vec;

use usvg::roxmltree::{Document, Node};

/// The text of the root element's own `<title>`, which is the document's
/// accessible name (SVG 2 §5.9.1).
///
/// A `<title>` nested deeper names that element rather than the drawing and
/// is left alone. Whitespace is collapsed the way it would be spoken. `None`
/// for markup that does not parse and for a missing or empty title; the
/// scene parser is the one that reports a document it cannot draw.
pub fn document_title(markup: &str) -> Option<String> {
    let document = Document::parse(markup).ok()?;
    let title = document
        .root_element()
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "title")?;
    let words: Vec<&str> = title
        .descendants()
        .filter(Node::is_text)
        .filter_map(|node| node.text())
        .flat_map(str::split_whitespace)
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::document_title;

    #[test]
    fn the_root_title_names_the_document() {
        assert_eq!(
            document_title(include_str!("../tests/data/titled_icon.svg")).as_deref(),
            Some("Warning sign")
        );
    }

    #[test]
    fn a_title_nested_in_an_element_names_that_element_not_the_document() {
        assert_eq!(
            document_title(include_str!("../tests/data/nested_title_icon.svg")),
            None
        );
    }

    #[test]
    fn an_untitled_document_and_path_data_have_no_name() {
        assert_eq!(
            document_title(include_str!("../tests/data/painted_icon.svg")),
            None
        );
        assert_eq!(document_title("M3 12h18M3 6h18M3 18h18"), None);
        assert_eq!(
            document_title(include_str!("../tests/data/blank_title_icon.svg")),
            None
        );
    }
}
