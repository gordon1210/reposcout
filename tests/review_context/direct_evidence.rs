use super::regressions::revisions;
use super::*;
use reposcout::model::ReviewContextReport;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

const TOKENS: usize = 4096;
const JSON_BYTES: usize = 12_288;
const TABLE_BYTES: usize = 6144;

fn source_hash(source: &str) -> String {
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(hash, "{byte:02x}").unwrap();
    }
    hash
}

#[derive(Clone, Copy)]
struct Layout {
    target: &'static str,
    product: &'static str,
    test: &'static str,
    target_name: &'static str,
    product_name: &'static str,
    test_name: &'static str,
}

const LAYOUTS: [Layout; 2] = [
    Layout {
        target: "src/aaa.ts",
        product: "src/zzz.ts",
        test: "tests/zzz.test.ts",
        target_name: "step",
        product_name: "pull",
        test_name: "probe",
    },
    Layout {
        target: "src/zzz.ts",
        product: "src/aaa.ts",
        test: "tests/aaa.test.ts",
        target_name: "turn",
        product_name: "read",
        test_name: "check",
    },
];

struct Example {
    fixture: Fixture,
    layout: Layout,
    sources: Vec<(String, String)>,
    head_target: String,
}

impl Example {
    fn new(layout: Layout) -> Self {
        let target_module = layout.target.strip_prefix("src/").unwrap();
        let target_module = target_module.strip_suffix(".ts").unwrap();
        let product_module = layout.product.strip_suffix(".ts").unwrap();
        let target = format!(
            "export function {}(n: number) {{ return n + 1; }}\n",
            layout.target_name
        );
        let head_target = target.replace("n + 1", "n + 2");
        let product = format!(
            "import {{ {} as hop }} from './{target_module}';\n\
             export function {}(n: number) {{ return hop(n); }}\n\
             export function late(obj: any, n: number) {{ return obj.{}(n); }}\n",
            layout.target_name, layout.product_name, layout.target_name
        );
        let direct_test = format!(
            "import {{ {} as hop }} from '../src/{target_module}';\n\
             export function {}() {{ return hop(3) === 4; }}\n",
            layout.target_name, layout.test_name
        );
        let mut sources = vec![
            (layout.target.into(), target),
            (layout.product.into(), product),
            (layout.test.into(), direct_test),
        ];
        let mut value = 0;
        for name in ["aaa", "bbb", "ccc", "ddd", "eee", "fff", "zzz"] {
            let path = format!("tests/{name}.test.ts");
            if path == layout.test {
                continue;
            }
            sources.push((
                path,
                format!(
                    "import {{ {} }} from '../{product_module}';\n\
                     export function {}() {{ return {}({value}) === {}; }}\n",
                    layout.product_name,
                    layout.test_name,
                    layout.product_name,
                    value + 1
                ),
            ));
            value += 1;
        }
        let old: Vec<_> = sources
            .iter()
            .map(|(path, source)| (path.as_str(), source.as_str()))
            .collect();
        let fixture = revisions(&old, &[(layout.target, &head_target)]);
        Self {
            fixture,
            layout,
            sources,
            head_target,
        }
    }

    fn source(&self, side: &str, path: &str) -> &str {
        if side == "head" && path == self.layout.target {
            &self.head_target
        } else {
            &self.sources.iter().find(|(p, _)| p == path).unwrap().1
        }
    }

    fn hash(&self, side: &str, path: &str) -> String {
        source_hash(self.source(side, path))
    }

    fn tree(&self, side: &str) -> String {
        let repository = Repository::open(self.fixture.path()).unwrap();
        let oid = if side == "base" {
            self.fixture.base
        } else {
            self.fixture.head
        };
        repository.find_commit(oid).unwrap().tree_id().to_string()
    }

    fn callers(&self) -> [(&str, &str, &str); 2] {
        [
            (self.layout.product, self.layout.product_name, "hop(n)"),
            (self.layout.test, self.layout.test_name, "hop(3)"),
        ]
    }
}

fn bounded_command(example: &Example, format: &str, bytes: usize) -> assert_cmd::Command {
    let mut command = test_command::reposcout_command();
    command
        .arg("review-context")
        .arg(example.fixture.path())
        .args([
            "--base",
            &example.fixture.base.to_string(),
            "--head",
            &example.fixture.head.to_string(),
            "--no-cache",
            "--no-project-config",
            "--encoding",
            "o200k_base",
            "--budget",
            &TOKENS.to_string(),
            "--max-output-bytes",
            &bytes.to_string(),
            "-f",
            format,
        ]);
    command
}

fn output(command: &mut assert_cmd::Command) -> String {
    String::from_utf8(command.assert().success().get_output().stdout.clone()).unwrap()
}

fn assert_bounds(text: &str, bytes: usize) {
    assert!(text.len() <= bytes, "byte budget exceeded");
    assert!(
        TokenCounter::new("o200k_base").unwrap().count(text) <= TOKENS,
        "rendered token budget exceeded"
    );
}

fn assert_declaration(symbol: &Value, source: &str, path: &str, name: &str, line: u64) {
    let start = source.find(&format!("export function {name}(")).unwrap();
    let end = start + source[start..].find('\n').unwrap();
    assert_eq!(symbol["path"], path);
    assert_eq!(symbol["name"], name);
    assert_eq!(symbol["source_hash"], source_hash(source));
    assert_eq!(symbol["declaration_span"]["start_line"], line);
    assert_eq!(symbol["declaration_span"]["end_line"], line);
    assert_eq!(symbol["declaration_span"]["start_byte"], start);
    assert_eq!(symbol["declaration_span"]["end_byte"], end);
}

fn json_evidence_gaps(report: &Value, example: &Example) -> Vec<String> {
    let mut gaps = Vec::new();
    for side in ["base", "head"] {
        assert_eq!(
            report["comparison"][format!("{side}_tree")],
            example.tree(side)
        );
        let changed = &report["changes"][0][side];
        assert_eq!(changed["path"], example.layout.target);
        assert_eq!(changed["sha256"], example.hash(side, example.layout.target));
        assert_eq!(changed["mapping_status"], "available");
        for (path, name, call) in example.callers() {
            let matching: Vec<_> = report["relations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|relation| {
                    relation["side"] == side
                        && relation["kind"] == "symbol-reference"
                        && relation["symbol"]["source"]["path"] == path
                        && relation["symbol"]["source"]["name"] == name
                        && relation["symbol"]["target"]["path"] == example.layout.target
                })
                .collect();
            if matching.is_empty() {
                gaps.push(format!("missing {side} direct caller {path}:{name}"));
                continue;
            }
            assert_eq!(matching.len(), 1, "duplicate direct call evidence");
            let relation = matching[0];
            assert_eq!(relation["change_basis"], "changed-definition");
            assert_eq!(relation["symbol"]["kind"], "call");
            assert_eq!(relation["symbol"]["syntax"], "imported-binding");
            assert_ne!(relation["edge"]["resolver"].as_str().unwrap(), "");
            assert_eq!(relation["edge"]["source"], path);
            assert_eq!(relation["edge"]["target"], example.layout.target);
            let source = example.source(side, path);
            assert_declaration(&relation["symbol"]["source"], source, path, name, 2);
            assert_declaration(
                &relation["symbol"]["target"],
                example.source(side, example.layout.target),
                example.layout.target,
                example.layout.target_name,
                1,
            );
            let start = source.find(call).unwrap();
            assert_eq!(relation["symbol"]["site"]["start_byte"], start);
            assert_eq!(relation["symbol"]["site"]["end_byte"], start + call.len());
            assert_eq!(relation["symbol"]["site"]["start_line"], 2);
            assert_eq!(relation["symbol"]["site"]["end_line"], 2);
        }
    }
    gaps
}

fn assert_uncertainty_and_totals(report: &Value, example: &Example) {
    assert_eq!(report["totals"]["changes"], 1);
    assert_eq!(report["totals"]["changes_omitted"], 0);
    for side in report["coverage"].as_array().unwrap() {
        assert_eq!(side["captured_files"], 9);
        assert_eq!(side["parse_errors"], 0);
        assert_eq!(side["config_errors"], 0);
        assert_eq!(side["incomplete_call_files"], 0);
        assert_eq!(side["call_resolution"]["omitted"], 0);
        assert!(side["call_resolution"]["unresolved"].as_u64().unwrap() > 0);
    }
    assert!(
        report["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| { line.as_str().unwrap().contains("dynamic/runtime") })
    );
    assert!(
        report["relations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|relation| {
                relation["symbol"]["source"]["path"] != example.layout.product
                    || relation["symbol"]["source"]["name"] != "late"
            })
    );
    for (total, entries) in [("relations", "relations"), ("candidates", "context")] {
        assert_eq!(
            report["totals"][total].as_u64().unwrap(),
            report[entries].as_array().unwrap().len() as u64
                + report["totals"][format!("{total}_omitted")]
                    .as_u64()
                    .unwrap()
        );
    }
    assert!(
        report["context"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| { file.get("source").is_none() })
    );
    assert!(
        report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| { change.get("diff").is_none() })
    );
}

fn table_evidence_gaps(text: &str, example: &Example) -> Vec<String> {
    let mut gaps = Vec::new();
    assert!(text.contains(&format!(
        "Trees: {} -> {}",
        example.tree("base"),
        example.tree("head")
    )));
    assert!(text.contains("dynamic/runtime"));
    for side in ["base", "head"] {
        let changed_prefix = format!("  {side}: {} ", example.layout.target);
        assert!(text.lines().any(|line| {
            line.starts_with(&changed_prefix)
                && line.contains(&format!(
                    "hash={}",
                    example.hash(side, example.layout.target)
                ))
        }));
        for (path, name, _) in example.callers() {
            let relation = format!(
                "{side} symbol-reference: {path} -> {} [",
                example.layout.target
            );
            let detail = format!(
                "  Call {name}:2 -> {}:1; site=2-2 syntax=ImportedBinding",
                example.layout.target_name
            );
            let lines: Vec<_> = text.lines().collect();
            let has_relation = lines.iter().enumerate().any(|(index, line)| {
                line.starts_with(&relation)
                    && lines.get(index + 1) == Some(&"  change basis: changed definition")
                    && lines.get(index + 2) == Some(&detail.as_str())
            });
            let has_handle = text.lines().any(|line| {
                line.starts_with(&format!("Read {side} {path}: "))
                    && line.contains(&format!(
                        "snapshot={} hash={}",
                        example.tree(side),
                        example.hash(side, path)
                    ))
            });
            if !has_relation || !has_handle {
                gaps.push(format!(
                    "{side} {path}: direct relation={has_relation}, snapshot/hash handle={has_handle}"
                ));
            }
        }
    }
    gaps
}

// A constructive feasibility witness, not the proposed allocation algorithm: retain
// the four independently required calls and drop every optional context row.
fn json_packet(ample: &Value) -> String {
    let mut packet: ReviewContextReport = serde_json::from_value(ample.clone()).unwrap();
    packet
        .relations
        .retain(|relation| relation.kind == "symbol-reference");
    assert_eq!(
        packet.relations.len(),
        4,
        "SETUP: unexpected producer relations"
    );
    packet.context.clear();
    packet.totals.relations_omitted = packet.totals.relations - packet.relations.len();
    packet.totals.candidates_omitted = packet.totals.candidates;
    packet.token_budget = TOKENS;
    packet.byte_budget = JSON_BYTES;
    format!("{}\n", serde_json::to_string(&packet).unwrap())
}

// Keep complete existing human blocks, changing only the two omission counters.
// This supplies a feasible packet without exporting or duplicating a renderer.
fn table_packet(ample: &str, report: &Value, example: &Example) -> String {
    let mut keep = true;
    let mut packet = String::new();
    for line in ample.lines() {
        if line.starts_with("Read ") {
            keep = ["base", "head"].into_iter().any(|side| {
                example
                    .callers()
                    .iter()
                    .any(|(path, _, _)| line.starts_with(&format!("Read {side} {path}: ")))
            });
        } else if line.starts_with("base ") || line.starts_with("head ") {
            keep = line.contains(" symbol-reference: ");
        }
        if keep {
            packet.push_str(line);
            packet.push('\n');
        }
    }
    let relations = report["totals"]["relations"].as_u64().unwrap();
    let candidates = report["totals"]["candidates"].as_u64().unwrap();
    packet
        .replace(
            &format!("relations: {relations} (0 omitted)"),
            &format!("relations: {relations} ({} omitted)", relations - 4),
        )
        .replace(
            "; 0 entries omitted",
            &format!("; {} entries omitted", candidates - 4),
        )
}

#[test]
fn direct_callers_keep_usable_identities_before_indirect_test_metadata() {
    let counter = TokenCounter::new("o200k_base").unwrap();
    let mut failures = Vec::new();
    for layout in LAYOUTS {
        let example = Example::new(layout);
        let ample = example.fixture.report(&["--encoding", "o200k_base"]);
        assert!(
            json_evidence_gaps(&ample, &example).is_empty(),
            "SETUP: missing producer evidence"
        );
        assert_uncertainty_and_totals(&ample, &example);
        assert_eq!(ample["totals"]["candidates"], 18);
        assert_eq!(ample["totals"]["relations_omitted"], 0);
        assert_eq!(ample["totals"]["candidates_omitted"], 0);
        let ample_table = output(
            &mut example
                .fixture
                .command_format(&["--encoding", "o200k_base"], "table"),
        );
        assert_eq!(
            table_evidence_gaps(&ample_table, &example),
            Vec::<String>::new()
        );
        for (format, bytes, packet) in [
            ("json", JSON_BYTES, json_packet(&ample)),
            (
                "table",
                TABLE_BYTES,
                table_packet(&ample_table, &ample, &example),
            ),
        ] {
            assert!(
                packet.len() <= bytes && counter.count(&packet) <= TOKENS,
                "SETUP: required {format} packet does not fit the frozen budget: {} bytes / {} tokens",
                packet.len(),
                counter.count(&packet)
            );
            let rendered = output(&mut bounded_command(&example, format, bytes));
            assert_bounds(&rendered, bytes);
            eprintln!(
                "direct-evidence {} {format}: {} bytes / {} configured output tokens\n{rendered}",
                layout.target,
                rendered.len(),
                counter.count(&rendered)
            );
            let gaps = if format == "json" {
                let report: Value = serde_json::from_str(&rendered).unwrap();
                assert_uncertainty_and_totals(&report, &example);
                assert_eq!(report["coverage"], ample["coverage"]);
                assert_eq!(report["totals"]["selected_files"], 0);
                assert!(
                    report["context"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|file| { file["selection"] == "not-requested" })
                );
                json_evidence_gaps(&report, &example)
            } else {
                for line in ample_table.lines().filter(|line| {
                    line.starts_with("Coverage ") || line.starts_with("  Changed graph files:")
                }) {
                    assert!(rendered.lines().any(|retained| retained == line));
                }
                table_evidence_gaps(&rendered, &example)
            };
            if !gaps.is_empty() {
                failures.push(format!("{} {format}: {}", layout.target, gaps.join("; ")));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "affordable direct evidence lost:\n{}",
        failures.join("\n")
    );
}

#[test]
fn direct_projection_preserves_test_first_source_selection_and_unknown_calls() {
    let example = Example::new(LAYOUTS[0]);
    let selection = [
        "--context",
        "--context-budget",
        "32000",
        "--context-max-files",
        "3",
    ];
    let ample = example.fixture.report(&[
        "--encoding",
        "o200k_base",
        "--context",
        "--context-budget",
        "32000",
        "--context-max-files",
        "3",
    ]);
    let selected: Vec<_> = ample["context"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["selection"] == "selected")
        .map(|file| {
            (
                file["side"].as_str().unwrap(),
                file["path"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        selected,
        [
            ("base", "src/aaa.ts"),
            ("head", "src/aaa.ts"),
            ("base", "tests/zzz.test.ts"),
        ]
    );
    let rendered = output(bounded_command(&example, "json", JSON_BYTES).args(selection));
    assert_bounds(&rendered, JSON_BYTES);
    let report: Value = serde_json::from_str(&rendered).unwrap();
    assert_uncertainty_and_totals(&report, &example);
    assert_eq!(report["coverage"], ample["coverage"]);
    for key in [
        "selected_files",
        "selected_tokens",
        "selection_omitted_files",
        "selection_omitted_tokens",
        "candidate_tokens",
        "candidate_bytes",
        "unknown_candidate_sizes",
        "diff_tokens",
        "source_files",
    ] {
        assert_eq!(report["totals"][key], ample["totals"][key], "{key}");
    }
    assert_eq!(report["totals"]["selected_files"], 3);
    assert_eq!(report["totals"]["selection_omitted_files"], 15);
    assert!(report["totals"]["candidates_omitted"].as_u64().unwrap() > 0);
    for file in report["context"].as_array().unwrap() {
        let side = file["side"].as_str().unwrap();
        let path = file["path"].as_str().unwrap();
        let expected = if selected.contains(&(side, path)) {
            "selected"
        } else {
            "file-limit"
        };
        assert_eq!(file["selection"], expected, "{side}:{path}");
    }
}

#[test]
fn unaffordable_status_envelope_is_an_error_not_an_empty_review() {
    let example = Example::new(LAYOUTS[0]);
    let mut command = test_command::reposcout_command();
    let result = command
        .arg("review-context")
        .arg(example.fixture.path())
        .args([
            "--base",
            &example.fixture.base.to_string(),
            "--head",
            &example.fixture.head.to_string(),
            "--no-cache",
            "--no-project-config",
            "--encoding",
            "o200k_base",
            "--budget",
            "256",
            "--max-output-bytes",
            "1024",
            "-f",
            "json",
        ])
        .assert()
        .failure();
    let result = result.get_output();
    assert_eq!(result.stdout, Vec::<u8>::new());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("output budget cannot hold the status envelope")
    );
}

fn assert_fanout_preserves_selection_and_coverage(report: &Value, original: &Value) {
    for key in [
        "unknown_candidate_sizes",
        "selected_files",
        "selected_tokens",
        "selection_omitted_files",
        "selection_omitted_tokens",
        "source_files",
        "diff_tokens",
    ] {
        assert_eq!(report["totals"][key], original["totals"][key], "{key}");
    }
    for (index, side) in report["coverage"].as_array().unwrap().iter().enumerate() {
        assert_eq!(side["observed_files"], 51);
        assert_eq!(side["captured_files"], 51);
        assert_eq!(side["graph_files"], 51);
        assert_eq!(side["inventory_truncated"], false);
        for key in [
            "parse_errors",
            "config_errors",
            "unsupported_call_files",
            "incomplete_call_files",
        ] {
            assert_eq!(side[key], original["coverage"][index][key], "{key}");
        }
        for key in ["unresolved", "unsupported", "omitted"] {
            assert_eq!(
                side["call_resolution"][key], original["coverage"][index]["call_resolution"][key],
                "{key}"
            );
        }
        assert!(side["call_resolution"]["unresolved"].as_u64().unwrap() > 0);
        for key in ["examined", "resolved"] {
            assert_eq!(
                side["call_resolution"][key].as_u64().unwrap(),
                original["coverage"][index]["call_resolution"][key]
                    .as_u64()
                    .unwrap()
                    + 42,
                "{key}"
            );
        }
    }
}

#[test]
fn direct_caller_handles_survive_the_hundred_candidate_cap() {
    let mut example = Example::new(LAYOUTS[0]);
    let original = example.fixture.report(&["--encoding", "o200k_base"]);
    assert_eq!(
        json_evidence_gaps(&original, &example),
        Vec::<String>::new()
    );

    // Forty-eight indirect tests put both product sides at positions 101/102 in
    // the existing source-selection order: changes, direct tests, indirect tests,
    // then the product neighbor. These extra files add only resolved calls.
    for value in 6..48 {
        example.sources.push((
            format!("tests/fan{value:02}.test.ts"),
            format!(
                "import {{ pull }} from '../src/zzz';\n\
                 export function probe() {{ return pull({value}) === {}; }}\n",
                value + 1
            ),
        ));
    }
    let old: Vec<_> = example
        .sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    example.fixture = revisions(&old, &[(example.layout.target, &example.head_target)]);

    let counter = TokenCounter::new("o200k_base").unwrap();
    let mut source_bytes = 0;
    let mut source_tokens = 0;
    for side in ["base", "head"] {
        for (path, _) in &example.sources {
            let source = example.source(side, path);
            source_bytes += source.len();
            source_tokens += counter.count(source);
        }
    }
    let rendered = output(&mut example.fixture.command(&["--encoding", "o200k_base"]));
    assert!(rendered.len() <= 1_048_576);
    assert!(counter.count(&rendered) <= 65_536);
    let report: Value = serde_json::from_str(&rendered).unwrap();
    let gaps = json_evidence_gaps(&report, &example);
    assert!(
        gaps.is_empty(),
        "direct relations lost at entry cap: {gaps:?}"
    );
    assert_eq!(report["totals"]["changes"], 1);
    assert_eq!(report["totals"]["candidates"], 102);
    // Each side has 50 imports and two calls touching the changed declaration.
    assert_eq!(report["totals"]["relations"], 104);
    assert_eq!(report["totals"]["candidate_bytes"], source_bytes);
    assert_eq!(report["totals"]["candidate_tokens"], source_tokens);
    for entries in ["changes", "context", "relations"] {
        assert!(report[entries].as_array().unwrap().len() <= 100);
    }
    for (total, entries) in [("candidates", "context"), ("relations", "relations")] {
        assert_eq!(
            report["totals"][total].as_u64().unwrap(),
            report[entries].as_array().unwrap().len() as u64
                + report["totals"][format!("{total}_omitted")]
                    .as_u64()
                    .unwrap()
        );
    }
    assert_fanout_preserves_selection_and_coverage(&report, &original);
    let table = output(
        &mut example
            .fixture
            .command_format(&["--encoding", "o200k_base"], "table"),
    );
    assert!(table.len() <= 1_048_576);
    assert!(counter.count(&table) <= 65_536);
    assert!(table.contains(&format!(
        "Context: 102 file sides / {source_tokens} source tokens / {source_bytes} bytes; 0 unknown sizes;"
    )));
    assert!(
        table
            .lines()
            .filter(|line| line.starts_with("Read "))
            .count()
            <= 100
    );
    assert!(
        table
            .lines()
            .filter(|line| line.starts_with("base ") || line.starts_with("head "))
            .count()
            <= 100
    );
    for side in ["base", "head"] {
        let prefix = format!(
            "Coverage {}: 51/51 captured; inventory truncated=false;",
            example.tree(side)
        );
        assert!(table.lines().any(|line| line.starts_with(&prefix)));
    }
    let gaps = table_evidence_gaps(&table, &example);
    assert!(
        gaps.is_empty(),
        "direct handles lost at entry cap: {gaps:?}"
    );
}
