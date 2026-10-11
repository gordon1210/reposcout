use super::regressions::revisions;
use super::*;
use std::collections::BTreeSet;
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

/// The abbreviated commit id that brief commands use.
fn short(commit: Oid) -> String {
    commit.to_string()[..12].to_string()
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
    // Base and head carry the same references; the brief groups them by target once, without a
    // side marker.
    assert_eq!(
        text.matches("  call -> answer: src/service.ts:2 run, tests/api.test.ts:2 check [test]\n")
            .count(),
        1,
        "{text}"
    );
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
            (
                "own.ts",
                "import { value } from './lib';\nexport function mine() { return value(); }\n",
            ),
            ("notes.py", "def helper():\n    return 1\n"),
            ("other.py", "def unrelated():\n    return 1\n"),
        ],
        &[
            ("lib.ts", "export function renamed() { return 1; }\n"),
            (
                "own.ts",
                "import { value } from './lib';\nexport function mine() { return value() + 1; }\n",
            ),
            ("notes.py", "def helper():\n    return 2\n"),
        ],
    );
    let text = brief(&fixture, &[]);
    // An unchanged caller of a definition that head no longer has is a base-only reference.
    assert!(
        text.contains("  call -> value (base only): old.ts:2 before\n"),
        "{text}"
    );
    // Imports, outgoing calls and calls from changed lines are visible in the diff.
    assert!(!text.contains("imports:"), "{text}");
    assert!(!text.contains("own.ts:2 mine"), "{text}");
    // Python has no call-reference resolver; only changed files count, never repository totals.
    assert!(
        text.contains("gaps: changed files without call references 1\n"),
        "{text}"
    );
    assert!(!text.contains("unresolved references"), "{text}");
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
        .find_map(|line| line.strip_prefix("omitted from output: "))
        .and_then(|rest| rest.split_once(" hunks (listed at the end)"))
        .unwrap_or_else(|| panic!("{text}"))
        .0
        .parse()
        .unwrap();
    assert!(shown > 0, "{text}");
    assert_eq!(shown + omitted, total);
    // Every omitted hunk is listed by its ranges, followed by the command that prints it.
    let listed = text
        .lines()
        .find_map(|line| line.strip_prefix("  many.ts @@ "))
        .unwrap_or_else(|| panic!("{text}"));
    assert_eq!(listed.split(' ').count(), omitted, "{text}");
    assert!(text.contains(&format!("omitted hunks ({omitted} in 1 files):\n")));
    let (base, head) = (short(fixture.base), short(fixture.head));
    assert!(
        text.ends_with(&format!(
            "  git diff --no-indent-heuristic {base} {head} -- ':(top,literal)many.ts'\n"
        )),
        "{text}"
    );
    assert_eq!(
        brief(&fixture, &["--budget", "600", "--max-output-bytes", "4000"]),
        text
    );
    // A tighter byte allowance removes more hunks; the omission still totals exactly.
    let small = brief(&fixture, &["--max-output-bytes", "1500"]);
    assert!(small.len() <= 1500);
    let shown = small.lines().filter(|line| line.starts_with("@@ ")).count();
    let omitted = total - shown;
    assert!(
        small.contains(&format!("omitted from output: {omitted} hunks")),
        "{small}"
    );
    assert!(small.contains(&format!(
        "git diff --no-indent-heuristic {base} {head} -- ':(top,literal)many.ts'"
    )));
}

#[test]
fn brief_default_budget_is_capped_and_explicit_budget_wins() {
    let fixture = revisions(
        &[("many.ts", &many_hunks(400, false))],
        &[("many.ts", &many_hunks(400, true))],
    );
    let text = brief(&fixture, &[]);
    let used = tokens(&text);
    assert!(used <= 4096 && used > 2000, "{used}");
    let smaller = brief(&fixture, &["--budget", "3000"]);
    assert!(tokens(&smaller) <= 3000 && tokens(&smaller) > 2000);
    let larger = brief(&fixture, &["--budget", "12000"]);
    assert!(tokens(&larger) > 8000 && tokens(&larger) <= 12000);
    // A brief below the default ceiling renders in full.
    let small = revisions(
        &[("few.ts", &many_hunks(3, false))],
        &[("few.ts", &many_hunks(3, true))],
    );
    let full = brief(&small, &[]);
    assert!(!full.contains("omitted"), "{full}");
    assert_eq!(brief(&small, &["--budget", "60000"]), full);
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
        .split("docs naming changed definitions:\n")
        .nth(1)
        .unwrap_or_else(|| panic!("{text}"))
        .lines()
        .collect();
    // Identifier spellings match; the same words in ordinary prose do not.
    assert_eq!(mentions, ["docs/guide.md:4 Use `scoreValue` directly."]);
    for absent in [
        "live.md",
        "unrelated.md",
        "edited after",
        "contract is stable",
    ] {
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
        text.contains("  call -> answer: service.ts:2 run\n"),
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
        text.contains("omitted from output: 24 usage lines\n"),
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

fn call_site(name: &str, import: &str, call: &str) -> String {
    format!(
        "import {{ {import} }} from './r';\nexport function {name}(x: number) {{ return {call}; }}\n"
    )
}

/// Five callers migrate from `round(x, 5, true)` to an options object and `f4` picks another
/// mode; `total` moves from `m.ts` to `moved.ts` with one edited line; two files move unchanged.
fn migration() -> Fixture {
    let mut base = vec![
        (
            "r.ts".to_string(),
            "export function round(x: number, step: number, up: boolean): number { return x; }\n"
                .to_string(),
        ),
        (
            "m.ts".to_string(),
            "export function keep(value: number) { return value; }\n\nexport function total(items: number[]) {\n  const sum = items.reduce((a, b) => a + b, 0);\n  const tax = sum * 0.2;\n  return sum + tax;\n}\n".to_string(),
        ),
        ("same1.ts".to_string(), "export const a = 1;\n".to_string()),
        ("same2.ts".to_string(), "export const b = 2;\n".to_string()),
    ];
    let mut head = vec![
        (
            "r.ts".to_string(),
            "export function quantize(x: number, options: { step: number; mode: string }): number { return x; }\n"
                .to_string(),
        ),
        (
            "m.ts".to_string(),
            "export function keep(value: number) { return value; }\n".to_string(),
        ),
        (
            "moved.ts".to_string(),
            "export function total(items: number[]) {\n  const sum = items.reduce((a, b) => a + b, 0);\n  const tax = sum * rate(items);\n  return sum + tax;\n}\n".to_string(),
        ),
    ];
    for index in 0..5 {
        let name = format!("f{index}");
        let mode = if index == 4 { "down" } else { "up" };
        base.push((
            format!("{name}.ts"),
            call_site(&name, "round", "round(x, 5, true)"),
        ));
        head.push((
            format!("{name}.ts"),
            call_site(
                &name,
                "quantize",
                &format!("quantize(x, {{ step: 5, mode: '{mode}' }})"),
            ),
        ));
    }
    let base: Vec<_> = base
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let head: Vec<_> = head
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let mut fixture = revisions(&base, &head);
    fs::create_dir_all(fixture.path().join("lib")).unwrap();
    for name in ["same1.ts", "same2.ts"] {
        fs::rename(
            fixture.path().join(name),
            fixture.path().join("lib").join(name),
        )
        .unwrap();
    }
    fixture.head = commit(fixture.path());
    fixture
}

#[test]
fn brief_shows_rule_explained_hunks_compactly_and_the_deviating_hunk_in_full() {
    let fixture = migration();
    let text = brief(&fixture, &[]);
    assert!(
        text.contains(concat!(
            "  R1 `round` → `quantize` (6 hunks)\n",
            "  R2 insert `{ step :` (5 hunks)\n",
            "  R3 `true` → `mode : 'up' }` (4 hunks)\n",
        )),
        "{text}"
    );
    // Hunks with edits other than renames show their changed lines, numbered, without context.
    assert!(
        text.contains(concat!(
            "modified f0.ts (1 hunk)\n",
            "  function f0 2-2\n",
            "@@ -1,2 +1,2 @@ [R1 R2 R3]\n",
            "-1: import { round } from './r';\n",
            "-export function f0(x: number) { return round(x, 5, true); }\n",
            "+1: import { quantize } from './r';\n",
            "+export function f0(x: number) { return quantize(x, { step: 5, mode: 'up' }); }\n",
        )),
        "{text}"
    );
    // The literal outlier stays visible with its rewrites, deviation and hidden lines named.
    assert!(
        text.contains(concat!(
            "modified f4.ts (1 hunk)\n",
            "  function f4 2-2\n",
            "@@ -1,2 +1,2 @@ [R1 R2; deviates from R3; 2 lines hidden: unchanged after rewrites]\n",
            "-2: export function f4(x: number) { return round(x, 5, true); }\n",
            "+2: export function f4(x: number) { return quantize(x, { step: 5, mode: 'down' }); }\n",
        )),
        "{text}"
    );
    for absent in ["  rewrite-only hunks:\n", "omitted"] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
    assert_eq!(brief(&fixture, &[]), text);
}

#[test]
fn brief_lists_pure_renames_and_shows_edited_moved_declarations() {
    let fixture = migration();
    let text = brief(&fixture, &[]);
    assert!(
        text.contains(
            "renamed or mode changed without a shown hunk: same1.ts -> lib/same1.ts, same2.ts -> lib/same2.ts\n"
        ),
        "{text}"
    );
    assert!(!text.contains("renamed same1.ts"), "{text}");
    // A declaration with an edited line does not move; both sides stay whole.
    for line in [
        "\n-  const sum = items.reduce((a, b) => a + b, 0);\n-  const tax = sum * 0.2;\n",
        "\n+  const sum = items.reduce((a, b) => a + b, 0);\n+  const tax = sum * rate(items);\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("hidden: moved"), "{text}");
}

#[test]
fn brief_budget_omits_listed_hunks_only_and_stays_deterministic() {
    let fixture = migration();
    for budget in ["3000", "1000", "400"] {
        let text = brief(&fixture, &["--budget", budget]);
        assert!(tokens(&text) <= budget.parse().unwrap(), "{text}");
        assert_eq!(brief(&fixture, &["--budget", budget]), text);
        // Compact hunks are admitted like other hunks and counted when omitted.
        if !text.contains("modified f0.ts") {
            assert!(text.contains("omitted"), "{text}");
        }
    }
}

#[test]
fn brief_flags_distinctive_old_spellings_left_in_head() {
    let tariff = |index: usize, field: &str, count: &str| {
        format!("export const t{index} = {{ {field}: 15, {count}: 1 }};\n")
    };
    let mut base: Vec<_> = (0..4)
        .map(|index| {
            (
                format!("t{index}.ts"),
                tariff(index, "laborIncrementMinutes", "count"),
            )
        })
        .collect();
    base.push((
        "notes.md".to_string(),
        "# Tariffs\n\nEach tariff sets laborIncrementMinutes.\nThe count of tariffs grows.\n"
            .to_string(),
    ));
    let head: Vec<_> = (0..3)
        .map(|index| {
            (
                format!("t{index}.ts"),
                tariff(index, "laborStepMinutes", "total"),
            )
        })
        .collect();
    let base: Vec<_> = base
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let head: Vec<_> = head
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let fixture = revisions(&base, &head);
    let text = brief(&fixture, &[]);
    assert!(
        text.contains(concat!(
            "old spellings still in head (verify intent):\n",
            "  `laborIncrementMinutes` → `laborStepMinutes`: 2 head lines (base 5)\n",
            "    notes.md:3 Each tariff sets laborIncrementMinutes.\n",
            "    t3.ts:1 export const t3 = { laborIncrementMinutes: 15, count: 1 };\n",
        )),
        "{text}"
    );
    // A plain word replaced everywhere it was edited is not a leftover name.
    assert!(!text.contains("`count` → `total`:"), "{text}");
}

fn owned(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect()
}

#[test]
fn brief_never_pairs_lines_within_a_hunk_or_short_lines_across_hunks() {
    let base = "import { debit, credit, audit, requireAdmin } from './ledger';\n\nexport function transfer(from: string, to: string, amount: number) {\n  debit(from, amount);\n  credit(to, amount);\n  audit('transfer', from, to, amount);\n}\n\nexport function deleteUser(user: string, target: string) {\n  requireAdmin(user);\n  audit('delete', user, target, 0);\n}\n\nexport function helperOne(a: number) {\n  const x = a * 2;\n  const y = x + 1;\n  return y;\n}\n\nexport function helperTwo(b: number) {\n  const z = b * 3;\n  return z;\n}\n\nexport function listUsers(user: string) {\n  audit('list', user, '', 0);\n}\n";
    let head = base
        .replace(
            "  debit(from, amount);\n  credit(to, amount);\n",
            "  credit(to, amount);\n  debit(from, amount);\n",
        )
        .replace(
            "  requireAdmin(user);\n  audit('delete'",
            "  audit('delete'",
        )
        .replace(
            "  audit('list', user, '', 0);\n",
            "  requireAdmin(user);\n  audit('list', user, '', 0);\n",
        );
    let fixture = revisions(&[("bank.ts", base)], &[("bank.ts", &head)]);
    let text = brief(&fixture, &[]);
    for line in [
        "\n-  debit(from, amount);\n",
        "\n+  debit(from, amount);\n",
        "\n-  requireAdmin(user);\n",
        "\n+  requireAdmin(user);\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("moved"), "{text}");
    assert!(!text.contains("hidden"), "{text}");
}

#[test]
fn brief_shows_moved_declarations_whose_lines_change() {
    let tail = "def log_poison(item):\n    print(item)\n\n\ndef handle(item):\n    print(item)\n\n\ndef record(item):\n    print(item)\n";
    let drain = "def drain(queue, limit):\n    for item in queue:\n        if item.poison:\n            log_poison(item)\n            return\n        handle(item)\n        record(item)\n";
    let base = format!("{drain}\n\n{tail}");
    let head = format!("{tail}\n\n{}", drain.replace("            return\n", ""));
    let fixture = revisions(&[("pkg/jobs.py", &base)], &[("pkg/jobs.py", &head)]);
    let text = brief(&fixture, &[]);
    // A removed line changes the declaration's line count, so both sides stay whole.
    for line in [
        "\n-def drain(queue, limit):\n",
        "\n-            return\n",
        "\n+def drain(queue, limit):\n",
        "\n+        handle(item)\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("moved"), "{text}");
}

#[test]
fn brief_operator_rewrites_show_every_site_compactly() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source =
            format!("export function name_{name}(u: any) {{\n  return u.profile.name;\n}}\n");
        head.push((
            format!("{name}.ts"),
            source.replace("u.profile", "u!.profile"),
        ));
        base.push((format!("{name}.ts"), source));
    }
    let guard =
        "export function deleteAll(u: any) {\n  if (isAuthorized(u)) {\n    wipe();\n  }\n}\n";
    base.push(("guard.ts".to_string(), guard.to_string()));
    head.push((
        "guard.ts".to_string(),
        guard.replace("(isAuthorized", "(!isAuthorized"),
    ));
    let fixture = revisions(&owned(&base), &owned(&head));
    let text = brief(&fixture, &[]);
    assert!(text.contains("  R1 insert `!` (4 hunks)\n"), "{text}");
    for site in [
        "@@ -1,3 +1,3 @@ [R1]\n-2:   return u.profile.name;\n+2:   return u!.profile.name;\n",
        "@@ -1,5 +1,5 @@ [R1]\n-2:   if (isAuthorized(u)) {\n+2:   if (!isAuthorized(u)) {\n",
    ] {
        assert!(text.contains(site), "{site} missing in {text}");
    }
    assert!(!text.contains("  rewrite-only hunks:\n"), "{text}");
}

/// Paths of `dir/{a,b}.ext:2` site tokens.
fn site_paths(sites: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for token in sites.split(' ') {
        let (path, lines) = token.rsplit_once(':').unwrap();
        assert!(lines.split(',').all(|line| line.parse::<usize>().is_ok()));
        match path.split_once('{') {
            Some((prefix, rest)) => {
                let (stems, suffix) = rest.split_once('}').unwrap();
                paths.extend(
                    stems
                        .split(',')
                        .map(|stem| format!("{prefix}{stem}{suffix}")),
                );
            }
            None => paths.push(path.to_string()),
        }
    }
    paths
}

#[test]
fn brief_groups_omitted_rule_explained_hunks_with_exact_sites_and_commands() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for index in 0..12 {
        let name =
            format!("export function name_{index}(u: any) {{\n  return u.profile.name;\n}}\n");
        head.push((format!("src/n{index}.ts"), name.replace("u.", "u!.")));
        base.push((format!("src/n{index}.ts"), name));
        let size =
            format!("export function size_{index}(u: any) {{\n  return u.profile.size + 1;\n}}\n");
        head.push((
            format!("lib/k{index}.ts"),
            size.replace("u.", "u!.").replace(" + ", " * "),
        ));
        base.push((format!("lib/k{index}.ts"), size));
    }
    base.push(("guard.ts".into(), "export const limit = 1;\n".into()));
    head.push((
        "guard.ts".into(),
        "export const limit = 1;\nexport function clamp(v: number) {\n  return Math.min(v, limit);\n}\n".into(),
    ));
    let fixture = revisions(&owned(&base), &owned(&head));
    let explained: BTreeSet<String> = base
        .iter()
        .map(|(path, _)| path.clone())
        .filter(|path| path != "guard.ts")
        .collect();
    let (mut counted_form, mut grouped) = (false, false);
    for budget in [600, 700, 1400] {
        let text = brief(&fixture, &["--budget", &budget.to_string()]);
        assert!(tokens(&text) <= budget, "{text}");
        assert!(text.contains("+export function clamp"), "{text}");
        let Some((_, section)) = text.split_once("\nomitted rule-explained hunks (") else {
            continue;
        };
        let (total, rest) = section.split_once(" in ").unwrap();
        let total: usize = total.parse().unwrap();
        let lines: Vec<&str> = rest
            .lines()
            .skip(1)
            .take_while(|line| line.starts_with("  "))
            .collect();
        let shown: BTreeSet<String> = explained
            .iter()
            .filter(|path| text.contains(&format!("\nmodified {path} (1 hunk)\n")))
            .cloned()
            .collect();
        assert_eq!(total + shown.len(), explained.len(), "{text}");
        let groups = lines.iter().filter(|line| !line.starts_with("  git diff "));
        if !text.contains("rule-explained hunks (grouped at the end)") {
            // Counted form: per combination counts and one command.
            let counted: usize = groups
                .map(|line| {
                    let (rules, rest) = line.trim_start().split_once(": ").unwrap();
                    assert!(rules.starts_with('R'), "{line}");
                    rest.split_once(' ').unwrap().0.parse::<usize>().unwrap()
                })
                .sum();
            assert_eq!(counted, total, "{text}");
            assert!(lines.last().unwrap().starts_with("  git diff "), "{text}");
            counted_form = true;
            continue;
        }
        let mut counted = 0;
        let mut sites = Vec::new();
        for line in groups {
            let (rules, rest) = line.trim_start().split_once(" (").unwrap();
            assert!(rules.starts_with('R'), "{line}");
            let (count, rest) = rest.split_once(' ').unwrap();
            let count: usize = count.parse().unwrap();
            let paths = site_paths(rest.split_once("): ").unwrap().1);
            assert_eq!(paths.len(), count, "{line}");
            counted += count;
            sites.extend(paths);
        }
        assert_eq!(counted, total, "{text}");
        let unique: BTreeSet<String> = sites.iter().cloned().collect();
        assert_eq!(unique.len(), sites.len(), "{text}");
        assert!(unique.is_disjoint(&shown), "{text}");
        assert_eq!(unique.len() + shown.len(), explained.len(), "{text}");
        let commands: Vec<&&str> = lines
            .iter()
            .filter(|line| line.starts_with("  git diff "))
            .collect();
        assert_eq!(commands.len(), unique.len().div_ceil(10), "{text}");
        for path in &unique {
            let spec = format!("':(top,literal){path}'");
            assert_eq!(
                commands.iter().filter(|line| line.contains(&spec)).count(),
                1,
                "{path} in {text}"
            );
            assert!(!text.contains(&format!("\n  {path} @@ ")), "{text}");
        }
        grouped = true;
    }
    assert!(counted_form && grouped, "both summary forms expected");
}

#[test]
fn brief_rewrites_never_fold_their_reversal_and_sites_name_the_changed_line() {
    let config = |name: &str, field: &str, value: &str| {
        format!(
            "export const {name} = {{\n  name: '{name}',\n  {field}: {value},\n  depth: 3,\n}};\n"
        )
    };
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        base.push((format!("{name}.ts"), config(name, "retry", "retryOnce")));
        head.push((format!("{name}.ts"), config(name, "retry", "failFast")));
    }
    base.push((
        "tls.ts".to_string(),
        config("tls", "verifyPeer", "failFast"),
    ));
    head.push((
        "tls.ts".to_string(),
        config("tls", "verifyPeer", "retryOnce"),
    ));
    let fixture = revisions(&owned(&base), &owned(&head));
    let text = brief(&fixture, &[]);
    assert!(
        text.contains("  R1 `retryOnce` → `failFast` (3 hunks)\n"),
        "{text}"
    );
    // `failFast` already occurs in base, so no site folds; each shows its changed line.
    assert!(
        text.contains("@@ -1,5 +1,5 @@ [R1]\n-3:   retry: retryOnce,\n+3:   retry: failFast,\n"),
        "{text}"
    );
    assert!(!text.contains("  rewrite-only hunks:"), "{text}");
    assert!(
        text.contains("\n-  verifyPeer: failFast,\n+  verifyPeer: retryOnce,\n"),
        "{text}"
    );
}

/// An import of `name` from the relative module `charge`.
fn charge_import(extension: &str, name: &str) -> String {
    if extension == "py" {
        format!("from .charge import {name}\n")
    } else {
        format!("import {{ {name} }} from './charge';\n")
    }
}

/// Callers of a renamed function; `billing` also changes the indentation of its call.
fn charge_callers(extension: &str, call: &str, moved_call: &str) -> Fixture {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = format!(
            "{}first({name});\nsecond({name});\n{call}(account, 10);\nthird({name});\n",
            charge_import(extension, call)
        );
        head.push((
            format!("{name}.{extension}"),
            source
                .replace("old_charge", "new_charge")
                .replace("oldCharge", "newCharge"),
        ));
        base.push((format!("{name}.{extension}"), source));
    }
    let billing = |call: &str, indent: &str| {
        format!(
            "{}if approved:\n    notify(account)\n{indent}{call}(account, 100)\nreturn account\n",
            charge_import(extension, call)
        )
    };
    base.push((format!("billing.{extension}"), billing(call, "    ")));
    head.push((format!("billing.{extension}"), billing(moved_call, "")));
    revisions(&owned(&base), &owned(&head))
}

#[test]
fn brief_shows_reindented_lines_in_indentation_sensitive_languages() {
    // A changed indentation shows the whole hunk with its context and without rewrites.
    let python = brief(&charge_callers("py", "old_charge", "new_charge"), &[]);
    assert!(
        python.contains(concat!(
            "modified billing.py (1 hunk)\n",
            "@@ -1,5 +1,5 @@\n",
            "-from .charge import old_charge\n",
            "+from .charge import new_charge\n",
            " if approved:\n",
            "     notify(account)\n",
            "-    old_charge(account, 100)\n",
            "+new_charge(account, 100)\n",
            " return account\n",
        )),
        "{python}"
    );
    assert!(python.contains("    R1: {a,b,c}.py:1\n"), "{python}");
    // Elsewhere the dedent leaves the hunk compact, never rewrite-only.
    let typescript = brief(&charge_callers("ts", "oldCharge", "newCharge"), &[]);
    assert!(
        typescript.contains("    R1: {a,b,c}.ts:1\n"),
        "{typescript}"
    );
    assert!(
        typescript.contains(concat!(
            "@@ -1,5 +1,5 @@ [R1]\n",
            "-1: import { oldCharge } from './charge';\n",
            "+1: import { newCharge } from './charge';\n",
            "-4:     oldCharge(account, 100)\n",
            "+4: newCharge(account, 100)\n",
        )),
        "{typescript}"
    );
}

#[test]
fn brief_lists_renames_and_mode_changes_without_shown_hunks() {
    use std::os::unix::fs::PermissionsExt as _;
    let source = |name: &str, access: &str| {
        format!(
            "import {{ {access} }} from './user';\nexport function name_{name}() {{\n  const p = {access};\n  return p.name;\n}}\nexport const tag_{name} = '{name}';\n"
        )
    };
    let base: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|name| (format!("src/{name}.ts"), source(name, "profile")))
        .collect();
    let head: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|name| (format!("src/{name}.ts"), source(name, "account")))
        .collect();
    let mut fixture = revisions(&owned(&base), &owned(&head));
    fs::create_dir_all(fixture.path().join("lib")).unwrap();
    fs::rename(
        fixture.path().join("src/a.ts"),
        fixture.path().join("lib/a.ts"),
    )
    .unwrap();
    fs::set_permissions(
        fixture.path().join("src/b.ts"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fixture.head = commit(fixture.path());
    let text = brief(&fixture, &[]);
    assert!(
        text.contains("renamed or mode changed without a shown hunk: src/a.ts -> lib/a.ts (1 rewrite-only), src/b.ts (mode 100644 -> 100755) (1 rewrite-only)\n"),
        "{text}"
    );
}

/// Two binary files and a dozen files with six changed functions each.
fn binaries_and_hunks() -> Fixture {
    let module = |index: usize, changed: bool| {
        let mut source = String::new();
        for function in 0..30 {
            let body = if changed && function % 5 == 0 {
                format!("  const y = x * {function} - {index};\n  return y;\n")
            } else {
                format!("  return x + {function};\n")
            };
            writeln!(
                source,
                "export function f{index}_{function}(x: number) {{\n{body}}}\n"
            )
            .unwrap();
        }
        source
    };
    let mut base = vec![
        ("assets/data.bin".to_string(), "\u{0}\u{1}data".to_string()),
        ("assets/logo.png".to_string(), "\u{0}\u{2}logo".to_string()),
    ];
    let mut head = vec![
        ("assets/data.bin".to_string(), "\u{0}\u{1}data2".to_string()),
        ("assets/logo.png".to_string(), "\u{0}\u{2}logo2".to_string()),
    ];
    for index in 0..12 {
        base.push((format!("src/m{index}.ts"), module(index, false)));
        head.push((format!("src/m{index}.ts"), module(index, true)));
    }
    revisions(&owned(&base), &owned(&head))
}

/// Change entry headings and their shown hunk headers.
type Shown = (BTreeSet<String>, BTreeSet<String>);

/// Shown items of a rendered brief.
fn shown_items(text: &str) -> Shown {
    let (mut entries, mut hunks) = (BTreeSet::new(), BTreeSet::new());
    let mut entry = None;
    for line in text.lines() {
        if let Some(heading) = ["modified ", "added ", "deleted ", "renamed "]
            .iter()
            .any(|status| line.starts_with(status))
            .then(|| line.split_once(" ("))
            .filter(|_| !line.starts_with("renamed or mode changed"))
            .flatten()
            .map(|(heading, _)| heading)
        {
            entries.insert(heading.to_string());
            entry = Some(heading.to_string());
        } else if line.starts_with("@@ ")
            && let Some(entry) = &entry
        {
            let header = line.splitn(3, "@@").take(2).collect::<Vec<_>>().concat();
            hunks.insert(format!("{entry} {header}"));
        }
    }
    (entries, hunks)
}

/// Detail of the omitted-hunk sections: other hunks counted with one full diff, counted with a
/// command, listed or none omitted; rule-explained hunks counted, grouped or none omitted.
fn omission_detail(text: &str) -> (u8, u8) {
    let summary = text
        .lines()
        .find_map(|line| line.strip_prefix("omitted from output: "))
        .unwrap_or("");
    let omitted = |kind: &str| {
        summary.split(", ").any(|part| {
            part.split_once(' ')
                .is_some_and(|(count, rest)| count.parse::<usize>().is_ok() && rest == kind)
        })
    };
    let hunks = if omitted("hunks (listed at the end)") {
        2
    } else if !omitted("hunks (counted at the end)") {
        3
    } else {
        u8::from(!text.contains("\nfull diff: "))
    };
    let explained = if omitted("rule-explained hunks (grouped at the end)") {
        1
    } else if omitted("rule-explained hunks (counted at the end)") || hunks == 0 {
        0
    } else {
        2
    };
    (hunks, explained)
}

#[test]
fn brief_budgets_from_256_render_and_never_show_less_at_a_larger_budget() {
    let fixture = binaries_and_hunks();
    let mut previous: Option<(Shown, (u8, u8))> = None;
    let mut shown = Vec::new();
    for budget in [256, 300, 400, 500, 700, 1000, 1500, 2500, 4000] {
        let text = brief(&fixture, &["--budget", &budget.to_string()]);
        assert!(tokens(&text) <= budget, "{budget}: {text}");
        let (entries, hunks) = shown_items(&text);
        let detail = omission_detail(&text);
        if budget >= 500 {
            // Changes without hunks stay visible by path.
            for path in ["assets/data.bin", "assets/logo.png"] {
                assert!(text.contains(path), "{budget}: {path} in {text}");
            }
        }
        // A larger budget keeps every entry and shown hunk of a smaller one and never reduces
        // the detail of the omitted-hunk sections.
        if let Some(((entries_before, hunks_before), detail_before)) = &previous {
            assert!(entries.is_superset(entries_before), "{budget}: {text}");
            assert!(hunks.is_superset(hunks_before), "{budget}: {text}");
            assert!(
                detail.0 >= detail_before.0 && detail.1 >= detail_before.1,
                "{budget}: {detail:?} after {detail_before:?}: {text}"
            );
        }
        shown.push(hunks.len());
        if text.contains("full diff:") {
            let command = format!(
                "full diff: git diff --no-indent-heuristic {} {}\n",
                short(fixture.base),
                short(fixture.head)
            );
            assert!(text.ends_with(&command), "{budget}: {text}");
        }
        previous = Some(((entries, hunks), detail));
    }
    assert!(shown.first() < shown.last(), "{shown:?}");
    let smallest = brief(&fixture, &["--budget", "256"]);
    assert!(smallest.contains("full diff:"), "{smallest}");
}

#[test]
fn brief_lists_at_most_forty_omitted_change_entries() {
    let files = |suffix: &str| -> Vec<(String, String)> {
        (0..100)
            .map(|index| {
                (
                    format!("assets/b{index:02}.bin"),
                    format!("\u{0}{index}{suffix}"),
                )
            })
            .collect()
    };
    let fixture = revisions(&owned(&files("")), &owned(&files("x")));
    let text = brief(&fixture, &["--budget", "1500"]);
    let omitted: usize = text
        .lines()
        .find_map(|line| line.strip_prefix("omitted from output: "))
        .and_then(|rest| {
            rest.split(", ")
                .find_map(|part| part.strip_suffix(" change entries"))
        })
        .unwrap_or_else(|| panic!("{text}"))
        .parse()
        .unwrap();
    assert!(omitted > 40, "{text}");
    let listed = text
        .lines()
        .find_map(|line| line.strip_prefix("omitted change entries without hunks: "))
        .unwrap_or_else(|| panic!("{text}"));
    let items: Vec<_> = listed.split(", ").collect();
    assert_eq!(items.len(), 41, "{text}");
    assert_eq!(items[40], format!("+{} more", omitted - 40));
    assert!(items[0].ends_with(".bin (modified)"), "{text}");
}

#[test]
fn brief_commands_use_the_merge_base_in_merge_base_mode() {
    let mut fixture = revisions(
        &[("many.ts", &many_hunks(40, false))],
        &[("many.ts", &many_hunks(40, true))],
    );
    let merge_base = fixture.base;
    let repository = Repository::open(fixture.path()).unwrap();
    let base_commit = repository.find_commit(merge_base).unwrap();
    let signature = Signature::now("RepoScout tests", "tests@example.com").unwrap();
    let side = repository
        .commit(
            None,
            &signature,
            &signature,
            "side",
            &base_commit.tree().unwrap(),
            &[&base_commit],
        )
        .unwrap();
    fixture.base = side;
    let text = brief(&fixture, &["--merge-base", "--budget", "600"]);
    assert!(
        text.starts_with(&format!(
            "review-context brief (merge-base): base {} head {} (requested base {})",
            short(merge_base),
            short(fixture.head),
            short(side)
        )),
        "{text}"
    );
    assert!(
        text.ends_with(&format!(
            "  git diff --no-indent-heuristic {} {} -- ':(top,literal)many.ts'\n",
            short(merge_base),
            short(fixture.head)
        )),
        "{text}"
    );
}

#[test]
fn brief_tight_budgets_reduce_the_omission_list_to_one_line() {
    let files = |offset: usize| -> Vec<(String, String)> {
        (0..60)
            .map(|index| {
                (
                    format!("src/f{index}.ts"),
                    format!(
                        "export function f{index}() {{ return {}; }}\n",
                        index + offset
                    ),
                )
            })
            .collect()
    };
    let fixture = revisions(&owned(&files(0)), &owned(&files(100)));
    let text = brief(&fixture, &["--budget", "1500"]);
    let omitted = 60 - text.lines().filter(|line| line.starts_with("@@ ")).count();
    assert!(omitted < 60, "{text}");
    assert!(
        text.contains(&format!(
            "\nomitted hunks: {omitted} in {omitted} files: git diff --no-indent-heuristic {} {}\n",
            short(fixture.base),
            short(fixture.head)
        )),
        "{text}"
    );
    assert!(!text.contains("full diff:"), "{text}");
}

#[test]
fn brief_literal_rewrites_show_every_site() {
    let config = |name: &str| {
        format!(
            "export const config_{name} = {{\n  name: '{name}',\n  debug: true,\n  level: 2,\n}};\n"
        )
    };
    let mut base: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|name| (format!("src/{name}.ts"), config(name)))
        .collect();
    let mut head: Vec<_> = base
        .iter()
        .map(|(path, source)| (path.clone(), source.replace("true", "false")))
        .collect();
    let tls = "export function open(host: string) {\n  return connect(host, {\n    rejectUnauthorized: true,\n    servername: host,\n  });\n}\n";
    let http =
        "export function client() {\n  return {\n    verifyPeer: true,\n    retries: 1,\n  };\n}\n";
    base.push(("src/tls.ts".into(), tls.into()));
    head.push(("src/tls.ts".into(), tls.replace("true", "false")));
    base.push(("src/http.ts".into(), http.into()));
    head.push((
        "src/http.ts".into(),
        http.replace("true,", "false,\n    timeoutMs: 5000,"),
    ));
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(text.contains("  R1 `true` → `false` (4 hunks)\n"), "{text}");
    for site in [
        "@@ -1,5 +1,5 @@ [R1]\n-3:   debug: true,\n+3:   debug: false,\n",
        "@@ -1,6 +1,6 @@ [R1]\n-3:     rejectUnauthorized: true,\n+3:     rejectUnauthorized: false,\n",
        "@@ -1,6 +1,7 @@ [deviates from R1]\n export function client() {\n",
        "\n+    verifyPeer: false,\n+    timeoutMs: 5000,\n",
    ] {
        assert!(text.contains(site), "{site} missing in {text}");
    }
    assert!(!text.contains("  rewrite-only hunks:\n"), "{text}");
}

#[test]
fn brief_renames_never_explain_the_same_text_inside_strings() {
    let caller = |name: &str| {
        format!(
            "import {{ roundUp }} from './round.ts';\n\nexport const total_{name} = roundUp({name}.length);\n"
        )
    };
    let mut base: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|name| (format!("src/{name}.ts"), caller(name)))
        .collect();
    base.push((
        "src/log.ts".into(),
        "export function trace() {\n  log('roundUp');\n}\n".into(),
    ));
    let head: Vec<_> = base
        .iter()
        .map(|(path, source)| (path.clone(), source.replace("roundUp", "quantize")))
        .collect();
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(
        text.contains("  R1 `roundUp` → `quantize` (3 hunks)\n"),
        "{text}"
    );
    assert!(text.contains("    R1: src/{a,b,c}.ts:1\n"), "{text}");
    assert!(
        text.contains("[deviates from R1]\n-  log('roundUp');\n+  log('quantize');\n"),
        "{text}"
    );
}

#[test]
fn brief_shows_python_dedents_next_to_inserted_blank_lines() {
    let caller = |name: &str| {
        format!(
            "from pkg.util import old_send\n\n\ndef run_{name}(x):\n    old_send(x)\n    return x\n"
        )
    };
    let mut base: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|name| (format!("pkg/{name}.py"), caller(name)))
        .collect();
    base.push((
        "pkg/billing.py".into(),
        "from pkg.util import old_audit, old_charge\n\n\ndef settle(account, big):\n    if big:\n        old_audit(account)\n        old_charge(account, 100)\n    return account\n".into(),
    ));
    let mut head: Vec<_> = base
        .iter()
        .map(|(path, source)| (path.clone(), source.replace("old_", "new_")))
        .collect();
    head[3].1 = head[3].1.replace(
        "        new_charge(account, 100)\n",
        "\n    new_charge(account, 100)\n",
    );
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(text.contains("modified pkg/billing.py"), "{text}");
    assert!(text.contains("\n+    new_charge(account, 100)\n"), "{text}");
}

/// `src/a.ts` loses a declaration that `src/b.ts` gains as `moved`.
fn moved_declaration(declaration: &str, moved: &str) -> Fixture {
    let keep = "export function keep() {\n  return 1;\n}\n";
    revisions(
        &[("src/a.ts", &format!("{keep}\n{declaration}"))],
        &[
            ("src/a.ts", keep),
            (
                "src/b.ts",
                &format!("import {{ Req }} from './req';\n\n{moved}"),
            ),
        ],
    )
}

#[test]
fn brief_shows_declarations_moved_with_new_comment_markers_or_braces() {
    let enforce = "export function enforce(req: Req) {\n  checkToken(req.token);\n  checkScope(req.scope, 'admin');\n  recordAccess(req.user, req.path);\n}\n";
    let commented = enforce
        .replace("{\n  checkToken", "{\n/*\n  checkToken")
        .replace("path);\n}", "path);\n*/\n}");
    let text = brief(&moved_declaration(enforce, &commented), &[]);
    for line in ["\n+/*\n", "\n+  checkToken(req.token);\n", "\n+*/\n"] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("moved"), "{text}");
    let handle = "export function handle(req: Req) {\n  if (!allowed(req)) {\n    deny(req, 403);\n    return logDenied(req);\n  }\n  serve(req);\n}\n";
    let nested = handle.replace("  }\n  serve(req);\n", "  serve(req);\n  }\n");
    let text = brief(&moved_declaration(handle, &nested), &[]);
    assert!(text.contains("\n+  serve(req);\n+  }\n"), "{text}");
    assert!(!text.contains("moved"), "{text}");
    // The unchanged declaration still moves, closing brace included.
    let text = brief(&moved_declaration(handle, handle), &[]);
    assert!(text.contains("  src/a.ts:5-11 → src/b.ts:3-9\n"), "{text}");
    // A modified file whose hunks all moved keeps an entry naming the destination.
    assert!(
        text.contains(concat!(
            "modified src/a.ts (1 hunk; 1 moved)\n",
            "  function handle 5-11 (base only)\n",
            "  @@ -1,11 +1,3 @@ [8 lines hidden: moved to src/b.ts:3-9]\n",
        )),
        "{text}"
    );
}

#[test]
fn brief_shows_statements_moved_within_a_declaration_or_into_tests() {
    let transfer = "export function transfer(from: Account, to: Account, amount: number) {\n  requireOwner(from, session.user);\n  requireLimit(from, amount);\n  requireNotFrozen(from, to);\n  const tx = begin();\n  debit(from, amount);\n  credit(to, amount);\n  tx.commit();\n  return tx;\n}\n";
    let guards = "  requireOwner(from, session.user);\n  requireLimit(from, amount);\n  requireNotFrozen(from, to);\n";
    let relocated = transfer
        .replace(guards, "")
        .replace("  tx.commit();\n", &format!("  tx.commit();\n{guards}"));
    let text = brief(
        &revisions(&[("src/bank.ts", transfer)], &[("src/bank.ts", &relocated)]),
        &[],
    );
    for line in [
        "\n-  requireOwner(from, session.user);\n",
        "\n+  requireOwner(from, session.user);\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("moved"), "{text}");
    let check = "export function check(user: User, id: string) {\n  requireSession(user);\n  requireRole(user, 'admin');\n  requireNotLocked(user, id);\n}\n";
    let spec = "import { remove } from '../src/users';\n\ntest('removes', () => {\n  expect(remove('x')).toBe(true);\n});\n";
    let users = "export function remove(id: string) {\n  return true;\n}\n";
    let text = brief(
        &revisions(
            &[
                ("src/users.ts", &format!("{users}\n{check}")),
                ("tests/users.test.ts", spec),
            ],
            &[
                ("src/users.ts", users),
                ("tests/users.test.ts", &format!("{spec}\n{check}")),
            ],
        ),
        &[],
    );
    for line in [
        "\n-  requireSession(user);\n",
        "\n+  requireSession(user);\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("moved"), "{text}");
}

#[test]
fn brief_lists_deleted_files_whose_hunks_moved_with_their_destination() {
    let settle = "export function settle(account: Account) {\n  const due = balance(account);\n  charge(account, due);\n  notify(account, due);\n  return due;\n}\n";
    let lib = "export function other(x: number) {\n  return x * 2;\n}\n";
    let mut fixture = revisions(
        &[("src/lib.ts", lib), ("src/old.ts", settle)],
        &[("src/lib.ts", &format!("{lib}\n{settle}"))],
    );
    fs::remove_file(fixture.path().join("src/old.ts")).unwrap();
    fixture.head = commit(fixture.path());
    let text = brief(&fixture, &[]);
    assert!(
        text.contains(concat!(
            "deleted src/old.ts (1 hunk; 1 moved)\n",
            "  function settle 1-6\n",
            "  @@ -1,6 +0,0 @@ [6 lines hidden: moved to src/lib.ts:5-10]\n",
        )),
        "{text}"
    );
}

#[test]
fn brief_commands_name_paths_literally_from_the_repository_root() {
    let paths = [
        "-dash.ts",
        "!bang.ts",
        "__init__.ts",
        "app/[id]/page.ts",
        "q'uote.ts",
    ];
    // Every file changes two distinct values, so no edit repeats as a rewrite.
    let source = |file: usize, offset: usize| -> String {
        (0..30).fold(String::new(), |mut text, line| {
            let value = if line % 15 == 0 {
                1000 * file + line + offset
            } else {
                line
            };
            let _ = writeln!(text, "export const v{line} = {value};");
            text
        })
    };
    let side = |offset: usize| -> Vec<(String, String)> {
        paths
            .iter()
            .enumerate()
            .map(|(file, path)| (path.to_string(), source(file, offset)))
            .collect()
    };
    let fixture = revisions(&owned(&side(0)), &owned(&side(5)));
    let commands: Vec<String> = [310, 400]
        .iter()
        .flat_map(|budget| {
            let text = brief(&fixture, &["--budget", &budget.to_string()]);
            text.lines()
                .filter(|line| line.contains(" -- "))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_ne!(commands, Vec::<String>::new());
    for command in &commands {
        let specs = command.split_once(" -- ").unwrap().1;
        assert!(
            specs.starts_with("':(top,literal)") && !specs.contains(":/"),
            "{command}"
        );
    }
    let all = commands.join("\n");
    for spec in [
        "':(top,literal)-dash.ts'",
        "':(top,literal)__init__.ts'",
        "':(top,literal)app/[id]/page.ts'",
        "':(top,literal)q'\\''uote.ts'",
    ] {
        assert!(all.contains(spec), "{spec} missing in {all}");
    }
}

#[test]
fn brief_keyword_edits_never_join_identifier_part_renames() {
    let perms = |op: &str| {
        format!(
            "def can_publish(user, doc):\n    if user.is_editor {op} doc.owner_id == user.id:\n        return True\n    return False\n"
        )
    };
    let cache = |op: &str| {
        format!(
            "from pkg.store import load\n\n\ndef fetch_{op}_cache(key):\n    return load(key)\n"
        )
    };
    let views = |op: &str| {
        format!(
            "from pkg.cache import fetch_{op}_cache\n\n\ndef page(key):\n    return fetch_{op}_cache(key)\n"
        )
    };
    let side = |op: &str| {
        vec![
            ("pkg/perms.py".to_string(), perms(op)),
            ("pkg/cache.py".to_string(), cache(op)),
            ("pkg/views.py".to_string(), views(op)),
        ]
    };
    let mut base = side("and");
    base.push(("pkg/store.py".into(), "def load(k):\n    return k\n".into()));
    let text = brief(&revisions(&owned(&base), &owned(&side("or"))), &[]);
    assert!(
        text.contains(concat!(
            "\n-    if user.is_editor and doc.owner_id == user.id:\n",
            "+    if user.is_editor or doc.owner_id == user.id:\n",
        )),
        "{text}"
    );
    assert!(text.contains("\n+def fetch_or_cache(key):\n"), "{text}");
    assert!(!text.contains("rewrites ("), "{text}");
}

#[test]
fn brief_literal_edits_in_code_and_strings_show_every_site() {
    let tls = |value: &str| {
        format!(
            "import {{ connect }} from './net';\n\nexport function open(host: string) {{\n  return connect(host, {{\n    rejectUnauthorized: {value},\n    servername: host,\n  }});\n}}\n"
        )
    };
    let cookie = |value: &str| {
        format!(
            "export const cookieOptions = {{\n  path: '/',\n  httpOnly: {value},\n  maxAge: 3600,\n}};\n"
        )
    };
    let spec = |value: &str| {
        format!(
            "import {{ isAdmin }} from '../src/admin';\n\nit('returns {value} for guests', () => {{\n  expect(isAdmin(guest)).toBe(false);\n}});\n"
        )
    };
    let side = |value: &str| {
        vec![
            ("src/tls.ts".to_string(), tls(value)),
            ("src/cookie.ts".to_string(), cookie(value)),
            ("tests/admin.test.ts".to_string(), spec(value)),
        ]
    };
    let mut base = side("true");
    base.push((
        "src/net.ts".into(),
        "export function connect(host: string, options: object) {\n  return { host, options };\n}\n".into(),
    ));
    base.push((
        "src/admin.ts".into(),
        "export function isAdmin(u: { admin: boolean }) {\n  return u.admin;\n}\n".into(),
    ));
    let text = brief(&revisions(&owned(&base), &owned(&side("false"))), &[]);
    assert!(
        text.contains("  R1 `true` → `false`, also inside literals (3 hunks)\n"),
        "{text}"
    );
    for site in [
        "[R1]\n-5:     rejectUnauthorized: true,\n+5:     rejectUnauthorized: false,\n",
        "[R1]\n-3:   httpOnly: true,\n+3:   httpOnly: false,\n",
        "[R1]\n-3: it('returns true for guests', () => {\n+3: it('returns false for guests', () => {\n",
    ] {
        assert!(text.contains(site), "{site} missing in {text}");
    }
    assert!(!text.contains("  rewrite-only hunks:\n"), "{text}");
}

#[test]
fn brief_string_edits_never_join_identifier_part_renames() {
    let session = |value: &str| {
        format!(
            "export function sessionCookie(id: string) {{\n  return {{\n    value: id,\n    sameSite: '{value}',\n    secure: true,\n  }};\n}}\n"
        )
    };
    let csrf = |value: &str| {
        format!("export const csrfPolicy = {{\n  origin: 'self',\n  mode: '{value}',\n}};\n")
    };
    let flags = |value: &str| {
        format!(
            "export const {value}Cookies = true;\n\nexport function cookiesFlag() {{\n  return {value}Cookies;\n}}\n"
        )
    };
    let side = |value: &str| {
        vec![
            ("src/session.ts".to_string(), session(value)),
            ("src/csrf.ts".to_string(), csrf(value)),
            ("src/flags.ts".to_string(), flags(value)),
        ]
    };
    let text = brief(
        &revisions(&owned(&side("strict")), &owned(&side("lax"))),
        &[],
    );
    for line in [
        "\n+    sameSite: 'lax',\n",
        "\n+  mode: 'lax',\n",
        "\n+export const laxCookies = true;\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("rewrites ("), "{text}");
}

#[test]
fn brief_number_edits_show_every_site_compactly() {
    let side = |value: &str| {
        vec![
            (
                "src/hash.ts".to_string(),
                format!(
                    "export const hashing = {{\n  algorithm: 'bcrypt',\n  saltRounds: {value},\n}};\n"
                ),
            ),
            (
                "src/password.ts".to_string(),
                format!(
                    "export const passwordPolicy = {{\n  minLength: {value},\n  requireDigit: true,\n}};\n"
                ),
            ),
            (
                "src/runtime.ts".to_string(),
                format!("export const runtime = {{\n  image: 'node{value}',\n}};\n"),
            ),
        ]
    };
    let text = brief(&revisions(&owned(&side("12")), &owned(&side("4"))), &[]);
    assert!(
        text.contains("  R1 `12` → `4`, also inside literals (3 hunks)\n"),
        "{text}"
    );
    for site in [
        "[R1]\n-3:   saltRounds: 12,\n+3:   saltRounds: 4,\n",
        "[R1]\n-2:   minLength: 12,\n+2:   minLength: 4,\n",
        "[R1]\n-2:   image: 'node12',\n+2:   image: 'node4',\n",
    ] {
        assert!(text.contains(site), "{site} missing in {text}");
    }
    assert!(!text.contains("  rewrite-only hunks:\n"), "{text}");
}

#[test]
fn brief_keyword_rewrites_show_every_return_site() {
    let side = |value: &str| {
        let mut files: Vec<_> = ["Beta", "Dark", "Cache"]
            .iter()
            .map(|name| {
                (
                    format!("src/flag{name}.ts"),
                    format!(
                        "export function is{name}Enabled() {{\n  log('{name}');\n  return {value};\n}}\n"
                    ),
                )
            })
            .collect();
        files.push((
            "src/perm.ts".to_string(),
            format!(
                "export function canDelete(user: User, doc: Doc) {{\n  if (user.id === doc.owner) {{\n    return true;\n  }}\n  return {value};\n}}\n"
            ),
        ));
        files
    };
    let text = brief(
        &revisions(&owned(&side("false")), &owned(&side("true"))),
        &[],
    );
    assert!(text.contains("  R1 `false` → `true` (4 hunks)\n"), "{text}");
    assert!(
        text.contains(concat!(
            "modified src/perm.ts (1 hunk)\n",
            "  function canDelete 1-6\n",
            "@@ -2,5 +2,5 @@ [R1]\n",
            "-5:   return false;\n",
            "+5:   return true;\n",
        )),
        "{text}"
    );
    assert!(!text.contains("  rewrite-only hunks:\n"), "{text}");
}

#[test]
fn brief_negations_show_every_site_compactly() {
    let side = |negated: bool| {
        let mut files = Vec::new();
        for (name, python, typescript) in [
            ("beta", "is_beta", "isBeta"),
            ("trial", "is_trial", "isTrial"),
            ("legacy", "is_legacy", "isLegacy"),
            ("guard", "is_suspended", "isSuspended"),
        ] {
            let (not, bang) = if negated { ("not ", "!") } else { ("", "") };
            files.push((
                format!("pkg/{name}.py"),
                format!(
                    "def {name}(user):\n    log('{name}')\n    if {not}user.{python}:\n        return deny()\n    return allow()\n"
                ),
            ));
            files.push((
                format!("src/{name}.ts"),
                format!(
                    "export function {name}(user: User) {{\n  log('{name}');\n  if ({bang}user.{typescript}) {{\n    return deny();\n  }}\n  return allow();\n}}\n"
                ),
            ));
        }
        files
    };
    let text = brief(&revisions(&owned(&side(false)), &owned(&side(true))), &[]);
    assert!(
        text.contains("  R1 insert `!` (4 hunks)\n  R2 insert `not` (4 hunks)\n"),
        "{text}"
    );
    assert!(
        text.contains("[R2]\n-3:     if user.is_suspended:\n+3:     if not user.is_suspended:\n"),
        "{text}"
    );
    assert!(
        text.contains("[R1]\n-3:   if (user.isSuspended) {\n+3:   if (!user.isSuspended) {\n"),
        "{text}"
    );
    assert_eq!(text.matches(" [R1]\n").count(), 4, "{text}");
    assert_eq!(text.matches(" [R2]\n").count(), 4, "{text}");
    assert!(!text.contains("  rewrite-only hunks:\n"), "{text}");
}

#[test]
fn brief_never_moves_declarations_from_tests_into_production() {
    let helper = "export function grantAll(user: User) {\n  user.roles = ['admin'];\n  user.mfaVerified = true;\n  return user;\n}\n";
    let login = "export function login(user: User) {\n  return user.mfaVerified && user.roles.length > 0;\n}\n";
    let case = "\ntest('login', () => {\n  expect(login(grantAll(makeUser()))).toBe(true);\n});\n";
    let text = brief(
        &revisions(
            &[
                (
                    "tests/session.test.ts",
                    &format!("import {{ login }} from '../src/session';\n\n{helper}{case}"),
                ),
                ("src/session.ts", login),
            ],
            &[
                (
                    "tests/session.test.ts",
                    &format!("import {{ login, grantAll }} from '../src/session';\n{case}"),
                ),
                ("src/session.ts", &format!("{login}\n{helper}")),
            ],
        ),
        &[],
    );
    for line in [
        "\n-export function grantAll(user: User) {\n",
        "\n-  user.mfaVerified = true;\n",
        "\n+export function grantAll(user: User) {\n",
        "\n+  user.mfaVerified = true;\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("hidden: moved"), "{text}");
}

#[test]
fn brief_never_moves_python_declarations_with_changed_indentation() {
    let keep = "def keep():\n    return 1\n";
    let text = brief(
        &revisions(
            &[
                (
                    "pkg/old.py",
                    &format!(
                        "{keep}\n\ndef settle(account, big):\n    if big:\n        charge(account)\n        audit(account)\n    notify(account)\n"
                    ),
                ),
                (
                    "pkg/util.py",
                    "def charge(a): pass\ndef audit(a): pass\ndef notify(a): pass\n",
                ),
            ],
            &[
                ("pkg/old.py", keep),
                (
                    "pkg/new.py",
                    "from pkg.util import charge, audit, notify\n\n\ndef settle(account, big):\n    if big:\n        charge(account)\n    audit(account)\n    notify(account)\n",
                ),
            ],
        ),
        &[],
    );
    for line in [
        "\n-        audit(account)\n",
        "\n+        charge(account)\n+    audit(account)\n",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    assert!(!text.contains("hidden: moved"), "{text}");
}

/// `src/{a,b,c}.ts` render `<old>` in base and `<new>` in head, imported from `./state`, plus
/// `extra` files.
fn global_swap(old: &str, new: &str, extra: &[(&str, &str, &str)]) -> Fixture {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = |field: &str| {
            format!(
                "import {{ {field} }} from './state';\nexport function show_{name}() {{\n  return render({field});\n}}\n"
            )
        };
        base.push((format!("src/{name}.ts"), source(old)));
        head.push((format!("src/{name}.ts"), source(new)));
    }
    for (path, before, after) in extra {
        base.push(((*path).to_string(), (*before).to_string()));
        head.push(((*path).to_string(), (*after).to_string()));
    }
    revisions(&owned(&base), &owned(&head))
}

#[test]
fn brief_renames_onto_a_name_already_in_base_never_fold() {
    let pure = brief(&global_swap("user", "member", &[]), &[]);
    assert!(pure.contains("    R1: src/{a,b,c}.ts:1\n"), "{pure}");
    // `account` already names another value in base, so every site shows its changed line.
    let pay = (
        "src/pay.ts",
        "export function pay(amount: number) {\n  debit(account, amount);\n  credit(user, amount);\n}\n",
        "export function pay(amount: number) {\n  debit(account, amount);\n  credit(account, amount);\n}\n",
    );
    let swapped = brief(&global_swap("user", "account", &[pay]), &[]);
    assert!(!swapped.contains("  rewrite-only hunks:"), "{swapped}");
    for path in ["a", "b", "c"] {
        assert!(
            swapped.contains(&format!(
                "modified src/{path}.ts (1 hunk)\n  function show_{path} 2-4\n@@ -1,4 +1,4 @@ [R1]\n-1: import {{ user }} from './state';\n+1: import {{ account }} from './state';\n-3:   return render(user);\n+3:   return render(account);\n"
            )),
            "{swapped}"
        );
    }
    assert!(
        swapped.contains("-3:   credit(user, amount);\n+3:   credit(account, amount);\n"),
        "{swapped}"
    );
}

#[test]
fn brief_part_renames_onto_an_existing_function_show_the_site() {
    let caller = |name: &str, call: &str| {
        format!(
            "import {{ {call} }} from './users';\nexport function {name}(id: string) {{\n  const found = {call}(id);\n  return found;\n}}\n"
        )
    };
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        base.push((format!("src/{name}.ts"), caller(name, "loadUser")));
        head.push((format!("src/{name}.ts"), caller(name, "loadUsers")));
    }
    base.push(("src/acl.ts".to_string(), caller("canEdit", "getUser")));
    head.push(("src/acl.ts".to_string(), caller("canEdit", "getUsers")));
    let users = "export function getUser(id: string) { return id; }\nexport function getUsers(id: string) { return [id]; }\n";
    base.push(("src/users.ts".to_string(), users.to_string()));
    head.push(("src/users.ts".to_string(), users.to_string()));
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(
        text.contains("  R1 `User` → `Users`, also inside names (4 hunks)\n"),
        "{text}"
    );
    assert!(text.contains("    R1: src/{a,b,c}.ts:1\n"), "{text}");
    assert!(
        text.contains(concat!(
            "@@ -1,5 +1,5 @@ [R1]\n",
            "-1: import { getUser } from './users';\n",
            "+1: import { getUsers } from './users';\n",
            "-3:   const found = getUser(id);\n",
            "+3:   const found = getUsers(id);\n",
        )),
        "{text}"
    );
}

#[test]
fn brief_moves_fold_only_pure_renames_and_list_them_as_sites() {
    let keep = "export function keep() {\n  return 1;\n}\n";
    let refund = |first: &str, second: &str| {
        format!(
            "export function refund(amount: number) {{\n  const {second} = load();\n  debit({first}, amount);\n  credit({second}, amount);\n  return amount;\n}}\n"
        )
    };
    let moved = |old: &str, first: &str, new: &str| {
        let before = format!("{keep}\n{}", refund(first, old));
        let after = refund(if first == old { new } else { first }, new);
        let extra = [
            ("src/old.ts", before.as_str(), keep),
            ("src/new.ts", "", after.as_str()),
        ];
        brief(&global_swap(old, new, &extra), &[])
    };
    // A move whose renamed lines are pure renames folds; its sites are listed and counted.
    let pure = moved("user", "user", "member");
    assert!(
        pure.contains("  R1 `user` → `member` (4 hunks)\n"),
        "{pure}"
    );
    assert!(
        pure.contains("  moved lines with rewrites (head lines):\n    R1: src/new.ts:2,3,4\n"),
        "{pure}"
    );
    assert!(pure.contains("moved hunks"), "{pure}");
    // Onto a name the declaration already uses, the move stays shown in full.
    let swapped = moved("user", "account", "account");
    assert!(!swapped.contains("hidden: moved"), "{swapped}");
    assert!(!swapped.contains("moved lines with rewrites"), "{swapped}");
    assert!(
        swapped.contains("\n-  credit(user, amount);\n"),
        "{swapped}"
    );
    assert!(
        swapped.contains("\n+  credit(account, amount);\n"),
        "{swapped}"
    );
}

#[test]
fn brief_renames_inside_strings_regexes_and_comments_never_fold() {
    let regex = |role: &str| {
        format!(
            "export function allowed(path: string) {{\n  return /^\\/{role}\\//.test(path);\n}}\n"
        )
    };
    let template = |role: &str| {
        format!(
            "export function query() {{\n  return `\n    SELECT id FROM users\n    WHERE role = {role}\n  `;\n}}\n"
        )
    };
    let python = |role: &str| {
        format!(
            "def purge(db):\n    db.run(\"\"\"\n        DELETE FROM users\n        WHERE role != {role}\n    \"\"\")\n"
        )
    };
    let comment =
        |role: &str| format!("export function z() {{\n  // {role} only\n  return 1;\n}}\n");
    let files = [
        ("src/re.ts", regex("admin"), regex("owner")),
        ("src/tpl.ts", template("admin"), template("owner")),
        ("pkg/sql.py", python("admin"), python("owner")),
        ("src/cm.ts", comment("admin"), comment("owner")),
    ];
    let extra: Vec<(&str, &str, &str)> = files
        .iter()
        .map(|(path, before, after)| (*path, before.as_str(), after.as_str()))
        .collect();
    let text = brief(&global_swap("admin", "owner", &extra), &[]);
    assert!(text.contains("    R1: src/{a,b,c}.ts:1\n"), "{text}");
    for changed in [
        "-2:   return /^\\/admin\\//.test(path);\n+2:   return /^\\/owner\\//.test(path);\n",
        "-4:     WHERE role = admin\n+4:     WHERE role = owner\n",
        "-4:         WHERE role != admin\n+4:         WHERE role != owner\n",
        "-2:   // admin only\n+2:   // owner only\n",
    ] {
        assert!(text.contains(changed), "{changed} missing in {text}");
    }
}

#[test]
fn brief_renames_that_move_line_breaks_show_the_hunk_in_full() {
    let caller = |name: &str, call: &str| {
        format!(
            "import {{ {call} }} from './check';\nexport function {name}(u: U) {{\n  return {call}(u);\n}}\n"
        )
    };
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        base.push((format!("src/{name}.ts"), caller(name, "oldCheck")));
        head.push((format!("src/{name}.ts"), caller(name, "newCheck")));
    }
    base.push((
        "src/guard.ts".to_string(),
        "import { oldCheck } from './check';\nexport function allowed(u: U) {\n  log(u);\n  return oldCheck(u)\n    && u.active;\n}\n"
            .to_string(),
    ));
    head.push((
        "src/guard.ts".to_string(),
        "import { newCheck } from './check';\nexport function allowed(u: U) {\n  log(u);\n  return\n    newCheck(u) && u.active;\n}\n"
            .to_string(),
    ));
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(text.contains("    R1: src/{a,b,c}.ts:1\n"), "{text}");
    assert!(
        text.contains(concat!(
            "-4:   return oldCheck(u)\n",
            "-    && u.active;\n",
            "+4:   return\n",
            "+    newCheck(u) && u.active;\n",
        )),
        "{text}"
    );
}

#[test]
fn brief_value_like_names_are_never_renames() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let call = |argument: &str| {
            format!("export function g_{name}(opts: O) {{\n  return run({argument});\n}}\n")
        };
        base.push((format!("src/g_{name}.ts"), call("opts")));
        head.push((format!("src/g_{name}.ts"), call("globalThis")));
        let typed = |kind: &str| {
            format!("export function f_{name}(payload: {kind}) {{\n  return payload;\n}}\n")
        };
        base.push((format!("src/t_{name}.ts"), typed("unknown")));
        head.push((format!("src/t_{name}.ts"), typed("any")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(text.contains("`opts` → `globalThis` (3 hunks)"), "{text}");
    assert!(text.contains("`unknown` → `any` (3 hunks)"), "{text}");
    assert!(!text.contains("  rewrite-only hunks:"), "{text}");
    assert!(
        text.contains("-2:   return run(opts);\n+2:   return run(globalThis);\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "-1: export function f_a(payload: unknown) {\n+1: export function f_a(payload: any) {\n"
        ),
        "{text}"
    );
}

/// Paths and sources as owned pairs, for fixtures built in loops.
fn sources(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|(path, source)| ((*path).to_string(), (*source).to_string()))
        .collect()
}

/// Asserts that `text` folds no hunk into a rewrite-only listing and shows each `expected` text.
fn assert_shown(text: &str, expected: &[&str]) {
    assert!(!text.contains("  rewrite-only hunks:"), "{text}");
    for part in expected {
        assert!(text.contains(part), "missing {part:?} in {text}");
    }
}

#[test]
fn brief_renames_of_keys_members_and_attributes_stay_compact() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = |binding: &str| {
            format!(
                "export function show_{name}(ctx: Ctx) {{\n  const {binding} = load(ctx);\n  return render({binding});\n}}\n"
            )
        };
        base.push((format!("src/{name}.ts"), source("user")));
        head.push((format!("src/{name}.ts"), source("account")));
    }
    let changed = [
        (
            "src/props.ts",
            "export function view(props: Props) {\n  const { user } = props;\n  return render(user);\n}\n",
        ),
        (
            "src/send.ts",
            "export function send(id: string) {\n  const user = id;\n  return post('/notify', { user });\n}\n",
        ),
        (
            "src/wire.ts",
            "export function wire(id: string) {\n  post('/a', { user: id });\n  return post('/b', { user: id });\n}\n",
        ),
        (
            "src/api.ts",
            "export async function who() {\n  const res = await fetchJson('/me');\n  log(res.user);\n  return res.user;\n}\n",
        ),
        (
            "src/Badge.tsx",
            "export function Badge(p: P) {\n  const v = p.v;\n  return <Avatar user={v} />;\n}\n",
        ),
    ];
    for (path, source) in changed {
        base.push((path.to_string(), source.to_string()));
        head.push((path.to_string(), source.replace("user", "account")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(
        text.contains("  rewrite-only hunks:\n    R1: src/{a,b,c}.ts:2\n"),
        "{text}"
    );
    for part in [
        "-2:   const { user } = props;\n-  return render(user);\n+2:   const { account } = props;\n",
        "+2:   const account = id;\n+  return post('/notify', { account });\n",
        "+2:   post('/a', { account: id });\n",
        "+3:   log(res.account);\n",
        "+3:   return <Avatar account={v} />;\n",
    ] {
        assert!(text.contains(part), "missing {part:?} in {text}");
    }
}

#[test]
fn brief_python_keyword_argument_renames_stay_compact() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = |binding: &str| {
            format!(
                "def show_{name}(ctx):\n    {binding} = load(ctx)\n    return render({binding})\n"
            )
        };
        base.push((format!("pkg/{name}.py"), source("user")));
        head.push((format!("pkg/{name}.py"), source("recipient")));
    }
    base.push((
        "pkg/notify.py".to_string(),
        "def notify(mailer, me):\n    mailer.send(user=me)\n    return mailer.log(user=me)\n"
            .to_string(),
    ));
    head.push((
        "pkg/notify.py".to_string(),
        "def notify(mailer, me):\n    mailer.send(recipient=me)\n    return mailer.log(recipient=me)\n"
            .to_string(),
    ));
    base.push(("pkg/__init__.py".to_string(), String::new()));
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(
        text.contains("  rewrite-only hunks:\n    R1: pkg/{a,b,c}.py:2\n"),
        "{text}"
    );
    assert!(
        text.contains("+2:     mailer.send(recipient=me)\n+    return mailer.log(recipient=me)\n"),
        "{text}"
    );
}

#[test]
fn brief_renames_in_unreviewed_grammars_stay_compact() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["A", "B", "C"] {
        let source = |binding: &str| {
            format!(
                "class Show{name} {{\n    string Run(Ctx c) {{\n        var {binding} = c.Load();\n        return Render({binding});\n    }}\n}}\n"
            )
        };
        base.push((format!("src/Show{name}.cs"), source("user")));
        head.push((format!("src/Show{name}.cs"), source("recipient")));
    }
    let notify = "class Notify {\n    void Run(Mailer m, string me) {\n        m.Send(user: me);\n        m.Log(user: me);\n    }\n}\n";
    base.push(("src/Notify.cs".to_string(), notify.to_string()));
    head.push((
        "src/Notify.cs".to_string(),
        notify.replace("user", "recipient"),
    ));
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert_shown(
        &text,
        &[
            "+3:         m.Send(recipient: me);\n",
            "modified src/ShowA.cs (1 hunk)\n",
            "modified src/ShowC.cs (1 hunk)\n",
            "+3:         var recipient = c.Load();\n",
        ],
    );
}

#[test]
fn brief_rust_field_renames_stay_compact() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = |field: &str| {
            format!(
                "#[derive(serde::Serialize)]\npub struct Out{name} {{ pub {field}: String }}\n\npub fn make_{name}({field}: String) -> Out{name} {{ Out{name} {{ {field} }} }}\n"
            )
        };
        base.push((format!("src/{name}.rs"), source("user")));
        head.push((format!("src/{name}.rs"), source("account")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert_shown(
        &text,
        &[
            "+2: pub struct Outa { pub account: String }\n",
            "+4: pub fn make_b(account: String) -> Outb { Outb { account } }\n",
            "+2: pub struct Outc { pub account: String }\n",
        ],
    );
}

#[test]
fn brief_go_renames_that_change_exportedness_stay_compact() {
    let mut base = sources(&[("go.mod", "module ex\n\ngo 1.22\n")]);
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = |field: &str| {
            format!(
                "package {name}\n\nimport \"encoding/json\"\n\ntype T struct {{\n\t{field} string\n}}\n\nfunc Encode(v string) ([]byte, error) {{\n\treturn json.Marshal(T{{{field}: v}})\n}}\n"
            )
        };
        base.push((format!("{name}/{name}.go"), source("name")));
        head.push((format!("{name}/{name}.go"), source("Name")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert_shown(
        &text,
        &[
            "+6: \tName string\n",
            "+10: \treturn json.Marshal(T{Name: v})\n",
        ],
    );

    let mut base = sources(&[("go.mod", "module ex\n\ngo 1.22\n")]);
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        let source = |helper: &str| {
            format!(
                "package {name}\n\nfunc {helper}(v string) string {{\n\treturn v\n}}\n\nfunc Use(v string) string {{\n\treturn {helper}(v)\n}}\n"
            )
        };
        base.push((format!("{name}/{name}.go"), source("helper")));
        head.push((format!("{name}/{name}.go"), source("Helper")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert_shown(&text, &["+3: func Helper(v string) string {\n"]);
}

#[test]
fn brief_switches_to_new_or_still_existing_functions_stay_compact() {
    let mut base = sources(&[
        (
            "src/lib.ts",
            "export function loadUser(id: string) {\n  return db.get(id);\n}\n\nexport function getUser(id: string) {\n  return cache.get(id);\n}\n",
        ),
        (
            "src/pay.ts",
            "export function charge(a: number) {\n  return a;\n}\n",
        ),
        (
            "src/d.ts",
            "import { getUser } from './lib';\n\nexport function peek(id: string) {\n  return getUser(id);\n}\n",
        ),
    ]);
    let mut head = sources(&[
        (
            "src/lib.ts",
            "export function loadAccount(id: string) {\n  return db.get(id);\n}\n\nexport function getUser(id: string) {\n  return cache.get(id);\n}\n\nexport function getAccount(id: string) {\n  return db.purge(id);\n}\n",
        ),
        (
            "src/pay.ts",
            "export function charge(a: number) {\n  return a;\n}\n\nexport function chargeTwice(a: number) {\n  return a * 2;\n}\n",
        ),
        (
            "src/d.ts",
            "import { getAccount } from './lib';\n\nexport function peek(id: string) {\n  return getAccount(id);\n}\n",
        ),
    ]);
    let caller = |module: &str, function: &str, name: &str, call: &str, parameter: &str| {
        format!(
            "import {{ {call} }} from './{module}';\n\nexport function {function}_{name}({parameter}) {{\n  return {call}({});\n}}\n",
            &parameter[..parameter.find(':').unwrap()]
        )
    };
    for name in ["a", "b", "c"] {
        base.push((
            format!("src/{name}.ts"),
            caller("lib", "show", name, "loadUser", "id: string"),
        ));
        head.push((
            format!("src/{name}.ts"),
            caller("lib", "show", name, "loadAccount", "id: string"),
        ));
    }
    for name in ["e", "f", "g"] {
        base.push((
            format!("src/{name}.ts"),
            caller("pay", "pay", name, "charge", "a: number"),
        ));
        head.push((
            format!("src/{name}.ts"),
            caller("pay", "pay", name, "chargeTwice", "a: number"),
        ));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(
        text.contains("  rewrite-only hunks:\n    R1: src/{a,b,c}.ts:1\n"),
        "{text}"
    );
    for part in [
        "+1: import { getAccount } from './lib';\n",
        "modified src/e.ts (1 hunk)\n",
        "modified src/g.ts (1 hunk)\n",
        "+4:   return chargeTwice(a);\n",
    ] {
        assert!(text.contains(part), "missing {part:?} in {text}");
    }
}

#[test]
fn brief_moves_that_rebind_free_names_are_shown() {
    let old = "const rate = 0.1;\n\nexport function keep() {\n  return 1;\n}\n";
    let fee = "\nexport function fee(amount: number) {\n  const base = amount * rate;\n  return Math.round(base);\n}\n";
    let other = "\nexport function other(amount: number) {\n  return amount * rate;\n}\n";
    let fixture = |target_rate: &str| {
        let target = format!("const rate = {target_rate};\n{other}");
        revisions(
            &[
                ("src/old.ts", &format!("{old}{fee}")),
                ("src/new.ts", &target),
            ],
            &[
                ("src/old.ts", old),
                ("src/new.ts", &format!("{target}{fee}")),
            ],
        )
    };
    let rebound = brief(&fixture("0.2"), &[]);
    assert!(!rebound.contains("moved hunks"), "{rebound}");
    assert!(
        rebound
            .contains("+export function fee(amount: number) {\n+  const base = amount * rate;\n"),
        "{rebound}"
    );
    let same = brief(&fixture("0.1"), &[]);
    assert!(
        same.contains("moved hunks (every changed line recurs in another hunk after rewrites; base lines → head lines):\n  src/old.ts:7-10 → src/new.ts:7-10\n"),
        "{same}"
    );
}

#[test]
fn brief_renames_with_language_meaning_stay_compact() {
    let mut base = sources(&[
        ("pkg/__init__.py", ""),
        (
            "pkg/core.py",
            "def refund(x):\n    return x\n\ndef charge(x):\n    return x\n\ndef settle(x):\n    return x\n",
        ),
    ]);
    let mut head = Vec::new();
    for name in ["refund", "charge", "settle"] {
        let source = |prefix: &str| {
            format!(
                "from pkg.core import {name}\n\n\ndef {prefix}_{name}():\n    assert {name}(1) == 1\n"
            )
        };
        base.push((format!("tests/test_{name}.py"), source("test")));
        head.push((format!("tests/test_{name}.py"), source("check")));
    }
    for name in ["a", "b", "c"] {
        let source = |decorator: &str| {
            format!("class M{name}:\n    @{decorator}\n    def make(x):\n        return x\n")
        };
        base.push((format!("pkg/m_{name}.py"), source("staticmethod")));
        head.push((format!("pkg/m_{name}.py"), source("classmethod")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert_shown(
        &text,
        &[
            "+2:     @classmethod\n",
            "modified pkg/m_c.py (1 hunk)\n",
            "+4: def check_charge():\n",
            "+4: def check_settle():\n",
        ],
    );
}

#[test]
fn brief_names_in_files_without_inventory_block_folding() {
    let fixture = |rates: Option<&str>| {
        let mut base = sources(&[("pkg/__init__.py", "")]);
        if let Some(rates) = rates {
            base.push(("pkg/rates.pyx".to_string(), rates.to_string()));
        }
        let mut head = Vec::new();
        for name in ["a", "b", "c"] {
            let source = |rate: &str| {
                format!(
                    "from .rates import {rate}\n\n\ndef price_{name}(x):\n    return x * {rate}\n"
                )
            };
            base.push((format!("pkg/{name}.py"), source("DEFAULT_RATE")));
            head.push((format!("pkg/{name}.py"), source("PROMO_RATE")));
        }
        revisions(&owned(&base), &owned(&head))
    };
    let blocked = brief(
        &fixture(Some("DEFAULT_RATE = 0.1\nPROMO_RATE = 0.5\n")),
        &[],
    );
    assert_shown(
        &blocked,
        &[
            "+5:     return x * PROMO_RATE\n",
            "modified pkg/c.py (1 hunk)\n",
        ],
    );
    let fresh = brief(&fixture(None), &[]);
    assert!(
        fresh.contains("  rewrite-only hunks:\n    R1: pkg/{a,b,c}.py:1\n"),
        "{fresh}"
    );
}

/// Benign `user` → `account` renames in `{dir}/{a,b,c}.{extension}`, plus probe files whose every
/// `user` becomes `account` as well.
fn user_probes(
    dir: &str,
    extension: &str,
    benign: &dyn Fn(&str, &str) -> String,
    probes: &[(&str, &str)],
) -> Fixture {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for name in ["a", "b", "c"] {
        base.push((format!("{dir}/{name}.{extension}"), benign(name, "user")));
        head.push((format!("{dir}/{name}.{extension}"), benign(name, "account")));
    }
    for (path, source) in probes {
        base.push(((*path).to_string(), (*source).to_string()));
        head.push(((*path).to_string(), source.replace("user", "account")));
    }
    revisions(&owned(&base), &owned(&head))
}

/// The benign sites fold as `folded` and every probe file keeps a shown hunk.
fn assert_probes_shown(text: &str, folded: &str, probes: &[(&str, &str)]) {
    assert!(
        text.contains(&format!("  rewrite-only hunks:\n    R1: {folded}\n")),
        "{text}"
    );
    for (path, _) in probes {
        assert!(
            text.contains(&format!("modified {path} (")),
            "{path} in {text}"
        );
    }
}

#[test]
fn brief_script_renames_bound_outside_the_tree_are_shown() {
    let benign = |name: &str, binding: &str| {
        format!(
            "export function show_{name}(ctx: Ctx) {{\n  const {binding} = load(ctx);\n  return render({binding});\n}}\n"
        )
    };
    let probes = [
        (
            "src/x_alias.ts",
            "import { user as u } from 'ext-lib';\n\nexport function f() {\n  return u();\n}\n",
        ),
        (
            "src/x_reexport.ts",
            "export { user as current } from 'ext-lib';\nexport const one = 1;\n",
        ),
        (
            "src/x_global.ts",
            "declare global {\n  var user: string;\n}\n\nexport function g() {\n  return user;\n}\n",
        ),
        (
            "src/x_tag.tsx",
            "export function T() {\n  const v = 1;\n  return <user id={v} />;\n}\n",
        ),
    ];
    let text = brief(&user_probes("src", "ts", &benign, &probes), &[]);
    assert_probes_shown(&text, "src/{a,b,c}.ts:2", &probes);
}

#[test]
fn brief_python_renames_of_external_imports_are_shown() {
    let benign = |name: &str, binding: &str| {
        format!("def show_{name}(ctx):\n    {binding} = load(ctx)\n    return render({binding})\n")
    };
    let probes = [(
        "pkg/x_alias.py",
        "from extlib import user as u\n\n\ndef f():\n    return u()\n",
    )];
    let text = brief(&user_probes("pkg", "py", &benign, &probes), &[]);
    assert_probes_shown(&text, "pkg/{a,b,c}.py:2", &probes);
}

#[test]
fn brief_python_renames_of_keyword_only_parameters_are_shown() {
    let benign = |name: &str, binding: &str| {
        format!("def show_{name}(ctx):\n    {binding} = load(ctx)\n    return render({binding})\n")
    };
    let probes = [
        (
            "pkg/x_bare.py",
            "def k(*, user=None):\n    return load(user)\n",
        ),
        (
            "pkg/x_star.py",
            "def j(*args, user: int):\n    return load(user)\n",
        ),
    ];
    let text = brief(&user_probes("pkg", "py", &benign, &probes), &[]);
    assert_probes_shown(&text, "pkg/{a,b,c}.py:2", &probes);
}

#[test]
fn brief_rust_renames_of_external_and_foreign_items_are_shown() {
    let benign = |name: &str, binding: &str| {
        format!(
            "pub fn show_{name}(ctx: u32) -> u32 {{\n    let {binding} = load(ctx);\n    render({binding})\n}}\n"
        )
    };
    let probes = [
        (
            "src/x_uselist.rs",
            "use extcrate::{user as u};\n\npub fn f() -> u32 {\n    u()\n}\n",
        ),
        (
            "src/x_extern.rs",
            "extern \"C\" {\n    fn user(id: u32) -> u32;\n}\n\npub fn call(id: u32) -> u32 {\n    unsafe { user(id) }\n}\n",
        ),
    ];
    let text = brief(&user_probes("src", "rs", &benign, &probes), &[]);
    assert_probes_shown(&text, "src/{a,b,c}.rs:2", &probes);
}

#[test]
fn brief_rust_renames_of_macros_and_extern_functions_are_shown() {
    let benign = |name: &str, binding: &str| {
        format!(
            "pub fn show_{name}(ctx: u32) -> u32 {{\n    let {binding} = load(ctx);\n    render({binding})\n}}\n"
        )
    };
    let probes = [
        (
            "src/x_macro.rs",
            "macro_rules! user {\n    () => {\n        1\n    };\n}\n\npub fn m() -> u32 {\n    user!()\n}\n",
        ),
        (
            "src/x_cabi.rs",
            "pub extern \"C\" fn user(id: u32) -> u32 {\n    id\n}\n",
        ),
    ];
    let text = brief(&user_probes("src", "rs", &benign, &probes), &[]);
    assert_probes_shown(&text, "src/{a,b,c}.rs:2", &probes);
}

#[test]
fn brief_renames_that_merge_or_split_names_in_a_file_are_shown() {
    let mut base = Vec::new();
    let mut head = Vec::new();
    for (names, old, new) in [
        (["a", "b", "c"], "user", "account"),
        (["d", "e", "f"], "acct", "account"),
        (["g", "h", "i"], "item", "entry"),
        (["j", "k", "l"], "item", "record"),
    ] {
        for name in names {
            let script = format!(
                "export function show_{name}(ctx: Ctx) {{\n  const {old} = load(ctx);\n  return render({old});\n}}\n"
            );
            head.push((format!("src/{name}.ts"), script.replace(old, new)));
            base.push((format!("src/{name}.ts"), script));
        }
    }
    let files = [
        (
            "src/merge.ts",
            "const user = 1;\n\nexport function m(acct: number) {\n  return user + acct;\n}\n",
            "const account = 1;\n\nexport function m(account: number) {\n  return account + account;\n}\n",
        ),
        (
            "src/split.ts",
            "export function s(ctx: Ctx) {\n  const item = load(ctx);\n  return render(item);\n}\n",
            "export function s(ctx: Ctx) {\n  const entry = load(ctx);\n  return render(record);\n}\n",
        ),
    ];
    for (path, before, after) in files {
        base.push((path.to_string(), before.to_string()));
        head.push((path.to_string(), after.to_string()));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(text.contains("  rewrite-only hunks:\n"), "{text}");
    assert!(text.contains("src/{a,b,c}.ts:2"), "{text}");
    assert!(
        text.contains("+3: export function m(account: number) {\n+  return account + account;\n"),
        "{text}"
    );
    assert!(
        text.contains("+2:   const entry = load(ctx);\n+  return render(record);\n"),
        "{text}"
    );
}

#[test]
fn brief_methods_move_only_between_identical_container_headers() {
    let fee = "\n  fee(amount: number) {\n    const base = super.fee(amount);\n    return Math.round(base * 2);\n  }\n";
    let fixture = |target: &str| {
        let source = "export class A extends BaseOne {\n  keep() {\n    return 1;\n  }\n";
        let other = "  other() {\n    return 2;\n  }\n";
        revisions(
            &[
                ("src/a.ts", &format!("{source}{fee}}}\n")),
                ("src/b.ts", &format!("{target} {{\n{other}}}\n")),
            ],
            &[
                ("src/a.ts", &format!("{source}}}\n")),
                ("src/b.ts", &format!("{target} {{\n{other}{fee}}}\n")),
            ],
        )
    };
    let rebased = brief(&fixture("export class B extends BaseTwo"), &[]);
    assert!(!rebased.contains("moved hunks"), "{rebased}");
    assert!(
        rebased.contains("+    const base = super.fee(amount);\n"),
        "{rebased}"
    );
    let same = brief(&fixture("export class A extends BaseOne"), &[]);
    assert!(same.contains("moved hunks"), "{same}");
}

#[test]
fn brief_renames_through_relative_imports_still_fold() {
    let library =
        |name: &str| format!("export function {name}(id: string) {{\n  return fetch(id);\n}}\n");
    let caller = |name: &str, call: &str| {
        format!(
            "import {{ {call} }} from './lib';\n\nexport function show_{name}(id: string) {{\n  return {call}(id);\n}}\n"
        )
    };
    let mut base = sources(&[("src/lib.ts", &library("loadUser"))]);
    let mut head = sources(&[("src/lib.ts", &library("loadAccount"))]);
    for name in ["a", "b", "c"] {
        base.push((format!("src/{name}.ts"), caller(name, "loadUser")));
        head.push((format!("src/{name}.ts"), caller(name, "loadAccount")));
    }
    let text = brief(&revisions(&owned(&base), &owned(&head)), &[]);
    assert!(
        text.contains("  rewrite-only hunks:\n    R1: src/{a,b,c,lib}.ts:1\n"),
        "{text}"
    );
    assert!(!text.contains("modified src/"), "{text}");
}

const HANDLER_TYPES: &str =
    "export interface Handler {\n  validate(input: Record<string, unknown>): string[];\n}\n";
const PLUMBING: &str = "import type { Handler } from './types';\n\nexport class Plumbing implements Handler {\n  validate(input: Record<string, unknown>): string[] {\n    return input.pipe ? [] : ['pipe'];\n  }\n}\n";
const JOBS: &str = "import { handlers } from './handlers/registry';\n\nexport function complete(kind: string, input: Record<string, unknown>): string[] {\n  const handler = handlers[kind];\n  return handler.validate(input);\n}\n";

fn handler(class: &str, field: &str) -> String {
    format!(
        "import type {{ Handler }} from './types';\n\nexport class {class} implements Handler {{\n  validate(input: Record<string, unknown>): string[] {{\n    return input.{field} ? [] : ['{field}'];\n  }}\n}}\n"
    )
}

fn registry(classes: &[(&str, &str)]) -> String {
    let mut source = String::from("import type { Handler } from './types';\n");
    for (class, file) in classes {
        let _ = writeln!(source, "import {{ {class} }} from './{file}';");
    }
    source.push_str("\nexport const handlers: Record<string, Handler> = {\n");
    for (class, file) in classes {
        let _ = writeln!(source, "  {file}: new {class}(),");
    }
    source.push_str("};\n");
    source
}

fn dispatch_line(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.strip_prefix("unresolved dispatch: "))
}

#[test]
fn brief_names_added_implementations_reached_only_through_dispatch() {
    let electric = handler("Electric", "meter");
    let gas = handler("Gas", "valve");
    let fake = handler("Fake", "stub");
    let old_registry = registry(&[("Plumbing", "plumbing")]);
    let new_registry = registry(&[
        ("Plumbing", "plumbing"),
        ("Electric", "electric"),
        ("Gas", "gas"),
    ]);
    let fixture = revisions(
        &[
            ("src/handlers/types.ts", HANDLER_TYPES),
            ("src/handlers/plumbing.ts", PLUMBING),
            ("src/handlers/registry.ts", &old_registry),
            ("src/jobs.ts", JOBS),
        ],
        &[
            ("src/handlers/electric.ts", &electric),
            ("src/handlers/gas.ts", &gas),
            ("src/handlers/registry.ts", &new_registry),
            ("tests/fake.test.ts", &fake),
        ],
    );
    let text = brief(&fixture, &[]);
    assert!(
        text.contains(
            "note: static potential impact only; run the repository checks before concluding;"
        ),
        "{text}"
    );
    let line = dispatch_line(&text).unwrap_or_else(|| panic!("{text}"));
    // The test double and the registry importer are neither implementations nor call sites.
    assert!(
        line.starts_with("added methods 2 in files 2 reuse existing method names"),
        "{line}"
    );
    assert!(
        line.ends_with(": validate (2) called at src/jobs.ts:5"),
        "{line}"
    );
    assert_eq!(brief(&fixture, &[]), text);
    // Every budget keeps the line within the budget; the collapsed rung counts sites instead.
    for (budget, calls) in [
        (3000, "called at src/jobs.ts:5"),
        (600, "called at src/jobs.ts:5"),
        (300, "1 receiver call site"),
    ] {
        let small = brief(&fixture, &["--budget", &budget.to_string()]);
        assert!(tokens(&small) <= budget, "{small}");
        let line = dispatch_line(&small).unwrap_or_else(|| panic!("{small}"));
        assert!(line.ends_with(&format!(": validate (2) {calls}")), "{line}");
    }
}

#[test]
fn brief_omits_dispatch_line_without_added_implementations_of_existing_members() {
    // Implementations edited in place, a new member name and direct function calls stay silent.
    let edited = PLUMBING.replace("input.pipe", "input.pipe && input.seal");
    let fixture = revisions(
        &[
            ("src/handlers/types.ts", HANDLER_TYPES),
            ("src/handlers/plumbing.ts", PLUMBING),
            (
                "src/handlers/registry.ts",
                &registry(&[("Plumbing", "plumbing")]),
            ),
            ("src/jobs.ts", JOBS),
        ],
        &[
            ("src/handlers/plumbing.ts", &edited),
            (
                "src/handlers/audit.ts",
                "export class Audit {\n  inspect(input: Record<string, unknown>): string[] {\n    return Object.keys(input);\n  }\n}\n\nexport function audited(kind: string): string {\n  return kind.trim();\n}\n",
            ),
            (
                "src/report.ts",
                "import { Audit, audited } from './handlers/audit';\n\nexport function report(kind: string): string[] {\n  return [audited(kind), ...new Audit().inspect({ kind })];\n}\n",
            ),
        ],
    );
    let text = brief(&fixture, &[]);
    assert!(text.contains("modified src/handlers/plumbing.ts"), "{text}");
    assert!(dispatch_line(&text).is_none(), "{text}");
}
