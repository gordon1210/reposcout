use crate::config::Config;
use crate::metrics::{lexical, tokens::TokenCounter};
use crate::model::{
    FindFieldTruncation, FindMatchEvidence, FindMatchMode, FindQueryCapability, FindQueryHit,
    FindQueryReport, FindReadSelector, FindReadTarget, FindSearchCoverage, LexicalDefinitionFacts,
    LexicalField, LexicalFileFacts, LexicalStatus, SCHEMA_VERSION, SourceRevision,
};
use crate::report::Format;
use crate::scan;
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Default maximum number of lexical hits returned before output-budget admission.
pub(super) const DEFAULT_LIMIT: usize = 20;
/// Hard maximum number of lexical hits returned before output-budget admission.
pub(super) const MAX_LIMIT: usize = 100;
/// Maximum Unicode scalar values accepted in a lexical query.
pub(super) const MAX_QUERY_CHARS: usize = 512;
/// Maximum distinct normalized terms accepted in a lexical query.
pub(super) const MAX_QUERY_TERMS: usize = 16;
/// Default token limit for the complete rendered lexical-search response.
pub(super) const DEFAULT_TOKENS: usize = 4_096;
/// Smallest accepted token limit for a lexical-search response.
pub(super) const MIN_TOKENS: usize = 256;
/// Largest accepted token limit for a lexical-search response.
pub(super) const MAX_TOKENS: usize = 65_536;
/// Default byte limit for the complete rendered lexical-search response.
pub(super) const DEFAULT_BYTES: usize = 65_536;
/// Smallest accepted byte limit for a lexical-search response.
pub(super) const MIN_BYTES: usize = 1_024;
/// Largest accepted byte limit for a lexical-search response.
pub(super) const MAX_BYTES: usize = 1_048_576;

/// Lexical query, filters and limits for deterministic body-free candidate selection.
#[derive(Debug, Clone)]
pub struct FindQueryOptions {
    pub query: String,
    pub match_mode: FindMatchMode,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub limit: usize,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub format: Format,
    pub pretty_json: bool,
}

/// A lexical-search report together with its complete budget-checked rendered output.
pub struct FindQueryOutput {
    pub report: FindQueryReport,
    pub rendered: String,
}

/// Find lexical declaration candidates through shared scanner facts and render body-free results within the requested output limits.
///
/// # Errors
///
/// Returns an error for invalid query options or targets, unrecoverable discovery or analysis failures,
/// token-counter initialization or serialization failures, or a budget that cannot hold the
/// minimal status envelope.
pub fn find(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &FindQueryOptions,
) -> Result<FindQueryOutput> {
    let query = options.query.trim();
    validate_options(query, options)?;
    let query_terms = validated_query_terms(query)?;
    let query_config = super::declaration_query_config(cfg);
    let artifacts = scan::run_with_artifacts(
        target,
        &query_config,
        exclusions,
        scan::ArtifactRequirements {
            lexical_facts: true,
            ..scan::ArtifactRequirements::default()
        },
    )?;
    let kind_filter = normalize_filter(options.kind.as_deref());
    let language_filter = normalize_filter(options.language.as_deref());
    let matches = collect_matches(
        &artifacts.lexical_facts,
        query,
        &query_terms,
        options,
        kind_filter.as_deref(),
        language_filter.as_deref(),
    );
    let counter = TokenCounter::new(&query_config.encoding)?;
    let root = artifacts.report.root;
    let report_root = root.to_str().map(|_| root.clone());
    let root_omitted = report_root.is_none();
    let mut report = FindQueryReport {
        kind: "find_query".to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
        root: report_root,
        root_omitted,
        encoding: counter.name().to_string(),
        query: query.to_string(),
        query_terms,
        match_mode: options.match_mode,
        kind_filter,
        language_filter,
        limit: options.limit,
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        coverage: matches.coverage,
        total_matches: matches.total_matches,
        returned_matches: 0,
        limit_omitted: matches.total_matches.saturating_sub(matches.hits.len()),
        budget_omitted: matches.hits.len(),
        hits: Vec::new(),
    };
    admit_matches(&mut report, matches.hits, options, &counter)?;
    let rendered = crate::report::find::render(&report, options.format, options.pretty_json)?;
    ensure!(
        rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget,
        "find output exceeded the validated budget"
    );
    Ok(FindQueryOutput { report, rendered })
}

fn validated_query_terms(query: &str) -> Result<Vec<String>> {
    let (query_terms, query_truncated) = lexical::query_terms(query);
    ensure!(
        !query_truncated,
        "find query contains a term longer than {} Unicode scalars",
        lexical::MAX_TERM_CHARS
    );
    ensure!(
        !query_terms.is_empty(),
        "find query must contain at least one alphanumeric term"
    );
    ensure!(
        query_terms.len() <= MAX_QUERY_TERMS,
        "find query contains more than {MAX_QUERY_TERMS} normalized terms"
    );
    Ok(query_terms)
}

struct FindMatches {
    coverage: FindSearchCoverage,
    total_matches: usize,
    hits: Vec<FindQueryHit>,
}

fn collect_matches(
    facts: &BTreeMap<PathBuf, LexicalFileFacts>,
    query: &str,
    query_terms: &[String],
    options: &FindQueryOptions,
    kind_filter: Option<&str>,
    language_filter: Option<&str>,
) -> FindMatches {
    let mut hits = facts
        .iter()
        .filter(|(_, file)| {
            language_filter.is_none_or(|filter| file.language.eq_ignore_ascii_case(filter))
        })
        .flat_map(|(path, file)| {
            file.definitions.iter().filter_map(move |definition| {
                if kind_filter.is_some_and(|filter| {
                    !definition
                        .definition
                        .symbol
                        .kind
                        .eq_ignore_ascii_case(filter)
                }) {
                    return None;
                }
                match_definition(
                    path,
                    file,
                    definition,
                    query,
                    query_terms,
                    options.match_mode,
                )
            })
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| {
                left.declaration_span
                    .start_byte
                    .cmp(&right.declaration_span.start_byte)
            })
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });
    for (index, hit) in hits.iter_mut().enumerate() {
        hit.rank = index + 1;
    }
    let total_matches = hits.len();
    hits.truncate(options.limit);
    FindMatches {
        coverage: coverage(facts),
        total_matches,
        hits,
    }
}

fn admit_matches(
    report: &mut FindQueryReport,
    hits: Vec<FindQueryHit>,
    options: &FindQueryOptions,
    counter: &TokenCounter,
) -> Result<()> {
    if !fits(report, options, counter)? {
        report.root = None;
        report.root_omitted = true;
    }
    ensure!(
        fits(report, options, counter)?,
        "find budget cannot fit the status envelope"
    );
    let limited_matches = hits.len();
    for hit in hits {
        let mut candidate = report.clone();
        candidate.hits.push(hit);
        candidate.returned_matches = candidate.hits.len();
        candidate.budget_omitted = limited_matches.saturating_sub(candidate.hits.len());
        if !fits(&candidate, options, counter)? {
            if candidate.root.take().is_some() {
                candidate.root_omitted = true;
            }
            if !fits(&candidate, options, counter)? {
                break;
            }
        }
        *report = candidate;
    }
    Ok(())
}

fn validate_options(query: &str, options: &FindQueryOptions) -> Result<()> {
    ensure!(!query.is_empty(), "find query cannot be empty");
    ensure!(
        query.chars().count() <= MAX_QUERY_CHARS,
        "find query must contain at most {MAX_QUERY_CHARS} Unicode scalars"
    );
    ensure!(
        (1..=MAX_LIMIT).contains(&options.limit),
        "find limit must be between 1 and {MAX_LIMIT}"
    );
    ensure!(
        (MIN_TOKENS..=MAX_TOKENS).contains(&options.token_budget),
        "find token budget must be between {MIN_TOKENS} and {MAX_TOKENS}"
    );
    ensure!(
        (MIN_BYTES..=MAX_BYTES).contains(&options.byte_budget),
        "find byte budget must be between {MIN_BYTES} and {MAX_BYTES}"
    );
    ensure!(
        matches!(
            options.format,
            Format::Table | Format::Json | Format::Markdown | Format::Ndjson
        ),
        "find supports table, JSON, Markdown, or NDJSON output"
    );
    ensure!(
        !options.pretty_json || options.format == Format::Json,
        "pretty output requires JSON"
    );
    for (name, value) in [
        ("kind", options.kind.as_deref()),
        ("language", options.language.as_deref()),
    ] {
        if let Some(value) = value {
            ensure!(
                !value.trim().is_empty() && value.chars().count() <= 128,
                "find {name} filter must contain between 1 and 128 Unicode scalars"
            );
        }
    }
    Ok(())
}

fn normalize_filter(value: Option<&str>) -> Option<String> {
    value.map(str::trim).map(str::to_string)
}

fn coverage(facts: &BTreeMap<PathBuf, LexicalFileFacts>) -> FindSearchCoverage {
    let mut coverage = FindSearchCoverage {
        files_total: facts.len(),
        ..FindSearchCoverage::default()
    };
    let mut truncated_fields = BTreeMap::<LexicalField, usize>::new();
    for file in facts.values() {
        match file.status {
            LexicalStatus::Inspected => coverage.files_inspected += 1,
            LexicalStatus::ParseErrors => {
                coverage.files_inspected += 1;
                coverage.parse_error_files += 1;
            }
            LexicalStatus::Unsupported => coverage.unsupported_files += 1,
            LexicalStatus::Unavailable => coverage.unavailable_files += 1,
        }
        coverage.definitions_total += file.definitions_total;
        coverage.definitions_inspected += file.definitions.len();
        coverage.definitions_omitted += file.definitions_omitted;
        let fields = file
            .definitions
            .iter()
            .flat_map(|definition| &definition.fields)
            .filter(|field| field.truncated)
            .map(|field| field.field)
            .collect::<BTreeSet<_>>();
        if !fields.is_empty() {
            coverage.field_truncated_files += 1;
        }
        for field in fields {
            *truncated_fields.entry(field).or_default() += 1;
        }
    }
    coverage.truncated_fields = truncated_fields
        .into_iter()
        .map(|(field, files)| FindFieldTruncation { field, files })
        .collect();
    coverage
}

fn match_definition(
    path: &Path,
    file: &LexicalFileFacts,
    facts: &LexicalDefinitionFacts,
    query: &str,
    query_terms: &[String],
    mode: FindMatchMode,
) -> Option<FindQueryHit> {
    let name = &facts.definition.symbol.name;
    let exact = ExactMatches::new(path, name, query);
    let (matched_terms, matched_fields, field_score) = matched_evidence(facts, query_terms, exact);
    let accepted = match mode {
        FindMatchMode::All => query_terms.iter().all(|term| matched_terms.contains(term)),
        FindMatchMode::Any => !matched_terms.is_empty(),
    };
    if !accepted {
        return None;
    }
    let reason = match_reason(&matched_fields);
    let definition = &facts.definition;
    let snapshot = SourceRevision::Worktree;
    Some(FindQueryHit {
        rank: 0,
        path: path.to_path_buf(),
        name: name.clone(),
        kind: definition.symbol.kind.clone(),
        language: file.language.clone(),
        declaration_span: definition.declaration_span,
        source_span: definition.source_span,
        signature: (!definition.symbol.signature.is_empty())
            .then(|| definition.symbol.signature.clone()),
        sha256: file.sha256.clone(),
        snapshot: snapshot.clone(),
        score: exact.base_score().saturating_add(field_score),
        matched_fields,
        reason,
        read: FindReadTarget {
            path: path.to_path_buf(),
            selector: FindReadSelector::Symbol(name.clone()),
            expected_hash: file.sha256.clone(),
            snapshot,
        },
    })
}

#[derive(Clone, Copy)]
struct ExactMatches {
    name: ExactName,
    path: ExactPath,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExactName {
    None,
    Simple,
    Qualified,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExactPath {
    None,
    File,
    Full,
}

impl ExactMatches {
    fn new(path: &Path, name: &str, query: &str) -> Self {
        let simple_name = name
            .rsplit(['.', ':', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or(name);
        let query = query.to_lowercase();
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let name = if name.to_lowercase() == query {
            ExactName::Qualified
        } else if simple_name.to_lowercase() == query {
            ExactName::Simple
        } else {
            ExactName::None
        };
        let path = if path.to_string_lossy().to_lowercase() == query {
            ExactPath::Full
        } else if filename == query || stem == query {
            ExactPath::File
        } else {
            ExactPath::None
        };
        Self { name, path }
    }

    fn base_score(self) -> u32 {
        let name_score = match self.name {
            ExactName::Qualified => 1_000,
            ExactName::Simple => 900,
            ExactName::None => 0,
        };
        let path_score = match self.path {
            ExactPath::Full => 700,
            ExactPath::File => 600,
            ExactPath::None => 0,
        };
        name_score + path_score
    }

    fn field(self, field: LexicalField) -> bool {
        match field {
            LexicalField::Name => self.name != ExactName::None,
            LexicalField::Path => self.path != ExactPath::None,
            LexicalField::Signature | LexicalField::Comment | LexicalField::Code => false,
        }
    }
}

fn matched_evidence(
    facts: &LexicalDefinitionFacts,
    query_terms: &[String],
    exact: ExactMatches,
) -> (BTreeSet<String>, Vec<FindMatchEvidence>, u32) {
    let mut matched_terms = BTreeSet::new();
    let mut evidence = Vec::new();
    let mut score = 0u32;
    for field in &facts.fields {
        let terms = query_terms
            .iter()
            .filter(|term| field.terms.binary_search(term).is_ok())
            .cloned()
            .collect::<Vec<_>>();
        matched_terms.extend(terms.iter().cloned());
        let is_exact = exact.field(field.field);
        if terms.is_empty() && !is_exact {
            continue;
        }
        let weight = match field.field {
            LexicalField::Name => 100,
            LexicalField::Path => 60,
            LexicalField::Signature => 40,
            LexicalField::Comment => 20,
            LexicalField::Code => 10,
        };
        let field_score =
            u32::try_from(terms.len()).map_or(u32::MAX, |count| count.saturating_mul(weight));
        score = score.saturating_add(field_score);
        evidence.push(FindMatchEvidence {
            field: field.field,
            terms,
            exact: is_exact,
        });
    }
    (matched_terms, evidence, score)
}

fn match_reason(evidence: &[FindMatchEvidence]) -> String {
    evidence
        .iter()
        .map(|evidence| {
            let field = match evidence.field {
                LexicalField::Name => "name",
                LexicalField::Path => "path",
                LexicalField::Signature => "signature",
                LexicalField::Comment => "comment",
                LexicalField::Code => "code",
            };
            let exact = if evidence.exact { "exact " } else { "" };
            format!("{exact}{field}:{}", evidence.terms.join(","))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn fits(
    report: &FindQueryReport,
    options: &FindQueryOptions,
    counter: &TokenCounter,
) -> Result<bool> {
    let rendered = crate::report::find::render(report, options.format, options.pretty_json)?;
    Ok(rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget)
}

pub(super) fn capability() -> FindQueryCapability {
    FindQueryCapability {
        command: "find".to_string(),
        formats: ["table", "json", "markdown", "ndjson"]
            .map(str::to_string)
            .to_vec(),
        match_modes: ["all", "any"].map(str::to_string).to_vec(),
        default_match_mode: "all".to_string(),
        fields: ["name", "path", "signature", "comment", "code"]
            .map(str::to_string)
            .to_vec(),
        default_limit: DEFAULT_LIMIT,
        max_limit: MAX_LIMIT,
        max_query_chars: MAX_QUERY_CHARS,
        max_query_terms: MAX_QUERY_TERMS,
        default_tokens: DEFAULT_TOKENS,
        min_tokens: MIN_TOKENS,
        max_tokens: MAX_TOKENS,
        default_bytes: DEFAULT_BYTES,
        min_bytes: MIN_BYTES,
        max_bytes: MAX_BYTES,
        max_definitions_per_file: lexical::MAX_DEFINITIONS_PER_FILE,
        max_terms_per_field: lexical::MAX_TERMS_PER_FIELD,
        max_term_chars: lexical::MAX_TERM_CHARS,
        max_code_bytes_per_definition: lexical::MAX_CODE_BYTES_PER_DEFINITION,
        max_comment_bytes_per_definition: lexical::MAX_COMMENT_BYTES_PER_DEFINITION,
        max_comment_nodes_per_file: lexical::MAX_COMMENT_NODES_PER_FILE,
        max_syntax_nodes_per_file: lexical::MAX_SYNTAX_NODES_PER_FILE,
    }
}
