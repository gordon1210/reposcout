//! Whether a renamed token is provably a pure rename of a plain binding: a plain identifier
//! position on both sides in a reviewed grammar, bound in its own file by a declaration or an
//! in-tree import on both sides, one-to-one with the other renames of its files, a new name
//! absent from every base blob, an old name absent from every head blob, and no
//! language-specific meaning in the change of spelling.

use super::bindings::{self, Bindings};

use crate::lang::FirstClass;
use crate::model::ReviewContextChange;
use crate::scan::ReviewCapture;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::{OnceCell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;
use tree_sitter::{Node, Tree};

/// Blob bytes scanned per revision tree; a larger tree proves no rename.
const MAX_TREE_BYTES: usize = 64 * 1024 * 1024;
/// Distinct identifiers checked against the trees; any further identifier proves no rename.
const MAX_IDENTIFIERS: usize = 4096;

/// One token position: a line of a change's base or head file and the token's byte offset in it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Site {
    pub change: usize,
    /// One-based.
    pub line: usize,
    pub byte: usize,
}

/// One renamed token: `old` at `base` became `new` at `head`, both complete tokens.
#[derive(Debug, Clone, Copy)]
pub(super) struct Rename<'a> {
    pub base: Site,
    pub head: Site,
    pub old: &'a str,
    pub new: &'a str,
}

/// A parsed file with its grammar and the byte offset of each line start.
struct Parsed {
    grammar: FirstClass,
    tree: Tree,
    starts: Vec<usize>,
}

/// Parsed files by change and side; `None` for files without content or grammar.
type Trees = FxHashMap<(usize, usize), Option<Rc<Parsed>>>;
/// File bindings by change and side; `None` for files without them.
type Bound = FxHashMap<(usize, usize), Option<Rc<Bindings>>>;

/// Where a renamed token sits: a plain binding position, or the original-name side of an import
/// or re-export specifier, with whether that specifier names an in-tree module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    Plain,
    Specifier(bool),
}

pub(super) struct Lexicon<'a> {
    capture: &'a ReviewCapture,
    changes: &'a [ReviewContextChange],
    /// Per change: a test file on either side.
    tests: &'a [bool],
    /// Every word of every blob of the base and head tree; `None` above the scan bound.
    words: [OnceCell<Option<FxHashSet<String>>>; 2],
    checked: RefCell<FxHashSet<String>>,
    trees: RefCell<Trees>,
    bound: RefCell<Bound>,
    /// Per change: every whole-identifier rename of its hunks.
    renames: &'a [Vec<(String, String)>],
    /// Dotted Python module paths of the captured files of each side, every suffix included.
    modules: [OnceCell<FxHashSet<String>>; 2],
}

impl<'a> Lexicon<'a> {
    pub(super) fn new(
        capture: &'a ReviewCapture,
        changes: &'a [ReviewContextChange],
        tests: &'a [bool],
        renames: &'a [Vec<(String, String)>],
    ) -> Self {
        Self {
            capture,
            changes,
            tests,
            words: [OnceCell::new(), OnceCell::new()],
            checked: RefCell::default(),
            trees: RefCell::default(),
            bound: RefCell::default(),
            renames,
            modules: [OnceCell::new(), OnceCell::new()],
        }
    }

    /// Both tokens sit at plain binding positions of reviewed grammars; the old name is bound
    /// in the base file and the new name in the head file of their sites, both by a
    /// declaration or both by an import from an in-tree module (a specifier token only by
    /// in-tree specifiers), and neither is bound outside the tree there; the rename is
    /// one-to-one with every other rename of both files; the spelling change has no
    /// language-specific meaning; the new name occurs in no base blob and the old name in no
    /// head blob.
    pub(super) fn pure(&self, rename: &Rename<'_>) -> bool {
        let (Some(base), Some(head)) = (
            self.parsed(rename.base.change, 0),
            self.parsed(rename.head.change, 1),
        ) else {
            return false;
        };
        let test = self.tests.get(rename.base.change).copied().unwrap_or(true)
            || self.tests.get(rename.head.change).copied().unwrap_or(true);
        if !renamable(rename.old, rename.new, [base.grammar, head.grammar], test) {
            return false;
        }
        let (Some(old_at), Some(new_at)) = (
            self.position(rename.base, 0, rename.old),
            self.position(rename.head, 1, rename.new),
        ) else {
            return false;
        };
        let (Some(old_bound), Some(new_bound)) = (
            self.bindings(rename.base.change, 0),
            self.bindings(rename.head.change, 1),
        ) else {
            return false;
        };
        let (old_by, new_by) = (old_bound.of(rename.old), new_bound.of(rename.new));
        let bound = match (old_at, new_at) {
            (Position::Plain, Position::Plain) => old_by & new_by != 0,
            (Position::Specifier(old_in_tree), Position::Specifier(new_in_tree)) => {
                old_in_tree
                    && new_in_tree
                    && old_by & bindings::IMPORTED != 0
                    && new_by & bindings::IMPORTED != 0
            }
            _ => false,
        };
        bound
            && self.one_to_one(rename.base.change, rename.old, rename.new)
            && self.one_to_one(rename.head.change, rename.old, rename.new)
            && self.absent(0, rename.new)
            && self.absent(1, rename.old)
    }

    /// Every rename of the change's hunks maps `old` to `new` exactly when it maps one of them.
    fn one_to_one(&self, change: usize, old: &str, new: &str) -> bool {
        self.renames.get(change).is_none_or(|renames| {
            renames
                .iter()
                .all(|(other_old, other_new)| (other_old == old) == (other_new == new))
        })
    }

    fn bindings(&self, change: usize, side: usize) -> Option<Rc<Bindings>> {
        if let Some(found) = self.bound.borrow().get(&(change, side)) {
            return found.clone();
        }
        let found = (|| {
            let (_, content) = self.content(change, side)?;
            let parsed = self.parsed(change, side)?;
            let module = |path: &str| self.module(side, path);
            bindings::bindings(parsed.grammar, &parsed.tree, content, &module).map(Rc::new)
        })();
        self.bound
            .borrow_mut()
            .insert((change, side), found.clone());
        found
    }

    /// A dotted Python module path names a captured `.py`/`.pyi` file or package of the side.
    fn module(&self, side: usize, path: &str) -> bool {
        self.modules[side]
            .get_or_init(|| {
                let mut modules = FxHashSet::default();
                for file in self.capture.revisions[side].files().keys() {
                    if !file
                        .extension()
                        .is_some_and(|extension| extension == "py" || extension == "pyi")
                    {
                        continue;
                    }
                    let mut parts: Vec<String> = file
                        .with_extension("")
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    if parts.last().is_some_and(|last| last == "__init__") {
                        parts.pop();
                    }
                    for start in 0..parts.len() {
                        modules.insert(parts[start..].join("."));
                    }
                }
                modules
            })
            .contains(path)
    }

    /// A declaration moved from the base lines `moved` of `source` to the head lines `lines`
    /// (one-based) of `target` keeps the meaning of its names: its enclosing containers have
    /// byte-identical headers on both sides, and every top-level statement of the target head
    /// file outside the `folded` line ranges, imports aside, that declares a name the block
    /// uses also occurs byte for byte as a top-level statement of the source base file.
    pub(super) fn keeps_bindings(
        &self,
        source: usize,
        target: usize,
        moved: &[usize],
        lines: &[usize],
        folded: &[(usize, usize)],
    ) -> bool {
        let (Some((_, head)), Some((_, base))) = (self.content(target, 1), self.content(source, 0))
        else {
            return false;
        };
        let (Some(head_tree), Some(base_tree)) = (self.parsed(target, 1), self.parsed(source, 0))
        else {
            return false;
        };
        match (
            containers(base, &base_tree, moved),
            containers(head, &head_tree, lines),
        ) {
            (Some(before), Some(after)) if before == after => {}
            _ => return false,
        }
        let mut used = FxHashSet::default();
        for &line in lines {
            if let Some(text) = line_text(head, &head_tree.starts, line) {
                names_in(text.as_bytes(), &mut used);
            }
        }
        let root = base_tree.tree.root_node();
        let mut cursor = root.walk();
        let originals: FxHashSet<&str> = root
            .named_children(&mut cursor)
            .filter_map(|node| base.get(node.byte_range()))
            .collect();
        let root = head_tree.tree.root_node();
        let mut cursor = root.walk();
        root.named_children(&mut cursor).all(|statement| {
            let (first, last) = (
                statement.start_position().row + 1,
                statement.end_position().row + 1,
            );
            if folded
                .iter()
                .any(|&(start, end)| first <= end && start <= last)
                || imports(statement)
            {
                return true;
            }
            let mut declared = Vec::new();
            declarations(statement, 0, &mut declared);
            let rebinds = declared.iter().any(|node| {
                head.get(node.byte_range())
                    .is_some_and(|name| used.contains(name))
            });
            !rebinds
                || head
                    .get(statement.byte_range())
                    .is_some_and(|text| originals.contains(text))
        })
    }

    fn content(&self, change: usize, side: usize) -> Option<(&'a Path, &'a str)> {
        let changed = [&self.changes[change].base, &self.changes[change].head][side].as_ref()?;
        let file = self.capture.revisions[side].files().get(&changed.path)?;
        Some((changed.path.as_path(), file.content.as_str()))
    }

    /// `name` occurs as a word in no blob of the side's tree.
    fn absent(&self, side: usize, name: &str) -> bool {
        {
            let mut checked = self.checked.borrow_mut();
            if !checked.contains(name) {
                if checked.len() >= MAX_IDENTIFIERS {
                    return false;
                }
                checked.insert(name.to_string());
            }
        }
        self.words[side]
            .get_or_init(|| self.tree_words(side))
            .as_ref()
            .is_some_and(|words| !words.contains(name))
    }

    fn tree_words(&self, side: usize) -> Option<FxHashSet<String>> {
        tree_words(
            &self.capture.root,
            &self.capture.revisions[side].tree,
            self.capture.deadline,
        )
    }

    fn parsed(&self, change: usize, side: usize) -> Option<Rc<Parsed>> {
        self.trees
            .borrow_mut()
            .entry((change, side))
            .or_insert_with(|| {
                let (path, content) = self.content(change, side)?;
                let grammar = crate::lang::detect(path)?.first_class?;
                let tree = crate::parse::parse(grammar, content)?;
                let starts = std::iter::once(0)
                    .chain(content.match_indices('\n').map(|(index, _)| index + 1))
                    .collect();
                Some(Rc::new(Parsed {
                    grammar,
                    tree,
                    starts,
                }))
            })
            .clone()
    }

    /// Where `token` sits at `site` as a whole plain binding outside any parse error.
    fn position(&self, site: Site, revision: usize, token: &str) -> Option<Position> {
        let (_, content) = self.content(site.change, revision)?;
        let parsed = self.parsed(site.change, revision)?;
        let start = site
            .line
            .checked_sub(1)
            .and_then(|line| parsed.starts.get(line))
            .map(|start| start + site.byte)?;
        let end = start + token.len();
        if content.get(start..end) != Some(token) {
            return None;
        }
        let node = parsed
            .tree
            .root_node()
            .descendant_for_byte_range(start, end)?;
        if node.start_byte() != start
            || node.end_byte() != end
            || !plain_binding(parsed.grammar, node)
            || intrinsic_tag(node, token)
            || erroneous(node)
        {
            return None;
        }
        let module = |path: &str| self.module(revision, path);
        Some(
            bindings::specifier(parsed.grammar, node, content, &module)
                .map_or(Position::Plain, Position::Specifier),
        )
    }
}

fn line_text<'t>(content: &'t str, starts: &[usize], line: usize) -> Option<&'t str> {
    let start = *starts.get(line.checked_sub(1)?)?;
    let end = starts.get(line).map_or(content.len(), |next| next - 1);
    content.get(start..end)
}

/// Every word of every blob of `tree`, or `None` when the repository or tree cannot be read,
/// the blobs exceed the scan bound or the deadline passes.
fn tree_words(root: &Path, tree: &str, deadline: Instant) -> Option<FxHashSet<String>> {
    let repo = git2::Repository::open(root).ok()?;
    let tree = repo.find_tree(git2::Oid::from_str(tree).ok()?).ok()?;
    let odb = repo.odb().ok()?;
    let mut words = FxHashSet::default();
    let mut bytes = 0usize;
    let mut complete = true;
    let walked = tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return git2::TreeWalkResult::Ok;
        }
        let size = odb.read_header(entry.id()).map(|(size, _)| size);
        bytes = bytes.saturating_add(size.unwrap_or(usize::MAX));
        if bytes > MAX_TREE_BYTES || Instant::now() >= deadline {
            complete = false;
            return git2::TreeWalkResult::Abort;
        }
        let Ok(blob) = repo.find_blob(entry.id()) else {
            complete = false;
            return git2::TreeWalkResult::Abort;
        };
        names_in(blob.content(), &mut words);
        git2::TreeWalkResult::Ok
    });
    (complete && walked.is_ok()).then_some(words)
}

/// Both names are ASCII and the spelling change carries no language-specific meaning: Go
/// export visibility, Python privacy and special methods, or test discovery in test files.
fn renamable(old: &str, new: &str, grammars: [FirstClass; 2], test: bool) -> bool {
    let upper = |name: &str| name.chars().next().is_some_and(char::is_uppercase);
    let private = |name: &str| name.starts_with('_');
    let dunder = |name: &str| name.len() > 4 && name.starts_with("__") && name.ends_with("__");
    let affix = |name: &str| {
        let name = name.to_ascii_lowercase();
        name.starts_with("test") || name.ends_with("test") || name.ends_with("tests")
    };
    old.is_ascii()
        && new.is_ascii()
        && !(grammars.contains(&FirstClass::Go) && upper(old) != upper(new))
        && !(grammars.contains(&FirstClass::Python)
            && (dunder(old) || dunder(new) || private(old) != private(new)))
        && !(test && affix(old) != affix(new))
}

/// Headers of the containers enclosing the declaration that starts on the first of `lines`,
/// outermost first: the text of each ancestor with a body before that body, trimmed.
fn containers<'c>(content: &'c str, parsed: &Parsed, lines: &[usize]) -> Option<Vec<&'c str>> {
    let first = lines.iter().min()?.checked_sub(1)?;
    let line = parsed.starts.get(first).copied()?;
    let start = line + content.get(line..)?.find(|ch: char| !ch.is_whitespace())?;
    let mut node = parsed
        .tree
        .root_node()
        .descendant_for_byte_range(start, start)?;
    let mut headers = Vec::new();
    while let Some(parent) = node.parent() {
        if let Some(body) = parent.child_by_field_name("body")
            && parent.start_byte() < start
            && body.start_byte() <= start
            && start < body.end_byte()
        {
            headers.push(content.get(parent.start_byte()..body.start_byte())?.trim());
        }
        node = parent;
    }
    headers.reverse();
    Some(headers)
}

/// A lowercase JSX element name, which names an intrinsic element rather than a binding.
fn intrinsic_tag(node: Node<'_>, token: &str) -> bool {
    token.starts_with(|first: char| first.is_ascii_lowercase())
        && node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "jsx_opening_element" | "jsx_closing_element" | "jsx_self_closing_element"
            ) && is_field(node, parent, "name")
        })
}

/// `node` is the given field of its parent.
fn is_field(node: Node<'_>, parent: Node<'_>, field: &str) -> bool {
    parent
        .child_by_field_name(field)
        .is_some_and(|child| child.id() == node.id())
}

/// A plain identifier position of a reviewed grammar: no member, key, field, attribute,
/// argument-name, module-path or macro position. Unreviewed grammars have none.
fn plain_binding(grammar: FirstClass, node: Node<'_>) -> bool {
    let kind = node.kind();
    let Some(parent) = node.parent() else {
        return false;
    };
    match grammar {
        FirstClass::TypeScript | FirstClass::Tsx | FirstClass::JavaScript => {
            matches!(kind, "identifier" | "type_identifier") && !script_member(node, parent)
        }
        FirstClass::Python => kind == "identifier" && !python_member(node, parent),
        FirstClass::Rust => {
            matches!(kind, "identifier" | "type_identifier") && !rust_member(node, parent)
        }
        FirstClass::Go => {
            matches!(kind, "identifier" | "type_identifier") && !go_member(node, parent)
        }
        _ => false,
    }
}

fn script_member(node: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        kind if kind.starts_with("jsx_attribute") || kind == "jsx_namespace_name" => true,
        "pair" | "pair_pattern" => is_field(node, parent, "key"),
        "member_expression" => is_field(node, parent, "property"),
        "nested_type_identifier" | "nested_identifier" => parent
            .named_child(0)
            .is_none_or(|first| first.id() != node.id()),
        "export_specifier" => is_field(node, parent, "alias"),
        "required_parameter" | "optional_parameter" => {
            let mut cursor = parent.walk();
            parent.children(&mut cursor).any(|child| {
                matches!(
                    child.kind(),
                    "accessibility_modifier" | "readonly" | "override_modifier"
                )
            })
        }
        _ => false,
    }
}

fn python_member(node: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        "attribute" => return is_field(node, parent, "attribute"),
        "keyword_argument" => return is_field(node, parent, "name"),
        "keyword_pattern" => return true,
        "function_definition" | "class_definition" if is_field(node, parent, "name") => {
            let container = parent
                .parent()
                .filter(|outer| outer.kind() == "decorated_definition")
                .map_or(parent.parent(), |outer| outer.parent());
            return container.is_some_and(class_body);
        }
        _ => {}
    }
    // Module paths of imports.
    let mut current = node;
    while let Some(outer) = current.parent() {
        match outer.kind() {
            "dotted_name" | "relative_import" => current = outer,
            "import_from_statement" => return is_field(current, outer, "module_name"),
            "import_statement" => return current.kind() == "dotted_name",
            "aliased_import" => {
                return is_field(current, outer, "name")
                    && outer
                        .parent()
                        .is_some_and(|statement| statement.kind() == "import_statement");
            }
            _ => break,
        }
    }
    // Class attributes: assignment targets directly in a class body.
    let mut current = node;
    while let Some(outer) = current.parent() {
        match outer.kind() {
            "pattern_list" | "tuple_pattern" | "list_pattern" => current = outer,
            "assignment" if is_field(current, outer, "left") => {
                return outer
                    .parent()
                    .filter(|statement| statement.kind() == "expression_statement")
                    .and_then(|statement| statement.parent())
                    .is_some_and(class_body);
            }
            _ => return false,
        }
    }
    false
}

fn class_body(node: Node<'_>) -> bool {
    node.kind() == "block"
        && node
            .parent()
            .is_some_and(|class| class.kind() == "class_definition")
}

fn rust_member(node: Node<'_>, parent: Node<'_>) -> bool {
    let mut current = Some(parent);
    while let Some(outer) = current {
        if matches!(
            outer.kind(),
            "token_tree" | "attribute_item" | "inner_attribute_item" | "attribute"
        ) {
            return true;
        }
        current = outer.parent();
    }
    match parent.kind() {
        "shorthand_field_initializer" | "label" | "lifetime" => true,
        "enum_variant" | "scoped_identifier" | "scoped_type_identifier" => {
            is_field(node, parent, "name")
        }
        "function_item"
        | "function_signature_item"
        | "const_item"
        | "type_item"
        | "static_item" => {
            is_field(node, parent, "name")
                && parent
                    .parent()
                    .filter(|list| list.kind() == "declaration_list")
                    .and_then(|list| list.parent())
                    .is_some_and(|item| matches!(item.kind(), "impl_item" | "trait_item"))
        }
        _ => false,
    }
}

fn go_member(node: Node<'_>, parent: Node<'_>) -> bool {
    match parent.kind() {
        "literal_element" => parent.parent().is_some_and(|element| {
            element.kind() == "keyed_element" && is_field(parent, element, "key")
        }),
        "qualified_type" => is_field(node, parent, "name"),
        _ => false,
    }
}

fn erroneous(node: Node<'_>) -> bool {
    let mut current = Some(node);
    while let Some(node) = current {
        if node.is_error() || node.is_missing() {
            return true;
        }
        current = node.parent();
    }
    false
}

/// Import and package statements, whose names bind through other files.
fn imports(statement: Node<'_>) -> bool {
    matches!(
        statement.kind(),
        "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "use_declaration"
            | "extern_crate_declaration"
            | "import_declaration"
            | "package_clause"
    ) || (statement.kind() == "export_statement"
        && statement.child_by_field_name("source").is_some())
}

/// Identifier nodes a top-level statement declares: its `name`, `left` and `pattern` fields and
/// those of nested declarators, never inside bodies, values or signatures.
fn declarations<'t>(node: Node<'t>, depth: usize, out: &mut Vec<Node<'t>>) {
    const DECLARING: [&str; 3] = ["name", "left", "pattern"];
    const SKIPPED: [&str; 10] = [
        "body",
        "value",
        "right",
        "parameters",
        "type",
        "return_type",
        "result",
        "arguments",
        "type_parameters",
        "superclass",
    ];
    if depth > 4 {
        return;
    }
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        if !child.is_named() {
            continue;
        }
        let field = u32::try_from(index)
            .ok()
            .and_then(|index| node.field_name_for_child(index));
        match field {
            Some(field) if DECLARING.contains(&field) => identifiers(child, out),
            Some(field) if SKIPPED.contains(&field) => {}
            _ => declarations(child, depth + 1, out),
        }
    }
}

fn identifiers<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    if node.kind().contains("identifier") {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        identifiers(child, out);
    }
}

/// Every run of letters, digits, `_`, `$` and non-ASCII bytes in `text` that does not start
/// with a digit, the run without its leading `$`s and each of its `$`-separated parts.
fn names_in(text: &[u8], names: &mut FxHashSet<String>) {
    let word =
        |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80;
    let mut add = |name: &[u8]| {
        if name.first().is_some_and(|byte| !byte.is_ascii_digit()) {
            let name = String::from_utf8_lossy(name);
            if !names.contains(name.as_ref()) {
                names.insert(name.into_owned());
            }
        }
    };
    for run in text
        .split(|&byte| !word(byte))
        .filter(|run| !run.is_empty())
    {
        add(run);
        let trimmed = run
            .iter()
            .position(|&byte| byte != b'$')
            .map_or(&[][..], |start| &run[start..]);
        add(trimmed);
        if run.contains(&b'$') {
            for part in run.split(|&byte| byte == b'$') {
                add(part);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_whole_identifier_runs() {
        let mut names = FxHashSet::default();
        names_in(
            "const getUsers = $user + x2 + 3d + a$b;\n// account".as_bytes(),
            &mut names,
        );
        for name in [
            "const", "getUsers", "$user", "user", "x2", "account", "a$b", "a", "b",
        ] {
            assert!(names.contains(name), "{name}");
        }
        for name in ["getUser", "3d", "d", "Users"] {
            assert!(!names.contains(name), "{name}");
        }
    }

    /// Whether each occurrence of `needle` in `source` is a plain binding, in source order.
    fn bindings(grammar: FirstClass, source: &str, needle: &str) -> Vec<bool> {
        let tree = crate::parse::parse(grammar, source).unwrap();
        source
            .match_indices(needle)
            .filter(|(start, _)| {
                let before = source[..*start].chars().next_back();
                let after = source[start + needle.len()..].chars().next();
                !before.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                    && !after.is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
            })
            .map(|(start, _)| {
                let node = tree
                    .root_node()
                    .descendant_for_byte_range(start, start + needle.len())
                    .unwrap();
                node.start_byte() == start
                    && node.end_byte() == start + needle.len()
                    && plain_binding(grammar, node)
            })
            .collect()
    }

    const SCRIPT: &str = "const { user } = props;\n\
        post({ user, a: user, user: 1 });\n\
        const v = res.user;\n\
        class K { constructor(private user: U, user2: U) {} user() {} user = 1; }\n\
        import { user as u } from './x';\n\
        export { u as user };\n\
        let t: user.Type = user;\n";

    #[test]
    fn script_members_keys_and_parameter_properties_are_no_bindings() {
        for grammar in [FirstClass::TypeScript, FirstClass::Tsx] {
            assert_eq!(
                bindings(grammar, SCRIPT, "user"),
                [
                    false, // destructuring shorthand pattern
                    false, // shorthand property
                    true,  // pair value
                    false, // pair key
                    false, // member property
                    false, // parameter property
                    false, // method name
                    false, // field name
                    true,  // imported name
                    false, // export alias
                    true,  // namespace of a nested type
                    true,  // plain value
                ],
                "{grammar:?}"
            );
            assert_eq!(bindings(grammar, SCRIPT, "Type"), [false]);
        }
        let jsx = "const x = <Avatar user={user} />;\n";
        assert_eq!(bindings(FirstClass::Tsx, jsx, "user"), [false, true]);
        assert_eq!(bindings(FirstClass::Tsx, jsx, "Avatar"), [true]);
    }

    #[test]
    fn javascript_members_keys_and_attributes_are_no_bindings() {
        let source = "const { user } = props;\n\
            post({ user, a: user, user: 1 }, res.user);\n\
            const x = <Avatar user={user} />;\n";
        assert_eq!(
            bindings(FirstClass::JavaScript, source, "user"),
            [false, false, true, false, false, false, true]
        );
    }

    #[test]
    fn python_attributes_keywords_class_members_and_module_paths_are_no_bindings() {
        let source = "from user import user as u\n\
            import user.mod\n\
            mailer.send(user=user)\n\
            obj.user\n\
            class C:\n    user: int = 1\n    def user(self): pass\n\
            def user(): pass\n\
            match v:\n    case P(user=0): pass\n";
        assert_eq!(
            bindings(FirstClass::Python, source, "user"),
            [
                false, // from-module path
                true,  // imported name
                false, // import module path
                false, // keyword argument name
                true,  // keyword argument value
                false, // attribute
                false, // class attribute
                false, // method
                true,  // module function
                false, // keyword pattern
            ]
        );
    }

    #[test]
    fn rust_fields_variants_methods_paths_and_macros_are_no_bindings() {
        let source = "struct S { user: u32 }\n\
            enum E { User }\n\
            fn f(user: u32) { let o = Outa { user }; let p = Outa { user: user }; o.user; S::user(); println!(\"{}\", user); }\n\
            impl S { fn user(&self) {} }\n\
            #[serde(rename = \"x\")] struct User;\n";
        assert_eq!(
            bindings(FirstClass::Rust, source, "user"),
            [
                false, // field declaration
                true,  // parameter
                false, // shorthand field initializer
                false, // field initializer key
                true,  // field initializer value
                false, // field access
                false, // scoped path name
                false, // macro argument
                false, // method
            ]
        );
        assert_eq!(bindings(FirstClass::Rust, source, "User"), [false, true]);
    }

    #[test]
    fn go_fields_keys_methods_and_qualified_types_are_no_bindings() {
        let source = "package a\n\
            type T struct { name string }\n\
            func (t T) name() {}\n\
            func g(name string) { v := T{name: name}; v.name; var q name.Type; _ = q }\n";
        assert_eq!(
            bindings(FirstClass::Go, source, "name"),
            [
                false, // field declaration
                false, // method
                true,  // parameter
                false, // composite literal key
                true,  // composite literal value
                false, // selector field
                false, // package of a qualified type
            ]
        );
        assert_eq!(bindings(FirstClass::Go, source, "Type"), [false]);
    }

    #[test]
    fn unreviewed_grammars_have_no_bindings() {
        let source = "class C { void M() { var user = 1; Send(user: user); } }";
        assert!(
            bindings(FirstClass::CSharp, source, "user")
                .iter()
                .all(|binding| !binding)
        );
    }

    #[test]
    fn spelling_changes_with_language_meaning_are_never_renamable() {
        let go = [FirstClass::Go; 2];
        let python = [FirstClass::Python; 2];
        let script = [FirstClass::TypeScript; 2];
        assert!(!renamable("helper", "Helper", go, false));
        assert!(renamable("helper", "assist", go, false));
        assert!(renamable("helper", "Helper", script, false));
        assert!(!renamable("cache", "_cache", python, false));
        assert!(!renamable("setup", "__init__", python, false));
        assert!(renamable("cache", "store", python, false));
        assert!(!renamable("test_total", "check_total", python, true));
        assert!(!renamable("totalWorks", "totalTest", script, true));
        assert!(renamable("test_total", "test_sum", python, true));
        assert!(renamable("test_total", "check_total", python, false));
    }
}
