use serde::Deserialize;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub(crate) struct LanguageFixtureManifest {
    pub language: Vec<LanguageFixture>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LanguageFixture {
    pub name: String,
    pub slug: String,
    pub filename: String,
    pub exact: String,
    pub near_a: String,
    pub near_b: String,
}

pub(crate) fn language_fixtures() -> Vec<LanguageFixture> {
    let source = include_str!("../fixtures/dup_languages.toml");
    toml::from_str::<LanguageFixtureManifest>(source)
        .expect("duplication language fixture manifest must be valid TOML")
        .language
}

pub(crate) fn materialize_fixture_tree(root: &Path) -> io::Result<()> {
    fs::write(
        root.join("reposcout.toml"),
        "min_dup_tokens = 8\nmin_dup_lines = 3\nnear_dup_min_similarity = 0.85\n",
    )?;
    for case in language_fixtures() {
        for (copy, content) in [
            ("exact_a", case.exact.as_str()),
            ("exact_b", case.exact.as_str()),
            ("near_a", case.near_a.as_str()),
            ("near_b", case.near_b.as_str()),
        ] {
            let directory = root.join(&case.slug).join(copy);
            fs::create_dir_all(&directory)?;
            fs::write(directory.join(&case.filename), content)?;
        }
    }
    Ok(())
}
