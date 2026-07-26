//! Tree-sitter parsing facade. Provides the `Language` for each first-class
//! grammar and a convenience `parse` helper. Frozen shared contract used by
//! the complexity and imports analyzers.

use crate::lang::FirstClass;
use tree_sitter::{Language, Parser, Tree};

/// Return the tree-sitter `Language` for a first-class language.
pub fn language(fc: FirstClass) -> Language {
    match fc {
        FirstClass::Rust => tree_sitter_rust::LANGUAGE.into(),
        FirstClass::Python => tree_sitter_python::LANGUAGE.into(),
        FirstClass::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        FirstClass::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        FirstClass::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        FirstClass::Go => tree_sitter_go::LANGUAGE.into(),
        FirstClass::Php => tree_sitter_php::LANGUAGE_PHP.into(),
    }
}

/// Parse `source` into a tree-sitter `Tree`, or `None` if the language could
/// not be configured or parsing failed.
pub fn parse(fc: FirstClass, source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&language(fc)).ok()?;
    parser.parse(source, None)
}
