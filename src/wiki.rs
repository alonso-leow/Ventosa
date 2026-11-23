//! The docs: a small wiki of Markdown articles. Built-in articles ship in the
//! binary (`docs/wiki/*.md`); edits and new articles are saved to
//! `$XDG_DATA_HOME/ventosa/wiki/<slug>.md` and take precedence. Images are
//! `![alt](img/<provider>/<file>.png)`, looked up in the user wiki folder, then
//! in the runtime's `docs/wiki/`.
//!
//! An article's first line is `# Title`; a `<!-- group: Name -->` line puts it
//! under a group of the index; its `## ` headings are its subsections.

use std::path::PathBuf;

pub struct Article {
    pub slug: String,
    pub title: String,
    pub group: String,
    pub body: String,
    /// Edited or written by the user (saved in the user wiki folder).
    pub custom: bool,
    pub builtin: bool,
}

macro_rules! builtin {
    ($($slug:literal),* $(,)?) => { &[$(($slug, include_str!(concat!("../docs/wiki/", $slug, ".md")))),*] };
}

pub static BUILTIN: &[(&str, &str)] = builtin!(
    "start-here",
    "team-setup",
    "access-and-roles",
    "local-network",
    "ssh-deploy",
    "ai-providers",
    "google-cli",
    "notion",
    "telegram-bots",
    "mailchimp",
    "mailjet",
    "stripe",
    "shopify",
    "hubspot",
    "trusted-sources",
    "subtitles",
);

pub fn user_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("wiki")
}

/// Where a relative image path resolves: the user folder, then the runtime docs.
pub fn image_path(rel: &str) -> Option<PathBuf> {
    let rt = std::env::var_os("VENTOSA_RUNTIME").map(PathBuf::from).unwrap_or_else(|| dirs::data_dir().unwrap_or_default().join("ventosa").join("runtime"));
    [user_dir().join(rel), rt.join("docs/wiki").join(rel), PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/wiki").join(rel)].into_iter().find(|p| p.is_file())
}

fn parse(slug: &str, body: String, custom: bool, builtin: bool) -> Article {
    let title = body.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or(slug).trim().to_string();
    let group = body.lines().find_map(|l| l.trim().strip_prefix("<!-- group:").and_then(|r| r.strip_suffix("-->"))).unwrap_or("guides").trim().to_string();
    Article { slug: slug.into(), title, group, body, custom, builtin }
}

/// Every article: built-ins (or their edited copies), then the user's own.
pub fn all() -> Vec<Article> {
    let mut out: Vec<Article> = BUILTIN
        .iter()
        .map(|(slug, text)| match std::fs::read_to_string(user_dir().join(format!("{slug}.md"))) {
            Ok(edited) => parse(slug, edited, true, true),
            Err(_) => parse(slug, text.to_string(), false, true),
        })
        .collect();
    for e in std::fs::read_dir(user_dir()).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(slug) = p.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
        if p.extension().is_some_and(|x| x == "md") && !BUILTIN.iter().any(|(b, _)| *b == slug) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                out.push(parse(&slug, text, true, false));
            }
        }
    }
    out
}

pub fn get(slug: &str) -> Option<Article> {
    all().into_iter().find(|a| a.slug == slug)
}

/// The `## ` subsections of an article.
pub fn sections(body: &str) -> Vec<String> {
    body.lines().filter_map(|l| l.strip_prefix("## ")).map(|s| s.trim().to_string()).collect()
}

pub fn save(slug: &str, text: &str) -> Result<(), String> {
    let slug: String = slug.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if slug.is_empty() {
        return Err("an article needs a slug (a-z, 0-9, -)".into());
    }
    std::fs::create_dir_all(user_dir()).map_err(|e| e.to_string())?;
    std::fs::write(user_dir().join(format!("{slug}.md")), text).map_err(|e| e.to_string())
}

/// Drop the user's copy: a built-in goes back to the shipped text, a custom article is deleted.
pub fn revert(slug: &str) -> Result<(), String> {
    std::fs::remove_file(user_dir().join(format!("{slug}.md"))).map_err(|e| e.to_string())
}

/// Search titles and bodies; returns slugs with the first matching line.
pub fn search(q: &str) -> Vec<(String, String)> {
    let q = q.to_lowercase();
    all().into_iter()
        .filter_map(|a| {
            let line = a.body.lines().find(|l| l.to_lowercase().contains(&q))?;
            Some((a.slug, line.trim().chars().take(90).collect()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_builtin_has_a_title_and_group() {
        for a in super::all().into_iter().filter(|a| a.builtin) {
            assert!(a.body.starts_with("# "), "{} starts with a title", a.slug);
            assert!(a.body.contains("<!-- group:"), "{} names its group", a.slug);
        }
    }
}
