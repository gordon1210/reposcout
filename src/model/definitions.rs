use super::{Deserialize, Serialize, SymbolOutline};

/// A source range with half-open byte offsets and inclusive, one-based line bounds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
}

/// A declaration identity and its own span, with an optional wrapper-expanded retrieval span.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefinitionFact {
    pub symbol: SymbolOutline,
    pub declaration_span: SourceSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceSpan>,
}

/// Whether definition extraction is available for the supported declaration matrix, with parse failures distinguished from unsupported input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DefinitionStatus {
    Available,
    ParseErrors,
    Unsupported,
    #[default]
    Unavailable,
}

/// Definition facts and extraction status derived from one immutable source input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefinitionFacts {
    #[serde(default)]
    pub status: DefinitionStatus,
    #[serde(default)]
    pub definitions: Vec<DefinitionFact>,
}
