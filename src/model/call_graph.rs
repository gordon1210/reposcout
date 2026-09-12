use super::{Deserialize, Serialize, SourceSpan};

/// Availability and extraction-limit state of one file's call and reference facts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallReferenceStatus {
    Available,
    ParseErrors,
    Unsupported,
    WorkTruncated,
    FactTruncated,
    #[default]
    Unavailable,
}

/// Whether a syntax site calls a target or refers to it without a proven call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallReferenceKind {
    #[default]
    Call,
    Reference,
}

/// The syntactic binding form observed at a call or reference site.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallReferenceSyntax {
    #[default]
    LocalDirect,
    ImportedBinding,
    ModuleQualified,
    Receiver,
    Dynamic,
}

/// The static import form used to associate a local binding with a module target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallImportKind {
    #[default]
    Named,
    Default,
    Namespace,
    RustPath,
}

/// The module-resolution outcome supplied to conservative call and reference binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallResolutionStatus {
    Resolved,
    Local,
    External,
    #[default]
    Unresolved,
}

/// The missing evidence, unsupported form or resource limit preventing a proven binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallUnresolvedReason {
    UnsupportedLanguage,
    ParseErrors,
    WorkLimit,
    FactLimit,
    UnsupportedSyntax,
    DynamicReceiver,
    DynamicTarget,
    ShadowedBinding,
    MissingImportResolution,
    #[default]
    MissingTarget,
    AmbiguousTarget,
    MissingOwner,
}

/// A declaration identity tied to its file and exact declaration span.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSymbolIdentity {
    pub path: String,
    pub source_hash: String,
    pub name: String,
    pub kind: String,
    pub declaration_span: SourceSpan,
}

/// A captured declaration and its syntactically established export names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallDeclaration {
    pub symbol: CallSymbolIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_names: Vec<String>,
    pub scope_span: SourceSpan,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub overloaded: bool,
}

/// A static import binding with its local name, target spelling and source span.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallImportBinding {
    pub module_specifier: String,
    pub imported_name: String,
    pub local_name: String,
    pub kind: CallImportKind,
    pub span: SourceSpan,
    pub scope_span: SourceSpan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_modules: Vec<String>,
}

/// The observed target spelling before scope and module evidence establish a binding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallTargetCandidate {
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_modules: Vec<String>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub shadowed: bool,
}

/// A captured call or reference site with optional owning declaration and binding evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallReferenceFact {
    pub kind: CallReferenceKind,
    pub syntax: CallReferenceSyntax,
    pub site: SourceSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<CallSymbolIdentity>,
    pub candidate: CallTargetCandidate,
    pub provenance: String,
}

/// Observed and retained extraction work, including omitted nodes, relations and unsupported forms.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallFactCoverage {
    pub parse_errors: usize,
    pub visited_nodes: usize,
    pub omitted_nodes: usize,
    pub observed_declarations: usize,
    pub retained_declarations: usize,
    pub omitted_declarations: usize,
    pub observed_imports: usize,
    pub retained_imports: usize,
    pub omitted_imports: usize,
    pub observed_relations: usize,
    pub retained_relations: usize,
    pub omitted_relations: usize,
    pub unsupported_syntax: usize,
}

/// Content-identified declarations, imports and call/reference sites with extraction coverage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallReferenceFacts {
    pub status: CallReferenceStatus,
    pub language: String,
    pub source_path: String,
    pub source_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declarations: Vec<CallDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<CallImportBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<CallReferenceFact>,
    #[serde(default)]
    pub coverage: CallFactCoverage,
}

/// A source-language module specifier requiring the existing module resolver.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CallModuleRequest {
    pub source_path: String,
    pub language: String,
    pub module_specifier: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_modules: Vec<String>,
}

/// An existing resolver's module decision and provenance for a requested source specifier.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallModuleResolution {
    pub request: CallModuleRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
    pub status: CallResolutionStatus,
}

/// A proven source-to-target call or reference with its site and binding provenance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCallReference {
    pub source: CallSymbolIdentity,
    pub target: CallSymbolIdentity,
    pub site: SourceSpan,
    pub kind: CallReferenceKind,
    pub syntax: CallReferenceSyntax,
    pub resolver: String,
}

/// An observed call or reference that remains unbound, with the reason preserved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCallReference {
    pub source_path: String,
    pub source_hash: String,
    pub site: SourceSpan,
    pub kind: CallReferenceKind,
    pub syntax: CallReferenceSyntax,
    pub candidate: CallTargetCandidate,
    pub reason: CallUnresolvedReason,
}

/// Examined, resolved, unresolved, unsupported and omitted relation counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallResolutionCoverage {
    pub examined: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub unsupported: usize,
    pub omitted: usize,
}

/// Resolved call/reference edges and explicit unresolved evidence with coverage counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallReferenceTopology {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<ResolvedCallReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<UnresolvedCallReference>,
    #[serde(default)]
    pub coverage: CallResolutionCoverage,
}
