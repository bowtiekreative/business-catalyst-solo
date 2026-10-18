use regex::Regex;
use std::{fs, path::{Component, Path, PathBuf}, process::{Command, Output}};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(crate) const MAX_TEXT: u64 = 1_048_576;

#[derive(Clone, Debug)]
pub struct Project {
    pub(crate) root: PathBuf,
}

impl Project {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        if !root.is_dir() { return Err("Choose a website folder".into()); }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path { &self.root }

    pub(crate) fn file(&self, relative: &str) -> Result<PathBuf> {
        if relative.contains('\\') || relative.is_empty() { return Err("Invalid file path".into()); }
        let mut target = self.root.clone();
        for part in Path::new(relative).components() {
            let Component::Normal(name) = part else { return Err("File must stay inside the website folder".into()); };
            if excluded(&name.to_string_lossy()) { return Err("This folder is private or generated".into()); }
            target.push(name);
            let meta = fs::symlink_metadata(&target)?;
            if meta.file_type().is_symlink() { return Err("Linked files are read-only".into()); }
        }
        if !target.canonicalize()?.starts_with(&self.root) { return Err("File is outside the website folder".into()); }
        if !target.is_file() { return Err("Choose a source file".into()); }
        Ok(target)
    }

    pub fn read_source(&self, relative: &str) -> Result<String> {
        let path = self.file(relative)?;
        if !editable(&path) { return Err("This file type is read-only in the starter".into()); }
        if fs::metadata(&path)?.len() > MAX_TEXT { return Err("Source files must be under 1 MiB".into()); }
        let text = fs::read_to_string(path)?;
        check_secrets(&text)?;
        Ok(text)
    }

    pub(crate) fn git_output(&self, args: &[&str]) -> Result<Output> {
        Ok(Command::new("git").current_dir(&self.root)
            .args(["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false",
                "-c", "commit.gpgsign=false", "-c", "core.attributesFile=/dev/null"])
            .env("GIT_ATTR_NOSYSTEM", "1").env("GIT_TERMINAL_PROMPT", "0")
            .args(args).output()?)
    }

    pub(crate) fn git(&self, args: &[&str]) -> Result<String> {
        let result = self.git_output(args)?;
        if !result.status.success() {
            // Git error text can contain config, remote URLs or credentials.
            return Err(format!("Git {} failed. Check repository access and local Git identity in your terminal.", args[0]).into());
        }
        Ok(String::from_utf8(result.stdout)?.trim().to_owned())
    }

    pub(crate) fn clean_repository(&self) -> Result<()> {
        let top = PathBuf::from(self.git(&["rev-parse", "--show-toplevel"])?).canonicalize()?;
        if top != self.root { return Err("Open the Git repository root, not a subfolder".into()); }
        self.git(&["rev-parse", "--verify", "HEAD"])?;
        if !self.git(&["status", "--porcelain", "--untracked-files=normal"])?.is_empty() {
            return Err("Commit or stash existing changes before saving with Catalyst".into());
        }
        if !self.git_output(&["check-ignore", "-q", ".catalyst/private/probe"])?.status.success() {
            return Err("Add .catalyst/private/ to .gitignore, commit it, then try again".into());
        }
        if !self.git_output(&["ls-files", ".catalyst/private"])?.stdout.is_empty() {
            return Err("Remove private Catalyst history from Git before saving".into());
        }
        let attrs = self.git(&["ls-files", "--", ".gitattributes", "**/.gitattributes"])?;
        let info = self.git(&["rev-parse", "--git-path", "info/attributes"])?;
        let info = self.root.join(info);
        if !attrs.is_empty() || info.exists() {
            return Err("Git attributes are not supported by the starter edit engine".into());
        }
        Ok(())
    }
}

pub(crate) fn excluded(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "build" | "vendor" | "data")
}

pub(crate) fn editable(path: &Path) -> bool {
    matches!(path.extension().and_then(|x| x.to_str()), Some("html" | "htm" | "astro" | "tsx" | "jsx" | "md" | "mdx" | "css"))
}

pub fn check_secrets(text: &str) -> Result<()> {
    let pattern = concat!(
        r"-----BEGIN (?:[A-Z ]+ )?PRIVATE KEY-----|",
        r"sk-ant-[A-Za-z0-9_-]{12,}|sk-[A-Za-z0-9]{20,}|",
        r"gh[pousr]_[A-Za-z0-9]{20,}|AKIA[A-Z0-9]{16}|",
        r#"(?i:(?:api[_-]?key|secret|password|access[_-]?token)\s*[=:]\s*["']?[A-Za-z0-9_+/=-]{20,})"#
    );
    if Regex::new(pattern)?.is_match(text) {
        return Err("Possible credential found. Remove it before opening or saving this source.".into());
    }
    Ok(())
}
