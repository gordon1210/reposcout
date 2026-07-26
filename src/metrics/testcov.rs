//! Test-presence heuristics: classify files as tests vs source code and
//! compute stem keys for matching test files to the source files they cover.

use tree_sitter::{Node, Tree};

/// Return whether a parsed Rust file contains an inline test attribute.
///
/// Attribute nodes are inspected instead of raw source text so examples in
/// comments and strings cannot mark a source file as tested.
pub fn has_inline_rust_tests(content: &str, tree: &Tree) -> bool {
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "attribute" && is_test_attribute(node, content) {
            return true;
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index as u32) {
                stack.push(child);
            }
        }
    }
    false
}

fn is_test_attribute(node: Node<'_>, content: &str) -> bool {
    let mut cursor = node.walk();
    let Some(name) = node
        .named_children(&mut cursor)
        .find(|child| child.kind() != "token_tree")
        .and_then(|child| child.utf8_text(content.as_bytes()).ok())
    else {
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

/// Returns `true` if `rel_path` looks like a test file.
///
/// A file is classified as a test if:
/// - any path component (directory or filename, after normalising `\` to `/`)
///   is one of `tests`, `test`, `__tests__`, `spec`, or `specs`; OR
/// - the filename stem starts with `test_`, ends with `_test`, or the filename
///   contains `.test.` or `.spec.`; OR
/// - the filename ends with `_spec.rb`; OR
/// - a PHP filename follows PHPUnit's `SomethingTest.php` convention.
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
///
/// The raw stem is always the first element; duplicates are removed while
/// preserving order.
pub fn test_stem_keys(rel_path: &str) -> Vec<String> {
    let phpunit_test = rel_path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .is_some_and(is_phpunit_filename);
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

    let test_layout = test_layout.flatten().filter(|test_index| {
        source_layout
            .map(|source_index| *test_index > source_index)
            .unwrap_or(true)
    });

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
