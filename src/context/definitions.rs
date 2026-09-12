use crate::model::{
    DefinitionFacts, DefinitionPlanFile, DefinitionPlanOmission, DefinitionPlanReport,
    DefinitionPlanningFact, DefinitionPlanningFacts, DefinitionStatus, PlannedDefinition,
    SCHEMA_VERSION, SourceRevision, SourceSpan,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Captured declaration and environment facts for one content-identified file side.
#[derive(Debug, Clone)]
pub struct DefinitionPlanInput {
    pub path: PathBuf,
    pub snapshot: SourceRevision,
    pub definitions: DefinitionFacts,
    pub planning: DefinitionPlanningFacts,
}

/// An explicit definition selector bound to a file and requested source revision.
#[derive(Debug, Clone)]
pub struct DefinitionSeed {
    pub path: PathBuf,
    pub snapshot: SourceRevision,
    pub selector: DefinitionSelector,
}

/// A known symbol, one-based line or file-level seed for pure definition selection.
#[derive(Debug, Clone)]
pub enum DefinitionSelector {
    Symbol(String),
    Line(usize),
    File,
}

/// Source-token, output-byte and selection-count bounds; rendered byte admission belongs to the query layer.
#[derive(Debug, Clone)]
pub struct DefinitionPlanLimits {
    pub token_budget: usize,
    pub byte_budget: usize,
    pub max_files: usize,
    pub max_definitions: usize,
}

impl Default for DefinitionPlanLimits {
    fn default() -> Self {
        Self {
            token_budget: 4_096,
            byte_budget: 65_536,
            max_files: 32,
            max_definitions: 16,
        }
    }
}

type Candidate = (usize, usize);

struct Planner<'a> {
    inputs: &'a [DefinitionPlanInput],
    report: DefinitionPlanReport,
    candidates: BTreeSet<Candidate>,
    selected: BTreeMap<Candidate, usize>,
    failures: BTreeMap<Candidate, (String, bool)>,
}

/// Select definitions and supported environment from supplied facts without I/O, counting overlapping captured source ranges once and clamping file and definition limits to 32.
#[must_use]
pub fn plan(
    inputs: &[DefinitionPlanInput],
    seeds: &[DefinitionSeed],
    limits: &DefinitionPlanLimits,
) -> DefinitionPlanReport {
    let mut planner = Planner::new(inputs, limits);
    if seeds.is_empty() {
        let mut files = (0..inputs.len()).collect::<Vec<_>>();
        files.sort_by_key(|&index| (&inputs[index].path, &inputs[index].snapshot));
        for file in files {
            if let Some(reason) = unavailable(&inputs[file]) {
                planner.omit(file, None, reason, false);
            }
            for definition in 0..inputs[file].definitions.definitions.len() {
                planner.admit((file, definition), "expansion", "generic-expansion", false);
            }
        }
    } else {
        for seed in seeds {
            planner.seed(seed);
        }
        let direct = planner
            .report
            .selected
            .iter()
            .map(|item| (item.file - 1, item.definition))
            .collect::<Vec<_>>();
        for candidate in direct {
            planner.environment(candidate);
        }
    }
    planner.finish()
}

impl<'a> Planner<'a> {
    fn new(inputs: &'a [DefinitionPlanInput], limits: &DefinitionPlanLimits) -> Self {
        let encoding = inputs
            .iter()
            .find(|input| !input.planning.encoding.is_empty())
            .map_or_else(String::new, |input| input.planning.encoding.clone());
        Self {
            inputs,
            candidates: BTreeSet::new(),
            selected: BTreeMap::new(),
            failures: BTreeMap::new(),
            report: DefinitionPlanReport {
                kind: "definition_plan".into(),
                schema_version: SCHEMA_VERSION.into(),
                strategy_version: 1,
                encoding,
                token_budget: limits.token_budget,
                context_budget: limits.token_budget,
                byte_budget: limits.byte_budget,
                max_files: limits.max_files.min(32),
                max_definitions: limits.max_definitions.min(32),
                candidate_definitions: 0,
                input_definitions: inputs
                    .iter()
                    .map(|input| input.definitions.definitions.len())
                    .sum(),
                unresolved_seeds: 0,
                ambiguous_seeds: 0,
                unavailable_seeds: 0,
                input_files: inputs.len(),
                unavailable_files: inputs
                    .iter()
                    .filter(|input| unavailable(input).is_some())
                    .count(),
                discovery_incomplete: false,
                planning_omitted_definitions: inputs
                    .iter()
                    .map(|input| input.planning.omitted_definitions)
                    .sum(),
                output_omitted_files: 0,
                selected_tokens: 0,
                selected_files: 0,
                omitted_definitions: 0,
                output_omitted: 0,
                files: inputs
                    .iter()
                    .enumerate()
                    .map(|(index, input)| DefinitionPlanFile {
                        id: index + 1,
                        path: input.path.clone(),
                        snapshot: input.snapshot.clone(),
                        sha256: input.planning.sha256.clone(),
                        extraction: input.definitions.status,
                        definitions: input.definitions.definitions.len(),
                        costed_definitions: input.planning.definitions.len(),
                        planning_omitted_definitions: input.planning.omitted_definitions,
                    })
                    .collect(),
                selected: Vec::new(),
                omissions: Vec::new(),
                omitted_details: 0,
                source: None,
            },
        }
    }

    fn seed(&mut self, seed: &DefinitionSeed) {
        let matching = self
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, input)| input.path == seed.path && input.snapshot == seed.snapshot)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [file] = matching.as_slice() else {
            let reason = if matching.is_empty() {
                "unresolved-file"
            } else {
                "ambiguous-file"
            };
            if matching.is_empty() {
                self.report.unresolved_seeds += 1;
            } else {
                self.report.ambiguous_seeds += 1;
            }
            self.push_omission(DefinitionPlanOmission {
                path: seed.path.clone(),
                name: selector_name(&seed.selector),
                reason: reason.into(),
                explicit: true,
            });
            return;
        };
        let file = *file;
        if let Some(reason) = unavailable(&self.inputs[file]) {
            self.report.unavailable_seeds += 1;
            self.omit(file, selector_name(&seed.selector), reason, true);
            return;
        }
        let definitions = select(&self.inputs[file].definitions, &seed.selector);
        if definitions.is_empty() {
            if self.inputs[file].definitions.status == DefinitionStatus::ParseErrors {
                self.report.unavailable_seeds += 1;
                self.omit(file, selector_name(&seed.selector), "parse-error", true);
            } else {
                self.report.unresolved_seeds += 1;
                self.omit(
                    file,
                    selector_name(&seed.selector),
                    "unresolved-definition",
                    true,
                );
            }
            return;
        }
        if definitions.len() > 1 && !matches!(seed.selector, DefinitionSelector::File) {
            self.report.ambiguous_seeds += 1;
            for definition in definitions {
                self.candidates.insert((file, definition));
                self.failures
                    .insert((file, definition), ("ambiguous-definition".into(), true));
            }
            return;
        }
        let reason = match seed.selector {
            DefinitionSelector::Symbol(_) => "explicit-symbol",
            DefinitionSelector::Line(_) => "explicit-line",
            DefinitionSelector::File => "explicit-file",
        };
        for definition in definitions {
            self.admit((file, definition), "direct", reason, true);
        }
    }

    fn admit(&mut self, candidate: Candidate, role: &str, reason: &str, explicit: bool) -> bool {
        self.candidates.insert(candidate);
        if let Some(&index) = self.selected.get(&candidate) {
            let selected = &mut self.report.selected[index];
            if !selected.reasons.iter().any(|existing| existing == reason) {
                selected.reasons.push(reason.into());
            }
            return true;
        }
        match self.selection(candidate, role, reason) {
            Ok((selected, total)) => {
                self.failures.remove(&candidate);
                self.selected.insert(candidate, self.report.selected.len());
                self.report.selected.push(selected);
                self.report.selected_tokens = total;
                true
            }
            Err(failure) => {
                self.failures
                    .entry(candidate)
                    .or_insert_with(|| (failure.into(), explicit));
                false
            }
        }
    }

    fn selection(
        &self,
        (file, definition): Candidate,
        role: &str,
        reason: &str,
    ) -> Result<(PlannedDefinition, usize), &'static str> {
        let input = &self.inputs[file];
        if let Some(reason) = unavailable(input) {
            return Err(reason);
        }
        if input.planning.encoding != self.report.encoding {
            return Err("encoding-mismatch");
        }
        let fact = input
            .definitions
            .definitions
            .get(definition)
            .ok_or("invalid-environment")?;
        let span = fact.source_span.ok_or("source-unavailable")?;
        if span.start_byte >= span.end_byte {
            return Err("source-unavailable");
        }
        let cost = self.cost((file, definition)).ok_or("cost-unavailable")?;
        let selected = PlannedDefinition {
            file: file + 1,
            definition,
            name: fact.symbol.name.clone(),
            kind: fact.symbol.kind.clone(),
            declaration_span: fact.declaration_span,
            source_span: span,
            tokens: cost.tokens,
            role: role.into(),
            reasons: vec![reason.into()],
            environment_gaps: cost.gaps.clone(),
        };
        if self.report.selected.len() >= self.report.max_definitions {
            return Err("definition-limit");
        }
        let files = self
            .report
            .selected
            .iter()
            .map(|item| item.file)
            .collect::<BTreeSet<_>>();
        if !files.contains(&(file + 1)) && files.len() >= self.report.max_files {
            return Err("file-limit");
        }
        let total = union_cost(
            self.report
                .selected
                .iter()
                .chain(std::iter::once(&selected)),
        )?;
        if total > self.report.token_budget {
            return Err(if cost.tokens > self.report.token_budget {
                "oversized-definition"
            } else {
                "token-budget"
            });
        }
        Ok((selected, total))
    }

    fn cost(&self, (file, definition): Candidate) -> Option<&DefinitionPlanningFact> {
        self.inputs[file]
            .planning
            .definitions
            .iter()
            .find(|fact| fact.definition == definition)
    }

    fn environment(&mut self, candidate: Candidate) {
        let Some(cost) = self.cost(candidate) else {
            return;
        };
        let environment = cost.environment.clone();
        for need in environment {
            let target = (candidate.0, need.definition);
            if !self.admit(target, "environment", &need.reason, false)
                && let Some(&index) = self.selected.get(&candidate)
            {
                let failure = self
                    .failures
                    .get(&target)
                    .map_or("unavailable", |(reason, _)| reason.as_str());
                let gap = format!("environment:{}:{failure}", need.reason);
                let gaps = &mut self.report.selected[index].environment_gaps;
                if !gaps.contains(&gap) {
                    gaps.push(gap);
                }
            }
        }
    }

    fn omit(&mut self, file: usize, name: Option<String>, reason: &str, explicit: bool) {
        self.push_omission(DefinitionPlanOmission {
            path: self.inputs[file].path.clone(),
            name,
            reason: reason.into(),
            explicit,
        });
    }

    fn push_omission(&mut self, omission: DefinitionPlanOmission) {
        if self.report.omissions.len() < 32 {
            self.report.omissions.push(omission);
        } else {
            self.report.omitted_details += 1;
        }
    }

    fn finish(mut self) -> DefinitionPlanReport {
        let failures = std::mem::take(&mut self.failures);
        for ((file, definition), (reason, explicit)) in failures {
            if self.selected.contains_key(&(file, definition)) {
                continue;
            }
            let name = self.inputs[file]
                .definitions
                .definitions
                .get(definition)
                .map(|fact| fact.symbol.name.clone());
            self.omit(file, name, &reason, explicit);
        }
        self.report.candidate_definitions = self.candidates.len();
        self.report.omitted_definitions = self.candidates.len().saturating_sub(self.selected.len());
        self.report.selected_files = self
            .report
            .selected
            .iter()
            .map(|item| item.file)
            .collect::<BTreeSet<_>>()
            .len();
        self.report
    }
}

fn unavailable(input: &DefinitionPlanInput) -> Option<&'static str> {
    match input.definitions.status {
        DefinitionStatus::Unsupported => Some("unsupported-extraction"),
        DefinitionStatus::Unavailable => Some("unavailable-extraction"),
        _ if input.planning.sha256.is_empty() => Some("cost-unavailable"),
        _ => None,
    }
}

fn selector_name(selector: &DefinitionSelector) -> Option<String> {
    match selector {
        DefinitionSelector::Symbol(name) => Some(name.clone()),
        DefinitionSelector::Line(line) => Some(format!("line:{line}")),
        DefinitionSelector::File => None,
    }
}

fn select(facts: &DefinitionFacts, selector: &DefinitionSelector) -> Vec<usize> {
    match selector {
        DefinitionSelector::File => (0..facts.definitions.len()).collect(),
        DefinitionSelector::Symbol(name) => {
            let exact = facts
                .definitions
                .iter()
                .enumerate()
                .filter(|(_, fact)| fact.symbol.name == *name)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if !exact.is_empty() {
                return exact;
            }
            facts
                .definitions
                .iter()
                .enumerate()
                .filter(|(_, fact)| {
                    fact.symbol
                        .name
                        .rsplit(['.', ':', '\\'])
                        .find(|part| !part.is_empty())
                        == Some(name.as_str())
                })
                .map(|(index, _)| index)
                .collect()
        }
        DefinitionSelector::Line(line) => {
            let mut matches = facts
                .definitions
                .iter()
                .enumerate()
                .filter(|(_, fact)| {
                    fact.declaration_span.start_line <= *line
                        && *line <= fact.declaration_span.end_line
                })
                .collect::<Vec<_>>();
            matches.sort_by_key(|(_, fact)| {
                (
                    std::cmp::Reverse(fact.declaration_span.start_byte),
                    fact.declaration_span.end_byte,
                )
            });
            let mut retained = Vec::new();
            let mut min_end: Option<usize> = None;
            for group in matches.chunk_by(|(_, left), (_, right)| {
                left.declaration_span.start_byte == right.declaration_span.start_byte
                    && left.declaration_span.end_byte == right.declaration_span.end_byte
            }) {
                let end = group[0].1.declaration_span.end_byte;
                if min_end.is_none_or(|previous| previous > end) {
                    retained.extend(group.iter().map(|(index, _)| *index));
                }
                min_end = Some(min_end.map_or(end, |previous| previous.min(end)));
            }
            retained.sort_unstable();
            retained
        }
    }
}

fn contains(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && outer.end_byte >= inner.end_byte
}

fn union_cost<'a>(
    items: impl Iterator<Item = &'a PlannedDefinition>,
) -> Result<usize, &'static str> {
    let mut ordered = items.collect::<Vec<_>>();
    ordered.sort_by_key(|item| {
        (
            item.file,
            item.source_span.start_byte,
            std::cmp::Reverse(item.source_span.end_byte),
        )
    });
    let mut outer: Option<&PlannedDefinition> = None;
    let mut total: usize = 0;
    for item in ordered {
        if let Some(previous) = outer.filter(|previous| previous.file == item.file) {
            if contains(previous.source_span, item.source_span) {
                if previous.source_span == item.source_span && previous.tokens != item.tokens {
                    return Err("inconsistent-span-cost");
                }
                continue;
            }
            if previous.source_span.end_byte > item.source_span.start_byte {
                return Err("crossing-source-spans");
            }
        }
        total = total.checked_add(item.tokens).ok_or("cost-overflow")?;
        outer = Some(item);
    }
    Ok(total)
}
