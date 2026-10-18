use crate::{Project, Result, check_secrets, project::MAX_TEXT};
use serde::{Deserialize, Serialize};
use std::{fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Review {
    pub path: String,
    pub before: String,
    pub after: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedChange {
    pub id: String,
    pub commit: String,
    pub review: Review,
}

impl Project {
    pub fn review(&self, path: &str, after: String, summary: String) -> Result<Review> {
        let before = self.read_source(path)?;
        if before == after { return Err("There are no changes to review".into()); }
        if after.len() as u64 > MAX_TEXT { return Err("Source files must be under 1 MiB".into()); }
        check_secrets(&after)?;
        if summary.trim().is_empty() || summary.len() > 200 || summary.chars().any(char::is_control) {
            return Err("Describe this change in one line (up to 200 characters)".into());
        }
        check_secrets(&summary)?;
        Ok(Review { path: path.into(), before, after, summary })
    }

    pub fn apply(&self, review: &Review) -> Result<SavedChange> {
        let current = self.read_source(&review.path)?;
        if current != review.before { return Err("Source changed since review. Rescan and review again.".into()); }
        let validated = self.review(&review.path, review.after.clone(), review.summary.clone())?;
        let file = self.file(&review.path)?;
        if fs::metadata(&file)?.permissions().readonly() { return Err("This source file is read-only".into()); }
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            if fs::metadata(&file)?.nlink() != 1 { return Err("Hard-linked files are read-only".into()); }
        }
        self.clean_repository()?;
        self.scan_tracked_secrets()?;
        let private = self.private_dir(true)?;
        let lock = private.join("save.lock");
        let lock_file = OpenOptions::new().write(true).create_new(true).open(&lock)
            .map_err(|_| "Another Catalyst save is running. If a process crashed, inspect .catalyst/private/save.lock.")?;
        let _guard = Lock { path: lock, file: Some(lock_file) };
        self.clean_repository()?;
        if self.read_source(&review.path)? != validated.before { return Err("Source changed while preparing the save".into()); }
        let id = format!("{}-{}", SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(), std::process::id());
        let record = private.join(format!("{id}.json"));
        // Persist recovery data before touching source. Ignored history is private.
        let mut saved = SavedChange { id, commit: "pending".into(), review: validated };
        atomic_write(&record, &serde_json::to_string_pretty(&saved)?)?;
        atomic_write(&file, &review.after)?;
        let commit = self.git(&["add", "--", &review.path])
            .and_then(|_| self.git(&["commit", "-m", &review.summary, "--", &review.path]));
        match commit {
            Ok(_) => {
                saved.commit = self.git(&["rev-parse", "HEAD"])
                    .map_err(|_| "Git committed the change but its ID could not be read. Inspect Git and private recovery history before retrying.")?;
                if atomic_write(&record, &serde_json::to_string_pretty(&saved)?).is_err() {
                    return Err(format!("Saved commit {} but could not finish private history. Recovery data is in .catalyst/private.", saved.commit).into());
                }
                Ok(saved)
            }
            Err(error) => {
                atomic_write(&file, &review.before)?;
                self.git(&["reset", "HEAD", "--", &review.path])?;
                fs::remove_file(record)?;
                Err(error)
            }
        }
    }

    pub fn history(&self) -> Result<Vec<SavedChange>> {
        let dir = self.private_dir(false)?;
        if !dir.exists() { return Ok(Vec::new()); }
        let mut changes = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink() { return Err("Linked history files are not supported".into()); }
            if entry.path().extension().is_some_and(|x| x == "json") {
                if entry.metadata()?.len() > MAX_TEXT * 3 { return Err("History record is too large".into()); }
                let text = fs::read_to_string(entry.path())?;
                check_secrets(&text)?;
                let saved: SavedChange = serde_json::from_str(&text)?;
                if saved.commit != "pending" { changes.push(saved); }
            }
        }
        changes.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(changes)
    }

    pub fn review_undo(&self, saved: &SavedChange) -> Result<Review> {
        if self.read_source(&saved.review.path)? != saved.review.after {
            return Err("File has newer changes. Restore the earlier text manually after reviewing it.".into());
        }
        self.review(&saved.review.path, saved.review.before.clone(), format!("Undo: {}", saved.review.summary).chars().take(200).collect())
    }

    fn private_dir(&self, create: bool) -> Result<PathBuf> {
        let mut dir = self.root.clone();
        for part in [".catalyst", "private"] {
            dir.push(part);
            if dir.exists() || fs::symlink_metadata(&dir).is_ok() {
                let meta = fs::symlink_metadata(&dir)?;
                if meta.file_type().is_symlink() || !meta.is_dir() { return Err("Private history must be a normal directory".into()); }
            } else if create {
                fs::create_dir(&dir)?;
                #[cfg(unix)] {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
                }
            }
        }
        Ok(dir)
    }

    fn scan_tracked_secrets(&self) -> Result<()> {
        let list = self.git_output(&["ls-files", "-z"])?;
        if !list.status.success() { return Err("Cannot inspect tracked files".into()); }
        for name in list.stdout.split(|x| *x == 0).filter(|x| !x.is_empty()) {
            let name = std::str::from_utf8(name)?;
            let path = self.root.join(name);
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() { return Err("Saving repositories with tracked symlinks is not supported".into()); }
            if !meta.is_file() { return Err("Saving repositories with submodules is not supported".into()); }
            if meta.len() > MAX_TEXT { return Err("Secret scan supports tracked files up to 1 MiB".into()); }
            let data = fs::read(&path)?;
            if data.contains(&0) { continue; }
            if let Ok(text) = std::str::from_utf8(&data) { check_secrets(text)?; }
            else { return Err("Cannot scan a non-UTF-8 text file for credentials".into()); }
        }
        Ok(())
    }
}

fn atomic_write(path: &Path, text: &str) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing parent folder")?)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    if path.exists() { temp.as_file().set_permissions(fs::metadata(path)?.permissions())?; }
    temp.persist(path)?;
    Ok(())
}

struct Lock { path: PathBuf, file: Option<fs::File> }
impl Drop for Lock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}
