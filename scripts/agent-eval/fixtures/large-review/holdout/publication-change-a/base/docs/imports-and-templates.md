# Import and template authoring

An import source may start with a small `---` metadata block containing `title`, `language`, and
`tags`. Entries use `key: value`; unknown keys, duplicate keys, and unterminated blocks fail.
Tags are comma-separated. If the title is absent, the first top-level `# ` heading supplies it.
The remaining text is the document body. CRLF is normalized to LF. This format is intentionally a
small local convention rather than full YAML or a complete Markdown parser.

Import preview parses and summarizes without creating a document. Import apply creates a new
document or revises an explicitly selected document in the same collection. Existing document
updates can include an expected revision to prevent overwriting a concurrent edit. Importing
does not grant approval or publish a release. It uses the ordinary document services so access,
revision immutability, and atomic errors retain the same meaning.

Templates contain title and body patterns with named `{{parameter}}` placeholders. The explicit
required parameter list must match the placeholders across both patterns. Instantiation validates
all required values, rejects undeclared values, and substitutes them literally. A supplied value
that itself contains placeholder syntax is content, not another template expansion. Empty values
are permitted, although the resulting document title must still satisfy document validation.

Template instantiation creates an ordinary working document in the template's collection or an
explicitly requested target. Readers can inspect template metadata, while creation requires edit
rights. Template title/body patterns are not release manifests and do not select document versions.
Their string replacement concerns authoring content rather than publication identity.

Document link syntax supports `[label](doc:document_ID)` and an optional `@revision_ID` suffix.
Unqualified links follow the current working target; qualified links select a matching revision.
The link checker hides titles for inaccessible, missing, or archived targets. Document outlines
recognize hash-prefixed headings outside fenced code and create stable duplicate anchors.
