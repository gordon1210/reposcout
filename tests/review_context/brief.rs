use super::regressions::revisions;
use super::*;
use std::fmt::Write as _;

fn brief_command(fixture: &Fixture, flags: &[&str]) -> assert_cmd::Command {
    let mut command = test_command::reposcout_command();
    command
        .arg("review-context")
        .arg(fixture.path())
        .args([
            "--base",
            &fixture.base.to_string(),
            "--head",
            &fixture.head.to_string(),
            "--no-cache",
            "--no-project-config",
            "--encoding",
            "o200k_base",
            "--brief",
        ])
        .args(flags);
    command
}

fn brief(fixture: &Fixture, flags: &[&str]) -> String {
    let output = brief_command(fixture, flags)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).unwrap()
}

fn tokens(text: &str) -> usize {
    TokenCounter::new("o200k_base").unwrap().count(text)
}

#[test]
fn brief_lists_definitions_diff_and_direct_consumers_without_identity_noise() {
    let fixture = fixture();
    let text = brief(&fixture, &[]);
    let base = fixture.base.to_string();
    let head = fixture.head.to_string();
    let header = text.lines().next().unwrap();
    assert!(header.contains(&format!("base {} head {}", &base[..12], &head[..12])));
    assert!(header.contains("changes 1"));
    assert!(text.contains("modified src/api.ts (1 hunk)\n  function answer 1-1\n"));
    assert!(text.contains(
        "\n-export function answer(): number { return 1; }\n+export function answer(): number { return 2; }\n"
    ));
    assert!(!text.contains("\\n"));
    let json = fixture.report(&[]);
    for identity in [
        json["comparison"]["base_tree"].as_str().unwrap(),
        json["changes"][0]["head"]["blob"].as_str().unwrap(),
        json["changes"][0]["head"]["sha256"].as_str().unwrap(),
    ] {
        assert!(!text.contains(identity), "{identity} in {text}");
    }
    assert!(!text.contains("Note:"));
    assert!(text.contains("  imported by: src/service.ts, tests/api.test.ts [test]\n"));
    // Base and head carry the same reference; the brief renders it once without a side marker.
    assert_eq!(
        text.matches("  call src/service.ts:2 run -> answer\n")
            .count(),
        1
    );
    assert!(text.contains("  call tests/api.test.ts:2 check -> answer [test]\n"));
    // The transitive dependent is counted; only its single entry chain names it.
    assert!(text.contains("counted, not listed: transitive dependents 1\n"));
    assert!(text.contains("  entry: src/app.ts <- src/service.ts\n"));
    assert_eq!(text.matches("src/app.ts").count(), 1);
    assert!(tokens(&text) <= 3000);
}

#[test]
fn brief_marks_single_revision_evidence_and_real_gaps() {
    let fixture = revisions(
        &[
            ("lib.ts", "export function value() { return 1; }\n"),
            (
                "old.ts",
                "import { value } from './lib';\nexport function before() { return value(); }\n",
            ),
            ("notes.py", "def helper():\n    return 1\n"),
        ],
        &[("old.ts", "export function before() { return 0; }\n")],
    );
    let text = brief(&fixture, &[]);
    assert!(text.contains("  imports: lib.ts (base only)\n"), "{text}");
    assert!(text.contains("call old.ts:2 before -> value @ lib.ts (base only)"));
    // Python has no call-reference resolver, which must read as a gap rather than zero evidence.
    assert!(text.contains("files without call-reference support 1"));
}

fn many_hunks(count: usize, changed: bool) -> String {
    let mut source = String::new();
    for index in 0..count {
        let value = if changed { index + 100 } else { index };
        writeln!(
            source,
            "export function f{index}() {{ return {value}; }}\n\n\n\n\n\n\n\n// keep {index}"
        )
        .unwrap();
    }
    source
}

#[test]
fn brief_budget_trims_trailing_hunks_with_exact_deterministic_counts() {
    let fixture = revisions(
        &[("many.ts", &many_hunks(40, false))],
        &[("many.ts", &many_hunks(40, true))],
    );
    let json = fixture.report(&[]);
    let total = usize::try_from(json["changes"][0]["hunks"].as_u64().unwrap()).unwrap();
    assert_eq!(total, 40);
    let text = brief(&fixture, &["--budget", "600", "--max-output-bytes", "4000"]);
    assert!(tokens(&text) <= 600);
    assert!(text.len() <= 4000);
    let shown = text.lines().filter(|line| line.starts_with("@@ ")).count();
    let omitted: usize = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("… "))
        .and_then(|rest| rest.strip_suffix(" more hunks omitted"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(shown > 0, "{text}");
    assert_eq!(shown + omitted, total);
    assert!(text.contains(&format!("{omitted} diff hunks")));
    assert_eq!(
        brief(&fixture, &["--budget", "600", "--max-output-bytes", "4000"]),
        text
    );
    // A tighter byte allowance removes more hunks; the omission still totals exactly.
    let small = brief(&fixture, &["--max-output-bytes", "1500"]);
    assert!(small.len() <= 1500);
    let shown = small.lines().filter(|line| line.starts_with("@@ ")).count();
    let omitted = total - shown;
    if shown == 0 {
        assert!(small.contains("1 diffs"), "{small}");
    } else {
        assert!(
            small.contains(&format!("{omitted} more hunks omitted")),
            "{small}"
        );
    }
}

#[test]
fn brief_default_budget_is_smaller_and_explicit_budget_wins() {
    let fixture = revisions(
        &[("many.ts", &many_hunks(400, false))],
        &[("many.ts", &many_hunks(400, true))],
    );
    let text = brief(&fixture, &[]);
    let used = tokens(&text);
    assert!(used <= 3000 && used > 1000, "{used}");
    let larger = brief(&fixture, &["--budget", "6000"]);
    assert!(tokens(&larger) > 3000);
    assert!(tokens(&larger) <= 6000);
}

#[test]
fn brief_is_text_only_and_excludes_reading_list_selection() {
    let fixture = fixture();
    for format in ["json", "ndjson"] {
        brief_command(&fixture, &["-f", format])
            .assert()
            .failure()
            .stderr(predicates::str::contains(
                "--brief supports table or Markdown",
            ));
    }
    for flag in ["--source", "--context"] {
        brief_command(&fixture, &[flag]).assert().failure();
    }
    let markdown = brief(&fixture, &["-f", "markdown"]);
    assert!(markdown.starts_with("```\nreview-context brief"));
    assert!(markdown.trim_end().ends_with("```"));
    assert_eq!(brief(&fixture, &["-f", "markdown"]), markdown);
}

#[test]
fn brief_doc_mentions_come_from_the_pinned_head_snapshot() {
    let mut fixture = revisions(
        &[
            (
                "src/score.ts",
                "export function scoreValue(): number { return 1; }\n",
            ),
            (
                "docs/guide.md",
                "# Guide\n\nThe score value contract is stable.\nUse `scoreValue` directly.\n",
            ),
            ("docs/old.md", "# Old\n\nscoreValue was documented here.\n"),
            (
                "docs/unrelated.md",
                "Nothing relevant; score and value differ.\n",
            ),
        ],
        &[(
            "src/score.ts",
            "export function scoreValue(): number { return 2; }\n",
        )],
    );
    fs::remove_file(fixture.path().join("docs/old.md")).unwrap();
    write(
        fixture.path(),
        "docs/changed.md",
        "scoreValue is described in a changed document.\n",
    );
    fixture.head = commit(fixture.path());
    write(
        fixture.path(),
        "docs/live.md",
        "scoreValue appears only in the dirty worktree.\n",
    );
    write(
        fixture.path(),
        "docs/guide.md",
        "scoreValue was edited after the head commit.\n",
    );
    let text = brief(&fixture, &[]);
    let mentions: Vec<_> = text
        .split("docs naming changed identifiers:\n")
        .nth(1)
        .unwrap_or_else(|| panic!("{text}"))
        .lines()
        .collect();
    assert_eq!(
        mentions,
        [
            "docs/guide.md:3 The score value contract is stable.",
            "docs/guide.md:4 Use `scoreValue` directly.",
        ]
    );
    for absent in ["live.md", "unrelated.md", "edited after"] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
}

#[test]
fn human_diff_and_source_bodies_keep_real_newlines_and_escape_controls() {
    let fixture = revisions(
        &[("lib.ts", "export function value() { return 1; }\n")],
        &[(
            "lib.ts",
            "export function value() { return 2; }\n// \u{1b}[31m\r\n",
        )],
    );
    for format in ["table", "markdown"] {
        let output = fixture
            .command_format(&["--diff", "--source"], format)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = String::from_utf8(output).unwrap();
        assert!(
            text.contains("\n+export function value() { return 2; }\n"),
            "{text}"
        );
        assert!(!text.contains("\\n+export"));
        assert!(text.contains("\\u{1b}[31m\\u{d}"));
        assert!(!text.contains('\u{1b}') && !text.contains('\r'));
        assert_eq!(format == "markdown", text.contains("\n```\n"));
    }
}

/// Indented lines under each change's `usage lines` header, in rendered order.
fn usage_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("  usage lines ") {
            inside = true;
        } else if inside && let Some(usage) = line.strip_prefix("    ") {
            lines.push(usage);
        } else {
            inside = false;
        }
    }
    lines
}

const CORE: &str =
    "def compute(value):\n    return value + 1\n\n\ndef helper():\n    return compute(1)\n";

#[test]
fn brief_usage_lines_name_change_identifiers_in_importers_without_references() {
    let fixture = revisions(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/core.py", CORE),
            (
                "pkg/use_a.py",
                "from pkg.core import compute\n\n\ndef run():\n    return compute(2)\n",
            ),
            (
                "pkg/use_b.py",
                "from pkg.core import (\n    helper as assist,\n)\n\n\ndef go():\n    return assist() + 1\n",
            ),
            (
                "pkg/use_c.py",
                "from pkg.core import compute\n\n__all__ = [\"other\"]\n",
            ),
        ],
        &[("pkg/core.py", &CORE.replace("value + 1", "value + 2"))],
    );
    let text = brief(&fixture, &[]);
    // The changed definition, a binding aliased from a declaration that uses it, and the import
    // line of an importer without any other matching line.
    assert_eq!(
        usage_lines(&text),
        [
            "pkg/use_a.py:5  return compute(2)",
            "pkg/use_b.py:7  return assist() + 1",
            "pkg/use_c.py:1  from pkg.core import compute",
        ],
        "{text}"
    );
    assert!(text.contains("  usage lines (identifier text match, no resolved reference):\n"));
    assert!(!text.contains("usage lines;") && !text.contains("usage lines,"));
    assert_eq!(brief(&fixture, &[]), text);
}

#[test]
fn brief_usage_lines_skip_importers_with_concrete_references() {
    let fixture = revisions(
        &[
            ("api.ts", "export function answer(): number { return 1; }\n"),
            (
                "service.ts",
                "import { answer } from './api';\nexport function run() { return answer(); }\n",
            ),
            ("index.ts", "export { answer } from './api';\n"),
        ],
        &[("api.ts", "export function answer(): number { return 2; }\n")],
    );
    let text = brief(&fixture, &[]);
    assert!(
        text.contains("  call service.ts:2 run -> answer\n"),
        "{text}"
    );
    assert_eq!(
        usage_lines(&text),
        ["index.ts:1  export { answer } from './api';"],
        "{text}"
    );
}

#[test]
fn brief_usage_lines_read_base_only_importers_from_the_base_snapshot() {
    let consumer = "from pkg.core import compute\n\n\ndef run():\n    return compute(2)\n";
    let mut fixture = revisions(
        &[
            ("pkg/__init__.py", ""),
            ("pkg/core.py", CORE),
            ("pkg/use_a.py", consumer),
        ],
        &[],
    );
    fs::remove_file(fixture.path().join("pkg/core.py")).unwrap();
    write(
        fixture.path(),
        "pkg/use_a.py",
        &format!("# compute moved away\n{consumer}"),
    );
    fixture.head = commit(fixture.path());
    let text = brief(&fixture, &[]);
    assert_eq!(
        usage_lines(&text),
        ["pkg/use_a.py:5 (base)  return compute(2)"],
        "{text}"
    );
}

#[test]
fn brief_usage_lines_are_bounded_round_robin_with_exact_omissions() {
    let mut files = vec![
        ("pkg/__init__.py".to_string(), String::new()),
        ("pkg/core.py".to_string(), CORE.to_string()),
    ];
    for index in 0..8 {
        let mut source = String::from("from pkg.core import compute\n");
        for call in 0..5 {
            writeln!(source, "\n\ndef run{call}():\n    return compute({call})").unwrap();
        }
        files.push((format!("pkg/use_{index}.py"), source));
    }
    let old: Vec<_> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let changed = CORE.replace("value + 1", "value + 2");
    let fixture = revisions(&old, &[("pkg/core.py", &changed)]);
    let text = brief(&fixture, &[]);
    let lines = usage_lines(&text);
    // Sixteen lines: every importer gets its first two lines before any gets a third.
    assert_eq!(lines.len(), 16, "{text}");
    for index in 0..8 {
        let prefix = format!("pkg/use_{index}.py:");
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with(&prefix))
                .count(),
            2
        );
    }
    assert!(lines.contains(&"pkg/use_0.py:5  return compute(0)"));
    assert!(lines.contains(&"pkg/use_0.py:9  return compute(1)"));
    assert!(
        text.contains("omitted from output: 24 usage lines;"),
        "{text}"
    );
    assert_eq!(brief(&fixture, &[]), text);

    // A tight budget removes the diff, then trailing usage lines; the counts stay exact.
    let small = brief(&fixture, &["--budget", "300"]);
    assert!(tokens(&small) <= 300, "{small}");
    let shown = usage_lines(&small).len();
    assert!(shown < 16, "{small}");
    assert!(
        small.contains(&format!("{} usage lines", 40 - shown)),
        "{small}"
    );
}
