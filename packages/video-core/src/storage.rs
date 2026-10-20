use crate::*;
use catalyst_core::local::{self, FileMode, PathPolicy, PrivateNamespace, WriteExpected};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const IGNORE: &str = "/media/\n/buffer/\n/exports/\n/.sidebar/private/\n.env\n.env.*\n";
type Files = BTreeMap<String, Option<Vec<u8>>>;
pub struct ProjectStore {
    root: PathBuf,
    baseline: Studio,
    files: Files,
    head: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveVersion {
    pub commit: String,
    pub created: bool,
}
impl ProjectStore {
    /// The destination must not exist, even if it is empty. No existing folder
    /// or repository is initialized/reconfigured by this operation.
    pub fn create(folder: impl AsRef<Path>, name: &str) -> Result<(Self, Studio)> {
        let mut studio = Studio::default();
        studio.rename(name)?;
        let json = studio.to_json()?;
        let requested = folder.as_ref();
        let leaf = requested
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Choose a new project folder")?;
        local::validate_relative_path(leaf)?;
        let parent = requested
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let root = parent.canonicalize()?.join(leaf);
        fs::create_dir(&root).map_err(
            |_| "Project creation requires a new folder; existing files were not changed",
        )?;
        for folder in ["overlays", "media", "buffer", "exports"] {
            local::create_directory(&root, folder)?;
        }
        local::private_dir(&root, PrivateNamespace::Sidebar, true)?;
        for (relative, bytes) in prepared_files(&studio, &json)? {
            let path = data_path(&root, &relative, FileMode::NewOrExisting)?;
            local::atomic_write(&path, &bytes, WriteExpected::Missing)?;
        }
        git(&root, &["init", "--template=", "-b", "main"])?;
        git(&root, &["config", "--local", "user.name", "Storyfold"])?;
        git(
            &root,
            &["config", "--local", "user.email", "local@storyfold.invalid"],
        )?;
        git_data(&root, &["add", "-f", "--"])?;
        git_data(&root, &["commit", "-m", "Create Storyfold project", "--"])?;
        Self::open(root)
    }
    /// Derive a unique app-owned child from the explicitly chosen parent.
    pub fn create_in(parent: impl AsRef<Path>, name: &str) -> Result<(Self, Studio)> {
        let mut check = Studio::default();
        check.rename(name)?;
        let parent = parent.as_ref().canonicalize()?;
        if !parent.is_dir() {
            return Err("Choose a parent folder for this project".into());
        }
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        for attempt in 0..100 {
            let destination = parent.join(format!(
                "storyfold-{timestamp:x}-{}-{attempt}",
                std::process::id()
            ));
            if fs::symlink_metadata(&destination).is_err() {
                return Self::create(destination, name);
            }
        }
        Err("Could not reserve a new project folder".into())
    }
    /// Read only: never initializes a repository or repairs/creates folders.
    pub fn open(folder: impl AsRef<Path>) -> Result<(Self, Studio)> {
        let meta = fs::symlink_metadata(folder.as_ref())?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("Choose a normal project folder".into());
        }
        let root = folder.as_ref().canonicalize()?;
        for folder in ["overlays", "media", "buffer", "exports"] {
            local::existing_directory(&root, folder)?;
        }
        let private = local::private_dir(&root, PrivateNamespace::Sidebar, false)?;
        if !private.is_dir() {
            return Err("Project private folder is missing".into());
        }
        validate_repository(&root)?;
        let files = read_files(&root)?;
        let bytes = files
            .get("project.json")
            .and_then(Option::as_ref)
            .ok_or("Project JSON is missing")?;
        let json = std::str::from_utf8(bytes).map_err(|_| "Project JSON must be UTF-8")?;
        let studio = Studio::from_json(json)?;
        validate_media(&root, &studio)?;
        let head = git(&root, &["rev-parse", "HEAD"])?;
        Ok((
            Self {
                root,
                baseline: studio.clone(),
                files,
                head,
            },
            studio,
        ))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn is_dirty(&self, studio: &Studio) -> bool {
        studio != &self.baseline
    }
    pub fn save(&mut self, studio: &Studio) -> Result<()> {
        let _lock = local::lock_project(&self.root, PrivateNamespace::Sidebar)?;
        self.save_locked(studio)
    }
    pub fn save_version(&mut self, studio: &Studio, summary: &str) -> Result<SaveVersion> {
        let summary = summary.trim();
        if summary.is_empty()
            || summary.chars().count() > 200
            || summary.chars().any(char::is_control)
        {
            return Err("Describe this version in one line up to 200 characters".into());
        }
        catalyst_core::check_secrets(summary)?;
        let _lock = local::lock_project(&self.root, PrivateNamespace::Sidebar)?;
        self.save_locked(studio)?;
        // All errors beyond here explicitly preserve the acknowledged JSON. A
        // failed version must never roll a user's valid saved edits backward.
        self.commit_saved(summary).map_err(|_| "Project saved, but its local Git version could not be created. Your saved edits remain; inspect local Git state before retrying.".into())
    }
    fn save_locked(&mut self, studio: &Studio) -> Result<()> {
        let json = studio.to_json()?;
        validate_media(&self.root, studio)?;
        self.check_baseline()?;
        let mut prepared = prepared_files(studio, &json)?;
        // project.json is the publication point. Mirrors are only derived files;
        // interrupted preparation never invalidates the old authoritative JSON.
        let project = prepared
            .remove("project.json")
            .ok_or("Missing prepared project")?;
        for (relative, bytes) in prepared {
            self.write_prepared(&relative, bytes)?;
        }
        self.write_prepared("project.json", project)?;
        self.baseline = studio.clone();
        Ok(())
    }
    fn write_prepared(&mut self, relative: &str, bytes: Vec<u8>) -> Result<()> {
        let previous = self
            .files
            .get(relative)
            .ok_or("Unknown project data file")?;
        if previous.as_deref() == Some(&bytes) {
            return Ok(());
        }
        let path = data_path(&self.root, relative, FileMode::NewOrExisting)?;
        let expected = previous
            .as_deref()
            .map_or(WriteExpected::Missing, WriteExpected::Exact);
        local::atomic_write(&path, &bytes, expected)?;
        self.files.insert(relative.into(), Some(bytes));
        Ok(())
    }
    fn check_baseline(&self) -> Result<()> {
        validate_repository(&self.root)?;
        if git(&self.root, &["rev-parse", "HEAD"])? != self.head
            || read_files(&self.root)? != self.files
        {
            return Err("Project changed outside this window. Reopen it before saving; your in-memory edits are unchanged.".into());
        }
        Ok(())
    }
    fn commit_saved(&mut self, summary: &str) -> Result<SaveVersion> {
        self.check_baseline()?;
        // Re-scan the exact data being committed, not a cached text preview.
        for bytes in self.files.values().flatten() {
            catalyst_core::check_secrets(
                std::str::from_utf8(bytes).map_err(|_| "Project data must be UTF-8")?,
            )?;
        }
        let changed = git_data_output(&self.root, &["diff", "--quiet", "HEAD", "--"])?;
        if changed.status.success() {
            return Ok(SaveVersion {
                commit: self.head.clone(),
                created: false,
            });
        }
        if changed.status.code() != Some(1) {
            return Err("Cannot inspect saved version".into());
        }
        git_data(&self.root, &["add", "-f", "--"])?;
        if git_data(&self.root, &["commit", "-m", summary, "--"]).is_err() {
            let _ = git_data(&self.root, &["reset", "-q", "HEAD", "--"]);
            return Err("Local version commit failed".into());
        }
        self.head = git(&self.root, &["rev-parse", "HEAD"])?;
        Ok(SaveVersion {
            commit: self.head.clone(),
            created: true,
        })
    }
}

fn data_names() -> Vec<String> {
    let mut names = vec!["project.json".into(), ".gitignore".into()];
    names.extend((1..=SLOT_COUNT).map(|slot| format!("overlays/slot-{slot}.json")));
    names
}
fn data_path(root: &Path, relative: &str, mode: FileMode) -> Result<PathBuf> {
    let policy = if relative == ".gitignore" {
        PathPolicy::ProjectIgnore
    } else {
        PathPolicy::LocalProject
    };
    local::confined_file(root, relative, policy, mode)
}
fn prepared_files(studio: &Studio, json: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::from([
        ("project.json".into(), json.as_bytes().to_vec()),
        (".gitignore".into(), IGNORE.as_bytes().to_vec()),
    ]);
    for slot in studio.slots() {
        let json = serde_json::to_vec_pretty(slot)?;
        catalyst_core::check_secrets(std::str::from_utf8(&json)?)?;
        files.insert(format!("overlays/slot-{}.json", slot.key().number()), json);
    }
    Ok(files)
}
fn read_files(root: &Path) -> Result<Files> {
    let mut files = Files::new();
    for name in data_names() {
        let path = data_path(root, &name, FileMode::NewOrExisting)?;
        let bytes = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                let limit = if name == "project.json" {
                    MAX_PROJECT_BYTES
                } else {
                    65_536
                };
                if meta.len() > limit as u64 {
                    return Err("Project data exceeds supported size".into());
                }
                Some(fs::read(&path)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        files.insert(name, bytes);
    }
    if files.get(".gitignore").and_then(Option::as_deref) != Some(IGNORE.as_bytes()) {
        return Err(
            "Project ignore rules changed; media and private data must stay ignored".into(),
        );
    }
    Ok(files)
}
fn validate_media(root: &Path, studio: &Studio) -> Result<()> {
    for content in studio
        .slots()
        .iter()
        .filter_map(|s| s.content())
        .chain(studio.cues().iter().filter_map(|c| c.content()))
    {
        if let Some(path) = &content.image {
            local::confined_file(root, path, PathPolicy::LocalProject, FileMode::Existing)?;
        }
    }
    for source in studio.media() {
        local::confined_file(
            root,
            source.path(),
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
    }
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let result = local::hardened_git(root, args)?;
    if !result.status.success() {
        return Err("Local project Git operation failed".into());
    }
    Ok(String::from_utf8(result.stdout)
        .map_err(|_| "Git returned invalid text")?
        .trim()
        .to_owned())
}
fn git_data_output(root: &Path, prefix: &[&str]) -> Result<std::process::Output> {
    let names = data_names();
    let mut args = prefix.to_vec();
    args.extend(names.iter().map(String::as_str));
    local::hardened_git(root, &args)
}
fn git_data(root: &Path, prefix: &[&str]) -> Result<()> {
    if !git_data_output(root, prefix)?.status.success() {
        return Err("Local project Git operation failed".into());
    }
    Ok(())
}
fn validate_repository(root: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(root.join(".git"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("Project must own a normal local Git repository".into());
    }
    if PathBuf::from(git(root, &["rev-parse", "--show-toplevel"])?).canonicalize()? != root {
        return Err("Project must be its repository root".into());
    }
    let staged = local::hardened_git(root, &["diff", "--cached", "--quiet", "--exit-code"])?;
    if !staged.status.success() {
        return Err("Project has staged changes; inspect Git before saving".into());
    }
    for path in [
        ".gitattributes",
        "overlays/.gitattributes",
        ".git/info/attributes",
    ] {
        if fs::symlink_metadata(root.join(path)).is_ok() {
            return Err("Project Git attributes are not supported".into());
        }
    }
    let files = local::hardened_git(root, &["ls-files", "-z"])?;
    if !files.status.success() {
        return Err("Cannot inspect tracked project files".into());
    }
    let allowed = data_names();
    let mut tracked_count = 0;
    for name in files
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = std::str::from_utf8(name).map_err(|_| "Tracked paths must be UTF-8")?;
        if !allowed.iter().any(|allowed| allowed == name) {
            return Err("Only project and overlay data may be tracked".into());
        }
        data_path(root, name, FileMode::NewOrExisting)?;
        tracked_count += 1;
    }
    if tracked_count != allowed.len() {
        return Err("Project and overlay data must remain tracked".into());
    }
    for ignored in [
        "media/probe",
        "buffer/probe",
        "exports/probe",
        ".sidebar/private/probe",
    ] {
        if !local::hardened_git(root, &["check-ignore", "-q", "--", ignored])?
            .status
            .success()
        {
            return Err("Project media, exports and private data must stay ignored".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> (tempfile::TempDir, ProjectStore, Editor) {
        let temp = tempfile::tempdir().unwrap();
        let (store, studio) =
            ProjectStore::create(temp.path().join("project"), "Interview").unwrap();
        (temp, store, Editor::new(studio).unwrap())
    }
    #[test]
    fn create_is_new_only_and_owned_structure_git_rules_are_real() {
        let (temp, store, _) = project();
        assert!(ProjectStore::create(store.root(), "Overwrite").is_err());
        assert!(ProjectStore::create(temp.path(), "Overwrite").is_err());
        for folder in ["overlays", "media", "buffer", "exports", ".sidebar/private"] {
            assert!(store.root().join(folder).is_dir());
        }
        assert_eq!(git(store.root(), &["remote"]).unwrap(), "");
        let files = git(store.root(), &["ls-files"]).unwrap();
        assert!(files.contains("project.json") && files.contains("overlays/slot-9.json"));
        assert!(!files.contains("media/") && !files.contains("private/"));
        let (child, _) = ProjectStore::create_in(temp.path(), "Another").unwrap();
        assert_eq!(child.root().parent().unwrap(), temp.path());
    }
    #[test]
    fn save_load_roundtrip_retains_removed_files_cues_and_does_not_replace_newer_draft() {
        let (_temp, mut store, _) = project();
        fs::write(store.root().join("media/interview.mp4"), b"source remains").unwrap();
        let (mut editor, first, _, _) = crate::timeline::tests::populated();
        editor.apply(Edit::RemoveClip { clip: first }).unwrap();
        let submitted = editor.studio().clone();
        editor
            .apply(Edit::Rename("Newer in-memory edit".into()))
            .unwrap();
        store.save(&submitted).unwrap();
        assert!(!store.is_dirty(&submitted));
        assert!(store.is_dirty(editor.studio()));
        let (_, loaded) = ProjectStore::open(store.root()).unwrap();
        assert_eq!(loaded, submitted);
        assert_eq!(
            fs::read(store.root().join("media/interview.mp4")).unwrap(),
            b"source remains"
        );
        assert_eq!(editor.studio().name(), "Newer in-memory edit");
    }
    #[test]
    fn stale_windows_external_changes_and_live_locks_preserve_the_saved_document() {
        let (_temp, mut first, mut editor) = project();
        let (mut second, _) = ProjectStore::open(first.root()).unwrap();
        editor.apply(Edit::Rename("First wins".into())).unwrap();
        first.save(editor.studio()).unwrap();
        editor.apply(Edit::Rename("Stale".into())).unwrap();
        assert!(second.save(editor.studio()).is_err());
        let before = fs::read(first.root().join("project.json")).unwrap();
        let guard = local::lock_project(first.root(), PrivateNamespace::Sidebar).unwrap();
        assert!(first.save(editor.studio()).is_err());
        drop(guard);
        // An inert lock file remains and a subsequent save succeeds: no stale lock.
        first.save(editor.studio()).unwrap();
        assert_ne!(fs::read(first.root().join("project.json")).unwrap(), before);
        fs::write(
            first.root().join("overlays/slot-1.json"),
            b"external change",
        )
        .unwrap();
        assert!(first.save(editor.studio()).is_err());
    }
    #[test]
    fn version_is_local_and_failed_commit_keeps_latest_valid_json() {
        let (_temp, mut store, mut editor) = project();
        let initial = store.save_version(editor.studio(), "No change").unwrap();
        assert!(!initial.created);
        editor.apply(Edit::Rename("Versioned".into())).unwrap();
        let saved = store
            .save_version(editor.studio(), "Save interview")
            .unwrap();
        assert!(saved.created && saved.commit != initial.commit);
        git(store.root(), &["config", "--local", "user.name", ""]).unwrap();
        editor
            .apply(Edit::Rename("Saved despite Git failure".into()))
            .unwrap();
        let error = store
            .save_version(editor.studio(), "Cannot commit")
            .unwrap_err()
            .to_string();
        assert!(error.contains("Project saved"));
        let (_, loaded) = ProjectStore::open(store.root()).unwrap();
        assert_eq!(loaded, *editor.studio());
        assert!(!store.is_dirty(editor.studio()));
        assert_eq!(
            git(store.root(), &["rev-parse", "HEAD"]).unwrap(),
            saved.commit
        );
    }
    #[test]
    fn malformed_untrusted_projects_and_unsafe_media_are_rejected_without_reinit() {
        let (_temp, mut store, mut editor) = project();
        let old = fs::read(store.root().join("project.json")).unwrap();
        for json in [
            "{}".to_owned(),
            String::from_utf8(old.clone())
                .unwrap()
                .replacen("\"version\": 1", "\"version\": 9", 1),
        ] {
            assert!(Studio::from_json(&json).is_err());
        }
        assert!(
            editor
                .apply(Edit::AddMedia {
                    path: "media/../outside.mp4".into(),
                    duration_ms: 1000
                })
                .is_err()
        );
        editor
            .apply(Edit::AddMedia {
                path: "media/missing.mp4".into(),
                duration_ms: 1000,
            })
            .unwrap();
        assert!(store.save(editor.studio()).is_err());
        assert_eq!(fs::read(store.root().join("project.json")).unwrap(), old);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = tempfile::NamedTempFile::new().unwrap();
            symlink(outside.path(), store.root().join("media/missing.mp4")).unwrap();
            assert!(store.save(editor.studio()).is_err());
            fs::remove_file(store.root().join("media/missing.mp4")).unwrap();
            fs::hard_link(outside.path(), store.root().join("media/missing.mp4")).unwrap();
            assert!(store.save(editor.studio()).is_err());
        }
    }
    #[test]
    fn interrupted_mirror_preparation_does_not_corrupt_authoritative_old_json() {
        let (_temp, mut store, mut editor) = project();
        let old = editor.studio().clone();
        // Simulate stopping after a complete mirror was published but before
        // project.json. Opening still reads the old valid document, not mirrors.
        fs::write(
            store.root().join("overlays/slot-2.json"),
            b"{\"key\":2,\"kind\":\"quote\"}",
        )
        .unwrap();
        let (_, recovered) = ProjectStore::open(store.root()).unwrap();
        assert_eq!(recovered, old);
        editor
            .apply(Edit::Rename(
                "Not allowed to overwrite external preparation".into(),
            ))
            .unwrap();
        assert!(store.save(editor.studio()).is_err());
    }
    #[test]
    fn failed_project_publication_leaves_old_json_loadable_then_can_retry() {
        let (_temp, mut store, mut editor) = project();
        let old = editor.studio().clone();
        let path = store.root().join("project.json");
        let original_permissions = fs::metadata(&path).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        editor
            .apply(Edit::SetSlotKind {
                slot: SlotKey::try_from(2).unwrap(),
                kind: Some(OverlayKind::Quote),
            })
            .unwrap();
        assert!(store.save(editor.studio()).is_err());
        assert!(
            fs::read_to_string(store.root().join("overlays/slot-2.json"))
                .unwrap()
                .contains("quote")
        );
        let (_, loaded) = ProjectStore::open(store.root()).unwrap();
        assert_eq!(loaded, old);
        fs::set_permissions(&path, original_permissions).unwrap();
        store.save(editor.studio()).unwrap();
        assert_eq!(
            ProjectStore::open(store.root()).unwrap().1,
            *editor.studio()
        );
    }
    #[test]
    fn version_never_stages_unknown_files_and_rejects_preexisting_staging_or_secret_summary() {
        let (_temp, mut store, mut editor) = project();
        let unknown = store.root().join("overlays/user-notes.json");
        fs::write(&unknown, format!("OPENAI_API_KEY=sk-{}", "a".repeat(40))).unwrap();
        editor.apply(Edit::Rename("Safe version".into())).unwrap();
        store.save_version(editor.studio(), "Safe version").unwrap();
        assert!(
            !git(store.root(), &["ls-files"])
                .unwrap()
                .contains("user-notes")
        );
        let before = fs::read(store.root().join("project.json")).unwrap();
        editor
            .apply(Edit::Rename("Must stay draft".into()))
            .unwrap();
        assert!(
            store
                .save_version(
                    editor.studio(),
                    &format!("OPENAI_API_KEY=sk-{}", "a".repeat(40))
                )
                .is_err()
        );
        assert_eq!(fs::read(store.root().join("project.json")).unwrap(), before);
        git(store.root(), &["add", "--", "overlays/user-notes.json"]).unwrap();
        assert!(store.save(editor.studio()).is_err());
        assert!(
            git(store.root(), &["diff", "--cached", "--name-only"])
                .unwrap()
                .contains("user-notes")
        );
        assert_eq!(
            fs::read_to_string(&unknown).unwrap(),
            format!("OPENAI_API_KEY=sk-{}", "a".repeat(40))
        );
    }
}
