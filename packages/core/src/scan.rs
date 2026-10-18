use crate::{Project, Result, model::{Page, Section, Site}, project::excluded};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub severity: String,
    pub page: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub site: Site,
    pub issues: Vec<Issue>,
    pub files: Vec<String>,
}

impl Project {
    pub fn scan(&self) -> Result<Report> {
        let mut files = Vec::new();
        walk(&self.root, &self.root, &mut files, 0)?;
        files.sort();
        let stack = if files.iter().any(|p| p.ends_with(".astro")) { "Astro" }
            else if self.root.join("next.config.js").exists() || self.root.join("next.config.ts").exists()
                || files.iter().any(|p| p.ends_with("page.tsx")) { "Next.js" }
            else if self.root.join("style.css").exists() && self.root.join("functions.php").exists() { "WordPress theme" }
            else { "Static HTML" };
        let mut site = Site { name: self.root.file_name().unwrap_or_default().to_string_lossy().into(), stack: stack.into(), ..Site::default() };
        let mut issues = Vec::new();
        let mut sources = Vec::new();
        let catalog: serde_json::Value = serde_json::from_str(crate::SECTION_CATALOG)?;
        let tag = Regex::new(r"(?is)<(header|section|footer)\b([^>]*)>")?;
        for file in &files {
            let Some(route) = route(file, stack) else { continue; };
            let text = self.read_source(file)?;
            let title = capture(&text, r"(?is)<title[^>]*>(.*?)</title>")?;
            let heading = capture(&text, r"(?is)<h1[^>]*>(.*?)</h1>")?;
            let desc = meta_description(&text)?;
            let mut secs = Vec::new();
            for (i, found) in tag.captures_iter(&text).enumerate() {
                let code = attribute(&found[2], "data-laka")?;
                let pattern = catalog["families"].as_array().into_iter().flatten()
                    .flat_map(|f| f["items"].as_array().into_iter().flatten())
                    .find(|item| item["code"].as_str() == Some(code.as_str()));
                secs.push(Section { id: format!("{file}:{i}"),
                    code: if code.is_empty() { "custom".into() } else { code },
                    variant: pattern.and_then(|p| p["variant"].as_str()).unwrap_or("custom").into(),
                    label: pattern.and_then(|p| p["label"].as_str()).unwrap_or(&found[1]).into(),
                    formula: pattern.and_then(|p| p["formula"].as_str()).unwrap_or("").into(), p: Default::default() });
            }
            if secs.is_empty() {
                secs.push(Section { id: format!("{file}:custom"), code: "custom".into(),
                    variant: "custom".into(), label: "Original source".into(), ..Section::default() });
            }
            if desc.is_empty() { issues.push(issue("warning", &route, "Add a meta description")); }
            if heading.is_empty() { issues.push(issue("warning", &route, "No literal H1 found; check the rendered page")); }
            let mapped = secs.iter().any(|s| s.code != "custom");
            if mapped {
                if !secs.first().is_some_and(|s| s.code.starts_with("01")) {
                    issues.push(issue("blocker", &route, "LAKA header must be first"));
                }
                if !secs.last().is_some_and(|s| s.variant == "footer") {
                    issues.push(issue("blocker", &route, "LAKA footer must be last"));
                }
                if secs.iter().filter(|s| s.code.starts_with("02")).count() != 1 {
                    issues.push(issue("blocker", &route, "Use exactly one LAKA hero"));
                }
            }
            site.pages.push(Page { id: file.clone(), name: if !heading.is_empty() { heading } else { route.clone() },
                route, title, desc, nav: true, file: Some(file.clone()), secs });
            sources.push(text);
        }
        let routes: HashSet<_> = site.pages.iter().map(|p| p.route.as_str()).collect();
        let links = Regex::new(r#"(?i)\bhref\s*=\s*["'](/[^"']*)["']"#)?;
        for (page, text) in site.pages.iter().zip(&sources) {
            let mut seen = HashSet::new();
            for found in links.captures_iter(text) {
                let raw = &found[1];
                if raw.starts_with("//") { continue; }
                let path = raw.split(['?', '#']).next().unwrap_or(raw);
                let target = normalize_route(path);
                let asset = self.root.join(path.trim_start_matches('/'));
                if !routes.contains(target.as_str()) && !asset.is_file() && seen.insert(target.clone()) {
                    issues.push(issue("blocker", &page.route, &format!("Broken internal link: {target}")));
                }
            }
        }
        if site.pages.is_empty() { issues.push(issue("warning", "/", "No supported pages found. Dynamic routes and WordPress PHP templates need an adapter.")); }
        Ok(Report { site, issues, files })
    }
}

fn issue(severity: &str, page: &str, message: &str) -> Issue {
    Issue { severity: severity.into(), page: page.into(), message: message.into() }
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<String>, depth: usize) -> Result<()> {
    if depth > 32 { return Err("Folder is too deeply nested".into()); }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if excluded(&name.to_string_lossy()) { continue; }
        let kind = entry.file_type()?;
        if kind.is_symlink() { continue; }
        if kind.is_dir() { walk(root, &entry.path(), files, depth + 1)?; }
        else if kind.is_file() {
            if files.len() >= 10_000 { return Err("Starter supports up to 10,000 files per site".into()); }
            files.push(entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn route(file: &str, stack: &str) -> Option<String> {
    if file.contains('[') || file.contains('(') { return None; }
    let path = if stack == "Astro" {
        file.strip_prefix("src/pages/")?.strip_suffix(".astro")?.to_owned()
    } else if stack == "Next.js" {
        let path = file.strip_prefix("src/").unwrap_or(file);
        if let Some(path) = path.strip_prefix("app/") {
            let path = path.strip_suffix("page.tsx").or_else(|| path.strip_suffix("page.jsx"))?;
            path.trim_end_matches('/').to_owned()
        } else {
            let path = path.strip_prefix("pages/")?;
            if path.starts_with('_') || path.starts_with("api/") { return None; }
            path.strip_suffix(".tsx").or_else(|| path.strip_suffix(".jsx"))?.to_owned()
        }
    } else {
        file.strip_suffix(".html").or_else(|| file.strip_suffix(".htm"))?.to_owned()
    };
    Some(normalize_route(&format!("/{path}")))
}

fn normalize_route(path: &str) -> String {
    let path = path.strip_suffix(".html").unwrap_or(path).trim_end_matches('/');
    let path = path.strip_suffix("/index").unwrap_or(path);
    if path.is_empty() { "/".into() } else { path.into() }
}

fn capture(text: &str, pattern: &str) -> Result<String> {
    let re = Regex::new(pattern)?;
    Ok(re.captures(text).and_then(|x| x.get(1)).map(|x| x.as_str().trim().to_owned()).unwrap_or_default())
}

fn attribute(text: &str, name: &str) -> Result<String> {
    capture(text, &format!(r#"(?i)\b{}\s*=\s*["']([^"']*)["']"#, regex::escape(name)))
}

fn meta_description(text: &str) -> Result<String> {
    for tag in Regex::new(r"(?is)<meta\b[^>]*>")?.find_iter(text) {
        if attribute(tag.as_str(), "name")?.eq_ignore_ascii_case("description") {
            return attribute(tag.as_str(), "content");
        }
    }
    Ok(String::new())
}
