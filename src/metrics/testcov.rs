//! Runner-backed aggregate test discovery plus conventional filename helpers
//! used only for navigation and production/test corpus projections.

use crate::model::{LineRange, TestFramework};
use crate::walk::{self, BoundedText};
use std::path::Path;
use tree_sitter::{Node, Tree};

/// Conventional files that can establish runner context for a narrower scan
/// target. The scan orchestrator probes only these names in target ancestors;
/// content-bearing manifests are still validated by `detect_frameworks`.
pub(crate) const RUNNER_EVIDENCE_FILE_NAMES: &[&str] = &[
    "Cargo.toml",
    "go.mod",
    "pytest.ini",
    "phpunit.xml",
    "phpunit.xml.dist",
    "package.json",
    "composer.json",
    "pyproject.toml",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "jest.config.js",
    "jest.config.mjs",
    "jest.config.cjs",
    "jest.config.ts",
    "jest.config.cts",
    "jest.config.json",
];

/// Detect test runners from discovered repository manifests and conventional
/// runner configuration. Test-looking source filenames alone are not setup
/// evidence.
#[must_use]
pub fn detect_frameworks<'a>(
    files: impl IntoIterator<Item = (&'a Path, &'a Path)>,
    max_file_bytes: u64,
) -> Vec<TestFramework> {
    let mut found = Vec::new();
    for (absolute_path, report_path) in files {
        let path = report_path.to_string_lossy().replace('\\', "/");
        let name = path
            .rsplit('/')
            .next()
            .unwrap_or(&path)
            .to_ascii_lowercase();
        let direct = if name == "cargo.toml" {
            Some("cargo-test")
        } else if name == "go.mod" {
            Some("go-test")
        } else if name == "pytest.ini" {
            Some("pytest")
        } else if matches!(name.as_str(), "phpunit.xml" | "phpunit.xml.dist") {
            Some("phpunit")
        } else if runner_config_name(
            &name,
            "vitest.config",
            &["js", "mjs", "cjs", "ts", "mts", "cts"],
        ) {
            Some("vitest")
        } else if runner_config_name(
            &name,
            "jest.config",
            &["js", "mjs", "cjs", "ts", "cts", "json"],
        ) {
            Some("jest")
        } else {
            None
        };
        if let Some(framework) = direct {
            push_framework(&mut found, framework, &path);
            continue;
        }

        let detector: fn(&str, &str, &mut Vec<TestFramework>) = match name.as_str() {
            "package.json" => detect_package_json,
            "composer.json" => detect_composer_json,
            "pyproject.toml" => detect_pyproject,
            _ => continue,
        };
        if let BoundedText::Content(content) =
            walk::read_text_bounded(absolute_path, max_file_bytes)
        {
            detector(&content, &path, &mut found);
        }
    }
    found.sort_by(|left, right| (&left.name, &left.evidence).cmp(&(&right.name, &right.evidence)));
    found
}

fn runner_config_name(name: &str, stem: &str, extensions: &[&str]) -> bool {
    name.strip_prefix(stem)
        .and_then(|suffix| suffix.strip_prefix('.'))
        .is_some_and(|extension| extensions.contains(&extension))
}

fn detect_package_json(content: &str, evidence: &str, found: &mut Vec<TestFramework>) {
    let Ok(document) = serde_json::from_str::<serde_json::Value>(content) else {
        return;
    };
    let dependency = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .any(|section| document[*section].get(name).is_some())
    };
    if dependency("vitest") {
        push_framework(found, "vitest", evidence);
    }
    if dependency("jest")
        || document
            .get("jest")
            .and_then(serde_json::Value::as_object)
            .is_some()
    {
        push_framework(found, "jest", evidence);
    }
    for script in document["scripts"]
        .as_object()
        .into_iter()
        .flat_map(|scripts| scripts.values())
        .filter_map(serde_json::Value::as_str)
    {
        if script_invokes(script, "vitest", |_| true) {
            push_framework(found, "vitest", evidence);
        }
        if script_invokes(script, "jest", |_| true) {
            push_framework(found, "jest", evidence);
        }
        if script_invokes(script, "bun", |arguments| {
            arguments.first() == Some(&"test")
        }) {
            push_framework(found, "bun-test", evidence);
        }
        if script_invokes(script, "node", |arguments| {
            arguments
                .iter()
                .any(|argument| *argument == "--test" || argument.starts_with("--test="))
        }) {
            push_framework(found, "node-test", evidence);
        }
    }
}

fn script_invokes(
    script: &str,
    expected_program: &str,
    arguments_match: impl Fn(&[&str]) -> bool,
) -> bool {
    script_segments(script).into_iter().any(|segment| {
        let tokens = command_tokens(segment);
        command_invocation(&tokens).is_some_and(|(program, arguments)| {
            program == expected_program && arguments_match(arguments)
        })
    })
}

fn script_segments(script: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in script.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if quote.is_none() && matches!(character, ';' | '|' | '&' | '\n') {
            segments.push(&script[start..index]);
            start = index + character.len_utf8();
        }
    }
    segments.push(&script[start..]);
    segments
}

fn command_invocation<'a>(tokens: &'a [&'a str]) -> Option<(&'a str, &'a [&'a str])> {
    let mut index = 0;
    skip_environment_assignments(tokens, &mut index);
    let first = program_name(tokens.get(index)?);
    if matches!(first, "env" | "cross-env" | "cross-env-shell" | "exec") {
        index += 1;
        skip_environment_assignments(tokens, &mut index);
    }

    let launcher = program_name(tokens.get(index)?);
    if matches!(launcher, "npx" | "bunx") {
        index += 1;
        skip_argument_separator(tokens, &mut index);
    } else if matches!(launcher, "yarn" | "pnpm") {
        index += 1;
        if tokens
            .get(index)
            .is_some_and(|token| matches!(*token, "exec" | "dlx" | "x"))
        {
            index += 1;
            skip_argument_separator(tokens, &mut index);
        }
    } else if launcher == "npm" {
        index += 1;
        if !tokens
            .get(index)
            .is_some_and(|token| matches!(*token, "exec" | "x"))
        {
            return None;
        }
        index += 1;
        skip_argument_separator(tokens, &mut index);
    }

    let program = program_name(tokens.get(index)?);
    if program.starts_with('-') {
        return None;
    }
    Some((program, &tokens[index + 1..]))
}

fn skip_argument_separator(tokens: &[&str], index: &mut usize) {
    if tokens.get(*index) == Some(&"--") {
        *index += 1;
    }
}

fn skip_environment_assignments(tokens: &[&str], index: &mut usize) {
    while tokens.get(*index).is_some_and(|token| {
        token.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
        })
    }) {
        *index += 1;
    }
}

fn program_name(token: &str) -> &str {
    let basename = token.rsplit(['/', '\\']).next().unwrap_or(token);
    basename.strip_suffix(".cmd").unwrap_or(basename)
}

fn command_tokens(script: &str) -> Vec<&str> {
    script
        .split_whitespace()
        .map(|part| {
            part.trim_matches(|character: char| matches!(character, '\'' | '"' | ';' | '(' | ')'))
        })
        .filter(|part| !part.is_empty())
        .collect()
}

fn detect_composer_json(content: &str, evidence: &str, found: &mut Vec<TestFramework>) {
    let Ok(document) = serde_json::from_str::<serde_json::Value>(content) else {
        return;
    };
    if ["require", "require-dev"]
        .iter()
        .any(|section| document[*section].get("phpunit/phpunit").is_some())
    {
        push_framework(found, "phpunit", evidence);
    }
}

fn detect_pyproject(content: &str, evidence: &str, found: &mut Vec<TestFramework>) {
    let Ok(document) = toml::from_str::<toml::Value>(content) else {
        return;
    };
    let has_config = document
        .get("tool")
        .and_then(|tool| tool.get("pytest"))
        .and_then(toml::Value::as_table)
        .is_some();
    let declares_dependency = pyproject_declares_pytest(&document);
    if has_config || declares_dependency {
        push_framework(found, "pytest", evidence);
    }
}

fn pyproject_declares_pytest(document: &toml::Value) -> bool {
    let project = document.get("project");
    let pep_621 = project.is_some_and(|project| {
        requirement_list_declares_pytest(project.get("dependencies"))
            || project
                .get("optional-dependencies")
                .and_then(toml::Value::as_table)
                .is_some_and(|groups| groups.values().any(requirement_value_declares_pytest))
    });
    let pep_735 = document
        .get("dependency-groups")
        .and_then(toml::Value::as_table)
        .is_some_and(|groups| groups.values().any(requirement_value_declares_pytest));
    let poetry = document
        .get("tool")
        .and_then(|tool| tool.get("poetry"))
        .is_some_and(|poetry| {
            dependency_table_declares_pytest(poetry.get("dependencies"))
                || dependency_table_declares_pytest(poetry.get("dev-dependencies"))
                || poetry
                    .get("group")
                    .and_then(toml::Value::as_table)
                    .is_some_and(|groups| {
                        groups.values().any(|group| {
                            dependency_table_declares_pytest(group.get("dependencies"))
                        })
                    })
        });
    let pdm = document
        .get("tool")
        .and_then(|tool| tool.get("pdm"))
        .and_then(|pdm| pdm.get("dev-dependencies"))
        .and_then(toml::Value::as_table)
        .is_some_and(|groups| groups.values().any(requirement_value_declares_pytest));
    let tool = document.get("tool");
    let uv_or_rye = ["uv", "rye"].iter().any(|name| {
        tool.and_then(|tool| tool.get(*name))
            .is_some_and(|config| requirement_list_declares_pytest(config.get("dev-dependencies")))
    });
    let hatch = tool
        .and_then(|tool| tool.get("hatch"))
        .and_then(|hatch| hatch.get("envs"))
        .and_then(toml::Value::as_table)
        .is_some_and(|environments| {
            environments.values().any(|environment| {
                requirement_list_declares_pytest(environment.get("dependencies"))
            })
        });

    pep_621 || pep_735 || poetry || pdm || uv_or_rye || hatch
}

fn requirement_list_declares_pytest(value: Option<&toml::Value>) -> bool {
    value.is_some_and(requirement_value_declares_pytest)
}

fn requirement_value_declares_pytest(value: &toml::Value) -> bool {
    value.as_array().is_some_and(|requirements| {
        requirements
            .iter()
            .filter_map(toml::Value::as_str)
            .any(requirement_names_pytest)
    })
}

fn dependency_table_declares_pytest(value: Option<&toml::Value>) -> bool {
    value
        .and_then(toml::Value::as_table)
        .is_some_and(|dependencies| {
            dependencies
                .keys()
                .any(|name| normalized_name(name) == "pytest")
        })
}

fn requirement_names_pytest(requirement: &str) -> bool {
    let name = requirement
        .trim_start()
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .collect::<String>();
    normalized_name(&name) == "pytest"
}

fn normalized_name(name: &str) -> String {
    name.to_ascii_lowercase().replace(['_', '.'], "-")
}

fn push_framework(found: &mut Vec<TestFramework>, name: &str, evidence: &str) {
    if !found
        .iter()
        .any(|item| item.name == name && item.evidence == evidence)
    {
        found.push(TestFramework {
            name: name.to_string(),
            evidence: evidence.to_string(),
        });
    }
}

/// Apply the detected runners' default test-file conventions.
#[must_use]
pub fn is_framework_test_file(frameworks: &[TestFramework], rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/");
    frameworks.iter().any(|framework| {
        framework_relative_path(framework, &normalized)
            .is_some_and(|relative| framework_matches_path(&framework.name, relative))
    })
}

#[must_use]
pub(crate) fn framework_applies_to_path(framework: &TestFramework, rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/");
    framework_relative_path(framework, &normalized).is_some()
}

fn framework_relative_path<'a>(framework: &TestFramework, path: &'a str) -> Option<&'a str> {
    let evidence = framework.evidence.replace('\\', "/");
    let scope = evidence.rsplit_once('/').map_or("", |(scope, _)| scope);
    if scope.is_empty() {
        return Some(path);
    }
    path.strip_prefix(scope)?.strip_prefix('/')
}

fn framework_matches_path(framework: &str, relative: &str) -> bool {
    let filename = relative.rsplit('/').next().unwrap_or(relative);
    match framework {
        "cargo-test" => cargo_test_path(relative),
        "go-test" => filename.ends_with("_test.go"),
        "pytest" => pytest_test_file(filename),
        "phpunit" => phpunit_test_file(filename),
        "jest" => jest_test_path(relative, filename),
        "vitest" => is_vitest_extension(file_extension(filename)) && dot_test_or_spec(filename),
        "bun-test" => bun_test_file(filename),
        "node-test" => node_test_path(relative, filename),
        _ => false,
    }
}

fn cargo_test_path(relative: &str) -> bool {
    let components = relative.split('/').collect::<Vec<_>>();
    file_extension(relative.rsplit('/').next().unwrap_or(relative)) == Some("rs")
        && ((components.len() == 2 && components[0] == "tests")
            || (components.len() == 3 && components[0] == "tests" && components[2] == "main.rs"))
}

fn pytest_test_file(filename: &str) -> bool {
    file_extension(filename) == Some("py")
        && (filename.starts_with("test_") || filename.ends_with("_test.py"))
}

fn phpunit_test_file(filename: &str) -> bool {
    filename
        .strip_suffix("Test.php")
        .is_some_and(|base| !base.is_empty())
        || file_extension(filename) == Some("phpt")
}

fn jest_test_path(relative: &str, filename: &str) -> bool {
    is_jest_extension(file_extension(filename))
        && (relative
            .split('/')
            .any(|component| component == "__tests__")
            || test_or_spec_stem(filename))
}

fn bun_test_file(filename: &str) -> bool {
    is_bun_extension(file_extension(filename))
        && (dot_test_or_spec(filename) || underscore_test_or_spec(filename))
}

fn node_test_path(relative: &str, filename: &str) -> bool {
    is_node_extension(file_extension(filename))
        && (relative.split('/').any(|component| component == "test") || node_test_stem(filename))
}

fn file_extension(filename: &str) -> Option<&str> {
    filename.rsplit_once('.').map(|(_, extension)| extension)
}

fn is_jest_extension(extension: Option<&str>) -> bool {
    matches!(extension, Some("js" | "jsx" | "ts" | "tsx"))
}

fn is_vitest_extension(extension: Option<&str>) -> bool {
    matches!(
        extension,
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts")
    )
}

fn is_bun_extension(extension: Option<&str>) -> bool {
    matches!(extension, Some("js" | "jsx" | "ts" | "tsx"))
}

fn is_node_extension(extension: Option<&str>) -> bool {
    matches!(extension, Some("js" | "cjs" | "mjs"))
}

fn dot_test_or_spec(filename: &str) -> bool {
    stem_has_suffix(filename, &[".test", ".spec"])
}

fn underscore_test_or_spec(filename: &str) -> bool {
    stem_has_suffix(filename, &["_test", "_spec"])
}

fn stem_has_suffix(filename: &str, suffixes: &[&str]) -> bool {
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    suffixes.iter().any(|suffix| {
        stem.strip_suffix(suffix)
            .is_some_and(|base| !base.is_empty())
    })
}

fn test_or_spec_stem(filename: &str) -> bool {
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    matches!(stem, "test" | "spec")
        || stem
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| matches!(suffix, "test" | "spec"))
}

fn node_test_stem(filename: &str) -> bool {
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    stem == "test"
        || stem.starts_with("test-")
        || stem
            .rsplit_once('.')
            .is_some_and(|(_, suffix)| suffix == "test")
        || stem.ends_with("-test")
        || stem.ends_with("_test")
}

/// Return whether a parsed Rust file contains an inline test attribute.
///
/// Attribute nodes are inspected instead of raw source text so examples in
/// comments and strings cannot mark a source file as tested.
#[must_use]
pub fn has_inline_rust_tests(content: &str, tree: &Tree) -> bool {
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "attribute" && is_test_attribute(node, content) {
            return true;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    false
}

/// Return 1-based inclusive line ranges that only compile for Rust test builds.
///
/// Direct `#[cfg(test)]` items and test functions are included. Compound
/// conditions are conservatively retained because they may also compile in a
/// production configuration.
#[must_use]
pub fn inline_rust_test_regions(content: &str, tree: &Tree) -> Vec<LineRange> {
    let mut ranges = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "attribute"
            && is_test_only_attribute(node, content)
            && let Some(item) = attributed_item(node)
        {
            ranges.push(LineRange {
                start: node.start_position().row + 1,
                end: item.end_position().row + 1,
            });
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    merge_line_ranges(ranges)
}

fn attributed_item(node: Node<'_>) -> Option<Node<'_>> {
    let parent = node.parent()?;
    if parent.kind() != "attribute_item" {
        return None;
    }
    let mut sibling = parent.next_named_sibling()?;
    while sibling.kind() == "attribute_item" {
        sibling = sibling.next_named_sibling()?;
    }
    Some(sibling)
}

fn is_test_only_attribute(node: Node<'_>, content: &str) -> bool {
    let Some(name) = attribute_name(node, content) else {
        return false;
    };
    if name == "test" || name.ends_with("::test") {
        return true;
    }
    if name != "cfg" {
        return false;
    }

    let Some(arguments) = node.child_by_field_name("arguments") else {
        return false;
    };
    let Ok(predicate) = arguments.utf8_text(content.as_bytes()) else {
        return false;
    };
    predicate
        .chars()
        .filter(|character| !character.is_whitespace())
        .eq("(test)".chars())
}

fn is_test_attribute(node: Node<'_>, content: &str) -> bool {
    let Some(name) = attribute_name(node, content) else {
        return false;
    };

    if name == "test" || name.ends_with("::test") {
        return true;
    }
    if name != "cfg" {
        return false;
    }

    node.child_by_field_name("arguments")
        .is_some_and(|arguments| subtree_enables_test(arguments, content, true))
}

fn attribute_name<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() != "token_tree")
        .and_then(|child| child.utf8_text(content.as_bytes()).ok())
}

fn subtree_enables_test(node: Node<'_>, content: &str, positive: bool) -> bool {
    if node.kind() == "identifier"
        && node
            .utf8_text(content.as_bytes())
            .is_ok_and(|text| text == "test")
    {
        return positive;
    }

    let mut cursor = node.walk();
    let children = node.named_children(&mut cursor).collect::<Vec<_>>();
    let mut index = 0;
    while index < children.len() {
        let negates_next = node.kind() == "token_tree"
            && children[index].kind() == "identifier"
            && children[index]
                .utf8_text(content.as_bytes())
                .is_ok_and(|text| text == "not")
            && index + 1 < children.len();
        if negates_next {
            if subtree_enables_test(children[index + 1], content, !positive) {
                return true;
            }
            index += 2;
        } else {
            if subtree_enables_test(children[index], content, positive) {
                return true;
            }
            index += 1;
        }
    }
    false
}

fn merge_line_ranges(mut ranges: Vec<LineRange>) -> Vec<LineRange> {
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<LineRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end.saturating_add(1)
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

/// Returns `true` if `rel_path` looks like a test file.
///
/// A file is classified as a test if:
/// - any path component (directory or filename, after normalising `\` to `/`)
///   is one of `tests`, `test`, `__tests__`, `spec`, or `specs`; OR
/// - the filename stem starts with `test_`, ends with `_test`, or the filename
///   contains `.test.` or `.spec.`; OR
/// - a Rust filename is exactly `tests.rs`, the conventional name for a split
///   test-only module; OR
/// - the filename ends with `_spec.rb`; OR
/// - a PHP filename follows `PHPUnit`'s `SomethingTest.php` convention.
#[must_use]
pub fn is_test_file(rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/");

    for component in normalized.split('/') {
        if matches!(
            component.to_ascii_lowercase().as_str(),
            "tests" | "test" | "__tests__" | "spec" | "specs"
        ) {
            return true;
        }
    }

    let filename = normalized.rsplit('/').next().unwrap_or(&normalized);
    let lower = filename.to_lowercase();

    if lower == "tests.rs" {
        return true;
    }

    let stem = if let Some(pos) = lower.rfind('.') {
        &lower[..pos]
    } else {
        &lower[..]
    };

    if stem.starts_with("test_") || stem.ends_with("_test") {
        return true;
    }
    if lower.contains(".test.") || lower.contains(".spec.") {
        return true;
    }
    if lower.ends_with("_spec.rb") {
        return true;
    }
    if is_phpunit_filename(filename) {
        return true;
    }

    false
}

/// Returns the package-qualified logical key for a source file.
///
/// The final extension and one conventional source-layout directory are
/// removed, while package prefixes and nested directories are retained.
/// Examples: `Foo.ts` → `"foo"`, `packages/web/src/api/Foo.ts` →
/// `"packages/web/api/foo"`.
#[must_use]
pub fn source_stem(rel_path: &str) -> String {
    let mut components = stem_components(rel_path);
    strip_layout_components(&mut components, false);
    components.join("/")
}

/// For a **test** file, returns the candidate logical source keys it likely
/// covers.
///
/// Package prefixes and nested directories are retained, conventional source
/// and test layout directories are removed, and filename variants are produced
/// by stripping well-known affixes:
/// leading `test_`, trailing `_test`, trailing `.test`, trailing `.spec`,
/// trailing `_spec`.
/// A Rust `tests/cli.rs` integration suite additionally maps to the package
/// `src/main.rs` key.
///
/// The raw stem is always the first element; duplicates are removed while
/// preserving order.
#[must_use]
pub fn test_stem_keys(rel_path: &str) -> Vec<String> {
    let normalized = rel_path.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or(&normalized);
    let rust_cli_test = filename.eq_ignore_ascii_case("cli.rs")
        && normalized
            .split('/')
            .any(|component| matches!(component.to_ascii_lowercase().as_str(), "tests" | "test"));
    let phpunit_test = is_phpunit_filename(filename);
    let mut components = stem_components(rel_path);
    strip_layout_components(&mut components, true);
    let raw = components.pop().unwrap_or_default();

    let mut stems: Vec<String> = vec![raw.clone()];

    if let Some(s) = raw.strip_prefix("test_") {
        stems.push(s.to_string());
    }
    if let Some(s) = raw.strip_suffix("_test") {
        stems.push(s.to_string());
    }
    if let Some(s) = raw.strip_suffix(".test") {
        stems.push(s.to_string());
    }
    if let Some(s) = raw.strip_suffix(".spec") {
        stems.push(s.to_string());
    }
    if let Some(s) = raw.strip_suffix("_spec") {
        stems.push(s.to_string());
    }
    if phpunit_test && let Some(s) = raw.strip_suffix("test") {
        stems.push(s.to_string());
    }
    if rust_cli_test {
        stems.push("main".to_string());
    }

    let prefix = components.join("/");
    let mut keys: Vec<String> = stems
        .into_iter()
        .map(|stem| {
            if prefix.is_empty() {
                stem
            } else {
                format!("{prefix}/{stem}")
            }
        })
        .collect();

    let mut seen = std::collections::HashSet::new();
    keys.retain(|k| seen.insert(k.clone()));

    keys
}

fn is_phpunit_filename(filename: &str) -> bool {
    let Some((stem, extension)) = filename.rsplit_once('.') else {
        return false;
    };
    extension.eq_ignore_ascii_case("php")
        && stem
            .strip_suffix("Test")
            .is_some_and(|base| !base.is_empty())
}

fn stem_components(rel_path: &str) -> Vec<String> {
    let normalized = rel_path.replace('\\', "/");
    let mut components: Vec<String> = normalized
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .map(str::to_lowercase)
        .collect();

    if let Some(filename) = components.last_mut()
        && let Some(pos) = filename.rfind('.')
    {
        filename.truncate(pos);
    }

    components
}

fn strip_layout_components(components: &mut Vec<String>, is_test: bool) {
    if components.len() < 2 {
        return;
    }

    let source_layout = components
        .iter()
        .rposition(|component| component == "src")
        .or_else(|| {
            components
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, component)| {
                    (component == "lib" && is_source_layout_position(components, index))
                        .then_some(index)
                })
        });
    let test_layout = is_test.then(|| {
        components.iter().rposition(|component| {
            matches!(
                component.as_str(),
                "tests" | "test" | "__tests__" | "spec" | "specs"
            )
        })
    });

    let test_layout = test_layout
        .flatten()
        .filter(|test_index| source_layout.is_none_or(|source_index| *test_index > source_index));

    let mut remove: Vec<usize> = [source_layout, test_layout].into_iter().flatten().collect();
    remove.sort_unstable_by(|a, b| b.cmp(a));
    remove.dedup();
    for index in remove {
        components.remove(index);
    }
}

fn is_source_layout_position(components: &[String], index: usize) -> bool {
    index == 0
        || (index == 2
            && matches!(
                components.first().map(String::as_str),
                Some("packages" | "crates" | "apps" | "services" | "modules")
            ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::FirstClass;
    use crate::parse;

    fn detect_fixture_frameworks<'a>(
        directory: &Path,
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Vec<TestFramework> {
        let mut candidates = Vec::new();
        for (report_path, content) in files {
            let absolute_path = directory.join(report_path);
            std::fs::create_dir_all(absolute_path.parent().unwrap()).unwrap();
            std::fs::write(&absolute_path, content).unwrap();
            candidates.push((absolute_path, Path::new(report_path).to_path_buf()));
        }
        detect_frameworks(
            candidates
                .iter()
                .map(|(absolute, report)| (absolute.as_path(), report.as_path())),
            64 * 1024,
        )
    }

    #[test]
    fn detects_every_supported_runner_from_discovery_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let cases = [
            (
                "rust/Cargo.toml",
                "[package]\nname='fixture'\nversion='0.1.0'\n",
                "cargo-test",
            ),
            ("go/go.mod", "module example.com/fixture\n", "go-test"),
            ("python/pytest.ini", "[pytest]\n", "pytest"),
            ("php/phpunit.xml", "<phpunit/>\n", "phpunit"),
            ("vitest/vitest.config.ts", "export default {}\n", "vitest"),
            ("jest/jest.config.js", "module.exports = {}\n", "jest"),
            (
                "bun/package.json",
                r#"{"scripts":{"test":"bun test"}}"#,
                "bun-test",
            ),
            (
                "node/package.json",
                r#"{"scripts":{"test":"node --test"}}"#,
                "node-test",
            ),
        ];
        let detected = detect_fixture_frameworks(
            directory.path(),
            cases
                .iter()
                .map(|(report_path, content, _)| (*report_path, *content)),
        );

        for (report_path, _, framework) in cases {
            assert!(
                detected
                    .iter()
                    .any(|item| { item.name == framework && item.evidence == report_path }),
                "missing {framework} from {report_path}: {detected:?}"
            );
        }
    }

    #[test]
    fn malformed_and_irrelevant_manifests_do_not_establish_runners() {
        let directory = tempfile::tempdir().unwrap();
        let cases = [
            ("broken/package.json", "{"),
            ("broken/composer.json", "{"),
            ("broken/pyproject.toml", "[project"),
            (
                "metadata/package.json",
                r#"{"description":"run jest and pytest here"}"#,
            ),
            (
                "metadata/scripts/package.json",
                r#"{"scripts":{"test":"echo vitest && echo jest && echo bun test && echo node --test"}}"#,
            ),
            (
                "metadata/node-options/package.json",
                r#"{"scripts":{"test":"node --test-reporter spec"}}"#,
            ),
            (
                "metadata/quoted/package.json",
                r#"{"scripts":{"test":"echo 'vitest; jest | bun test & node --test'"}}"#,
            ),
            ("metadata/flags/package.json", r#"{"jest":false}"#),
            (
                "metadata/composer.json",
                r#"{"description":"phpunit/phpunit"}"#,
            ),
            (
                "metadata/pyproject.toml",
                "[project]\nname='fixture'\ndescription='pytest is great'\ndependencies=['pytest-cov']\n# pytest\n",
            ),
            ("metadata/vitest.config.example.ts", "export default {}\n"),
        ];
        let detected = detect_fixture_frameworks(directory.path(), cases);

        assert!(detected.is_empty(), "unexpected runners: {detected:?}");
    }

    #[test]
    fn package_script_detection_is_command_position_aware() {
        for script in [
            "vitest run",
            "npx vitest run",
            "pnpm exec vitest run",
            "npm exec -- jest",
            "cross-env NODE_ENV=test bun test",
            "node --test",
        ] {
            assert!(
                script_invokes(script, "vitest", |_| true)
                    || script_invokes(script, "jest", |_| true)
                    || script_invokes(script, "bun", |arguments| arguments.first()
                        == Some(&"test"))
                    || script_invokes(script, "node", |arguments| arguments.contains(&"--test")),
                "missed runner command: {script}"
            );
        }

        for script in [
            "echo vitest",
            "echo 'vitest; jest | bun test & node --test'",
            "pnpm --filter vitest lint",
            "npx --package vitest echo ready",
            "node --test-reporter spec",
        ] {
            assert!(!script_invokes(script, "vitest", |_| true), "{script}");
            assert!(!script_invokes(script, "jest", |_| true), "{script}");
            assert!(
                !script_invokes(script, "bun", |arguments| arguments.first()
                    == Some(&"test")),
                "{script}"
            );
            assert!(
                !script_invokes(script, "node", |arguments| {
                    arguments
                        .iter()
                        .any(|argument| *argument == "--test" || argument.starts_with("--test="))
                }),
                "{script}"
            );
        }
    }

    #[test]
    fn pyproject_detection_reads_only_supported_dependency_locations() {
        let positives = [
            "[project]\ndependencies=['pytest>=8']\n",
            "[project.optional-dependencies]\ntest=['pytest[testing]']\n",
            "[dependency-groups]\ntest=['pytest~=8.0']\n",
            "[tool.poetry.group.test.dependencies]\npytest='^8'\n",
            "[tool.pdm.dev-dependencies]\ntest=['pytest']\n",
            "[tool.uv]\ndev-dependencies=['pytest']\n",
            "[tool.hatch.envs.test]\ndependencies=['pytest']\n",
            "[tool.pytest.ini_options]\naddopts='-q'\n",
        ];
        for content in positives {
            let parsed = toml::from_str::<toml::Value>(content);
            assert!(
                parsed.is_ok(),
                "invalid positive fixture: {content}\n{parsed:?}"
            );
            let mut found = Vec::new();
            detect_pyproject(content, "pyproject.toml", &mut found);
            assert_eq!(found.len(), 1, "failed to detect pytest in:\n{content}");
            assert_eq!(found[0].name, "pytest");
        }

        let negatives = [
            "[project]\ndescription='pytest'\n",
            "[project]\ndependencies=['pytest-cov']\n",
            "[tool.example]\ncommand='pytest'\n",
            "[tool]\npytest='mentioned but not configuration'\n",
            "# pytest\n[project]\nname='fixture'\n",
        ];
        for content in negatives {
            let mut found = Vec::new();
            detect_pyproject(content, "pyproject.toml", &mut found);
            assert!(found.is_empty(), "false pytest detection in:\n{content}");
        }
    }

    #[test]
    fn runner_default_filename_patterns_are_scoped_and_table_driven() {
        let cases = [
            ("cargo-test", "Cargo.toml", "tests/api.rs", true),
            ("cargo-test", "Cargo.toml", "tests/api/main.rs", true),
            ("cargo-test", "Cargo.toml", "tests/common/mod.rs", false),
            ("cargo-test", "Cargo.toml", "Tests/api.rs", false),
            ("go-test", "go.mod", "internal/user_test.go", true),
            ("go-test", "go.mod", "internal/user.go", false),
            ("go-test", "go.mod", "internal/user_TEST.go", false),
            ("pytest", "pyproject.toml", "tests/test_user.py", true),
            ("pytest", "pyproject.toml", "tests/user_test.py", true),
            ("pytest", "pyproject.toml", "tests/helpers.py", false),
            ("pytest", "pyproject.toml", "tests/conftest.py", false),
            ("pytest", "pyproject.toml", "tests/Test_user.py", false),
            ("phpunit", "composer.json", "tests/UserTest.php", true),
            ("phpunit", "composer.json", "tests/scenario.phpt", true),
            ("phpunit", "composer.json", "tests/bootstrap.php", false),
            ("phpunit", "composer.json", "tests/usertest.php", false),
            ("jest", "package.json", "src/user.test.ts", true),
            ("jest", "package.json", "src/__tests__/user.tsx", true),
            ("jest", "package.json", "src/user.test.mjs", false),
            ("jest", "package.json", "src/user.Test.ts", false),
            ("vitest", "package.json", "src/user.spec.mts", true),
            ("vitest", "package.json", "src/__tests__/user.ts", false),
            ("vitest", "package.json", "src/user.test.helper.ts", false),
            ("bun-test", "package.json", "src/user_spec.tsx", true),
            ("bun-test", "package.json", "src/__tests__/user.ts", false),
            ("bun-test", "package.json", "src/user_spec.helper.ts", false),
            ("node-test", "package.json", "test/helper.js", true),
            ("node-test", "package.json", "src/test-user.mjs", true),
            ("node-test", "package.json", "src/user-test.cjs", true),
            ("node-test", "package.json", "src/user.spec.js", false),
            ("node-test", "package.json", "test/helper.ts", false),
        ];
        for (name, evidence, path, expected) in cases {
            let frameworks = [TestFramework {
                name: name.to_string(),
                evidence: evidence.to_string(),
            }];
            assert_eq!(
                is_framework_test_file(&frameworks, path),
                expected,
                "{name} classified {path} incorrectly"
            );
        }

        let scoped = [TestFramework {
            name: "pytest".to_string(),
            evidence: "packages/api/pyproject.toml".to_string(),
        }];
        assert!(is_framework_test_file(
            &scoped,
            "packages/api/tests/test_user.py"
        ));
        assert!(!is_framework_test_file(
            &scoped,
            "packages/web/tests/test_user.py"
        ));
    }

    // ── is_test_file ──────────────────────────────────────────────────────────

    #[test]
    fn detects_file_in_tests_dir() {
        assert!(is_test_file("tests/cli.rs"));
        assert!(is_test_file("tests/fixtures/sample.py"));
        assert!(is_test_file("src/tests/helpers.rs"));
    }

    #[test]
    fn detects_test_dir_variants() {
        assert!(is_test_file("test/main.py"));
        assert!(is_test_file("__tests__/utils.js"));
        assert!(is_test_file("spec/models/user_spec.rb"));
        assert!(is_test_file("specs/api_spec.rb"));
        assert!(is_test_file("Tests/cli.rs"));
    }

    #[test]
    fn detects_test_prefix_in_stem() {
        assert!(is_test_file("test_foo.py"));
        assert!(is_test_file("src/test_bar.go"));
    }

    #[test]
    fn detects_test_suffix_in_stem() {
        assert!(is_test_file("foo_test.go"));
        assert!(is_test_file("foo_test.py"));
        assert!(is_test_file("foo_test.rs"));
    }

    #[test]
    fn detects_rust_split_test_module_filename() {
        assert!(is_test_file("src/metrics/complexity/tests.rs"));
        assert!(is_test_file("src/tests.rs"));
    }

    #[test]
    fn detects_dot_test_and_dot_spec() {
        assert!(is_test_file("x.test.ts"));
        assert!(is_test_file("foo.spec.js"));
        assert!(is_test_file("src/component.test.tsx"));
    }

    #[test]
    fn detects_spec_rb_suffix() {
        assert!(is_test_file("user_spec.rb"));
        assert!(is_test_file("models/user_spec.rb"));
    }

    #[test]
    fn detects_phpunit_test_suffix_without_misclassifying_words() {
        assert!(is_test_file("app/Domain/UserServiceTest.php"));
        assert!(is_test_file("app/Domain/APIClientTest.PHP"));
        assert!(!is_test_file("app/Domain/Contest.php"));
    }

    #[test]
    fn regular_files_are_not_tests() {
        assert!(!is_test_file("foo.py"));
        assert!(!is_test_file("src/main.rs"));
        assert!(!is_test_file("src/scan.rs"));
        assert!(!is_test_file("utils/helpers.go"));
        assert!(!is_test_file("lib/user.rb"));
    }

    #[test]
    fn rust_inline_tests_use_attributes_not_substrings() {
        let positives = [
            "#[test]\nfn works() {}\n",
            "#[cfg ( all(feature = \"x\", test) )]\nmod tests {}\n",
            "#[cfg(any(not(windows), test))]\nmod tests {}\n",
            "#[cfg(not(not(test)))]\nmod tests {}\n",
            "#[tokio::test]\nasync fn works() {}\n",
        ];
        for source in positives {
            let tree = parse::parse(FirstClass::Rust, source).unwrap();
            assert!(
                has_inline_rust_tests(source, &tree),
                "{source}\n{}",
                tree.root_node().to_sexp()
            );
        }

        let source = r##"
// #[test]
const EXAMPLE: &str = "#[cfg(test)]";
#[cfg(feature = "test")]
fn production() {}
#[cfg(not(test))]
fn non_test_build() {}
#[cfg(all(feature = "x", not(test)))]
fn also_non_test_build() {}
"##;
        let tree = parse::parse(FirstClass::Rust, source).unwrap();
        assert!(!has_inline_rust_tests(source, &tree));
    }

    #[test]
    fn rust_test_regions_only_include_test_only_items() {
        let source = r"
pub fn production() {}

#[cfg(any(test, unix))]
fn also_production() {}

#[cfg(test)]
mod tests {
    #[test]
    fn works() {}
}
";
        let tree = parse::parse(FirstClass::Rust, source).unwrap();
        let ranges = inline_rust_test_regions(source, &tree);

        assert_eq!(ranges.len(), 1, "{ranges:?}");
        assert_eq!(ranges[0].start, 7);
        assert_eq!(ranges[0].end, 11);
    }

    #[test]
    fn normalises_backslashes() {
        assert!(is_test_file("src\\tests\\helper.rs"));
        assert!(is_test_file("foo_test.py"));
    }

    // ── source_stem ───────────────────────────────────────────────────────────

    #[test]
    fn source_stem_single_extension() {
        assert_eq!(source_stem("Foo.ts"), "foo");
        assert_eq!(source_stem("main.rs"), "main");
        assert_eq!(source_stem("helpers.go"), "helpers");
    }

    #[test]
    fn source_stem_multi_extension() {
        assert_eq!(source_stem("bar.test.ts"), "bar.test");
        assert_eq!(source_stem("foo.spec.js"), "foo.spec");
    }

    #[test]
    fn source_stem_path_component() {
        assert_eq!(source_stem("src/lib/utils.rs"), "lib/utils");
    }

    #[test]
    fn keys_match_nested_source_and_test_layouts() {
        let source = source_stem("src/domain/parser.ts");
        let tests = test_stem_keys("tests/domain/parser.test.ts");

        assert_eq!(source, "domain/parser");
        assert!(tests.contains(&source));
    }

    #[test]
    fn keys_match_colocated_tests() {
        assert_eq!(
            source_stem("src/components/Button.tsx"),
            "components/button"
        );
        assert!(
            test_stem_keys("src/components/Button.test.tsx")
                .contains(&"components/button".to_string())
        );
    }

    #[test]
    fn keys_preserve_monorepo_package_prefixes() {
        assert_eq!(
            source_stem("packages/web/src/api/client.ts"),
            "packages/web/api/client"
        );
        assert!(
            test_stem_keys("packages/web/tests/api/client.spec.ts")
                .contains(&"packages/web/api/client".to_string())
        );
        assert!(
            !test_stem_keys("packages/server/tests/api/client.spec.ts")
                .contains(&"packages/web/api/client".to_string())
        );
    }

    #[test]
    fn package_named_lib_is_not_removed_as_a_layout_directory() {
        let source = source_stem("packages/lib/src/user.ts");
        let tests = test_stem_keys("packages/lib/tests/user.test.ts");

        assert_eq!(source, "packages/lib/user");
        assert!(tests.contains(&source), "test keys were: {tests:?}");
    }

    // ── test_stem_keys ────────────────────────────────────────────────────────

    #[test]
    fn rust_cli_integration_tests_match_the_package_entrypoint() {
        assert!(test_stem_keys("tests/cli.rs").contains(&source_stem("src/main.rs")));
        assert!(
            test_stem_keys("crates/worker/tests/cli.rs")
                .contains(&source_stem("crates/worker/src/main.rs"))
        );
        assert!(!test_stem_keys("src/cli.rs").contains(&source_stem("src/main.rs")));
    }

    #[test]
    fn keys_for_test_prefix() {
        let keys = test_stem_keys("test_foo.py");
        assert!(keys.contains(&"test_foo".to_string()));
        assert!(keys.contains(&"foo".to_string()));
        assert_eq!(keys[0], "test_foo");
    }

    #[test]
    fn keys_for_test_suffix() {
        let keys = test_stem_keys("foo_test.go");
        assert!(keys.contains(&"foo_test".to_string()));
        assert!(keys.contains(&"foo".to_string()));
    }

    #[test]
    fn keys_for_dot_test() {
        let keys = test_stem_keys("Foo.test.ts");
        assert!(keys.contains(&"foo.test".to_string()));
        assert!(keys.contains(&"foo".to_string()));
    }

    #[test]
    fn keys_for_spec_rb() {
        let keys = test_stem_keys("user_spec.rb");
        assert!(keys.contains(&"user_spec".to_string()));
        assert!(keys.contains(&"user".to_string()));
    }

    #[test]
    fn phpunit_keys_match_camel_case_source_files() {
        let source = source_stem("src/Domain/UserService.php");
        let keys = test_stem_keys("tests/Domain/UserServiceTest.php");

        assert_eq!(source, "domain/userservice");
        assert!(keys.contains(&source), "test keys were: {keys:?}");
    }

    #[test]
    fn keys_no_duplicates() {
        // If raw stem already equals a stripped variant, no duplicate.
        let keys = test_stem_keys("test.ts");
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(
            keys.len(),
            unique.len(),
            "test_stem_keys must not contain duplicates"
        );
    }
}
