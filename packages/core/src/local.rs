//! Shared local-file primitives. Domain callers retain their own file-type,
//! repository and schema policies; these functions never accept shell commands.
use crate::Result;
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

#[derive(Clone, Copy)]
pub enum PathPolicy {
    /// Preserve Catalyst's existing website exclusions and read behavior.
    Website,
    /// Normal non-dot project paths, with hard-linked files rejected.
    LocalProject,
    /// Only the app-owned root .gitignore; never arbitrary dot-file access.
    ProjectIgnore,
}
#[derive(Clone, Copy)]
pub enum FileMode {
    Existing,
    /// Only the final file may be absent; every parent must already be safe.
    NewOrExisting,
}

pub fn validate_relative_path(relative: &str) -> Result<()> {
    if relative.is_empty()
        || relative.len() > 512
        || relative.contains('\\')
        || relative.chars().any(char::is_control)
    {
        return Err("Invalid relative file path".into());
    }
    for part in relative.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        if part.is_empty()
            || part.starts_with('.')
            || part.ends_with(['.', ' '])
            || part.contains([':', '<', '>', '"', '|', '?', '*'])
            || reserved
        {
            return Err("File must stay inside its project folder".into());
        }
    }
    if Path::new(relative)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("File must stay inside its project folder".into());
    }
    Ok(())
}

/// Resolve only under a canonical root. No symlinks are followed in descendants.
pub fn confined_file(
    root: &Path,
    relative: &str,
    policy: PathPolicy,
    mode: FileMode,
) -> Result<PathBuf> {
    // Catalyst's old parser allowed normal UTF-8 names without the project
    // length/control/colon policy; retain that behavior for existing websites.
    if matches!(policy, PathPolicy::Website) {
        if relative.contains('\\') || relative.is_empty() {
            return Err("Invalid file path".into());
        }
    } else if matches!(policy, PathPolicy::ProjectIgnore) {
        if relative != ".gitignore" {
            return Err("Only the project ignore file is allowed".into());
        }
    } else {
        validate_relative_path(relative)?;
    }
    let parts: Vec<_> = Path::new(relative).components().collect();
    let mut target = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            return Err("File must stay inside the project folder".into());
        };
        if matches!(policy, PathPolicy::Website)
            && crate::project::excluded(&name.to_string_lossy())
        {
            return Err("This folder is private or generated".into());
        }
        target.push(name);
        match fs::symlink_metadata(&target) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err("Linked files are read-only".into());
                }
                if index + 1 < parts.len() && !meta.is_dir() {
                    return Err("File parent must be a normal directory".into());
                }
                if index + 1 == parts.len() {
                    if !meta.is_file() {
                        return Err("Choose a regular file".into());
                    }
                    if !matches!(policy, PathPolicy::Website) {
                        reject_file_links(&target)?;
                    }
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && index + 1 == parts.len()
                    && matches!(mode, FileMode::NewOrExisting) =>
            {
                if !target
                    .parent()
                    .ok_or("Missing parent folder")?
                    .canonicalize()?
                    .starts_with(root)
                {
                    return Err("File is outside the project folder".into());
                }
                return Ok(target);
            }
            Err(error) => return Err(error.into()),
        }
    }
    if !target.canonicalize()?.starts_with(root) {
        return Err("File is outside the project folder".into());
    }
    Ok(target)
}

pub fn create_directory(root: &Path, relative: &str) -> Result<PathBuf> {
    directory(root, relative, true)
}
pub fn existing_directory(root: &Path, relative: &str) -> Result<PathBuf> {
    directory(root, relative, false)
}
fn directory(root: &Path, relative: &str, create: bool) -> Result<PathBuf> {
    check_root(root)?;
    validate_relative_path(relative)?;
    let mut dir = root.to_path_buf();
    for part in relative.split('/') {
        dir.push(part);
        match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => return Err("Project folder must be a normal directory".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                fs::create_dir(&dir)?
            }
            Err(error) => return Err(error.into()),
        }
    }
    if !dir.canonicalize()?.starts_with(root) {
        return Err("Folder is outside the project folder".into());
    }
    Ok(dir)
}

#[derive(Clone, Copy)]
pub enum PrivateNamespace {
    Catalyst,
    Sidebar,
}
pub fn private_dir(root: &Path, namespace: PrivateNamespace, create: bool) -> Result<PathBuf> {
    check_root(root)?;
    let namespace = match namespace {
        PrivateNamespace::Catalyst => ".catalyst",
        PrivateNamespace::Sidebar => ".sidebar",
    };
    let mut dir = root.to_path_buf();
    for part in [namespace, "private"] {
        dir.push(part);
        if dir.exists() || fs::symlink_metadata(&dir).is_ok() {
            let meta = fs::symlink_metadata(&dir)?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err("Private history must be a normal directory".into());
            }
        } else if create {
            fs::create_dir(&dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    Ok(dir)
}
fn check_root(root: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(root)?;
    if meta.file_type().is_symlink() || !meta.is_dir() || root.canonicalize()? != root {
        return Err("Project root changed or is not a normal canonical folder".into());
    }
    Ok(())
}

pub fn reject_hard_link(meta: &fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err("Hard-linked files are read-only".into());
        }
    }
    #[cfg(not(unix))]
    let _ = meta;
    Ok(())
}

/// Stable Windows std metadata does not expose link count. Query the open file
/// handle through Win32 rather than silently accepting hardlinks on that target.
pub fn reject_file_links(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err("Linked files are read-only".into());
    }
    reject_hard_link(&meta)?;
    #[cfg(windows)]
    {
        use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
        if meta.file_attributes() & 0x400 != 0 {
            return Err("Reparse-point files are read-only".into());
        }
        let file = fs::File::open(path)?;
        let mut info =
            std::mem::MaybeUninit::<winapi::um::fileapi::BY_HANDLE_FILE_INFORMATION>::zeroed();
        // SAFETY: the live std File owns the valid handle, and Win32 initializes
        // the supplied correctly-sized output before we read it on success.
        let status = unsafe {
            winapi::um::fileapi::GetFileInformationByHandle(
                file.as_raw_handle() as _,
                info.as_mut_ptr(),
            )
        };
        if status == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: status is successful, so the output structure is initialized.
        if unsafe { info.assume_init() }.nNumberOfLinks != 1 {
            return Err("Hard-linked files are read-only".into());
        }
    }
    Ok(())
}

/// An advisory lock is released by the OS if its process exits or crashes.
/// The empty lock file stays in the ignored private folder; it is not a stale
/// ownership flag and must not be removed while other processes may be saving.
pub struct SaveLock {
    _file: fs::File,
}
impl Drop for SaveLock {
    fn drop(&mut self) {
        // Release ownership before close even if a concurrently spawned child
        // temporarily inherited a duplicate of this open file description.
        let _ = fs2::FileExt::unlock(&self._file);
    }
}
pub fn lock_project(root: &Path, namespace: PrivateNamespace) -> Result<SaveLock> {
    lock_named(root, namespace, "save.lock")
}
pub fn lock_named(root: &Path, namespace: PrivateNamespace, name: &str) -> Result<SaveLock> {
    validate_relative_path(name)?;
    if name.contains('/') {
        return Err("Lock must have one fixed filename".into());
    }
    let path = private_dir(root, namespace, true)?.join(name);
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("Save lock must be a normal file".into());
        }
        reject_file_links(&path)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    reject_hard_link(&file.metadata()?)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| "Another project save is running")?;
    Ok(SaveLock { _file: file })
}

/// A confined, private streaming output. FFmpeg may write its temporary path;
/// publication fsyncs and refuses to replace any pre-existing destination.
pub struct PendingFile {
    root: PathBuf,
    relative: String,
    temp: tempfile::NamedTempFile,
}
impl PendingFile {
    pub fn prepare(root: &Path, relative: &str) -> Result<Self> {
        check_root(root)?;
        let destination = confined_file(
            root,
            relative,
            PathPolicy::LocalProject,
            FileMode::NewOrExisting,
        )?;
        if fs::symlink_metadata(&destination).is_ok() {
            return Err("New output must not replace an existing file".into());
        }
        let temp = tempfile::Builder::new()
            .prefix("pending-")
            .suffix(".part")
            .tempfile_in(destination.parent().ok_or("Missing output parent")?)?;
        Ok(Self {
            root: root.to_owned(),
            relative: relative.into(),
            temp,
        })
    }
    pub fn path(&self) -> &Path {
        self.temp.path()
    }
    pub fn file_mut(&mut self) -> &mut fs::File {
        self.temp.as_file_mut()
    }
    pub fn publish(self) -> Result<PathBuf> {
        check_root(&self.root)?;
        let destination = confined_file(
            &self.root,
            &self.relative,
            PathPolicy::LocalProject,
            FileMode::NewOrExisting,
        )?;
        reject_file_links(self.temp.path())?;
        self.temp.as_file().sync_all()?;
        self.temp.persist_noclobber(&destination).map_err(
            |_| "Output exists or could not be published; no existing file was replaced",
        )?;
        #[cfg(unix)]
        fs::File::open(destination.parent().ok_or("Missing output parent")?)?.sync_all()?;
        Ok(destination)
    }
}

pub enum WriteExpected<'a> {
    /// Catalyst already holds its own review/save lock and content checks.
    Any,
    Missing,
    Exact(&'a [u8]),
}
fn check_expected(path: &Path, expected: &WriteExpected<'_>) -> Result<()> {
    match expected {
        WriteExpected::Any => Ok(()),
        WriteExpected::Missing => match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err("File changed while preparing the save".into()),
        },
        WriteExpected::Exact(before) => {
            let meta = fs::symlink_metadata(path)?;
            if meta.file_type().is_symlink()
                || !meta.is_file()
                || meta.len() != before.len() as u64
                || fs::read(path)? != *before
            {
                return Err("File changed while preparing the save".into());
            }
            reject_file_links(path)
        }
    }
}

/// The caller must first confine `path` and its parent. The final expected-byte
/// check protects stale cooperating saves, not a malicious process racing rename.
pub fn atomic_write(path: &Path, bytes: &[u8], expected: WriteExpected<'_>) -> Result<()> {
    atomic_write_inner(path, bytes, expected, || Ok(()))
}
fn atomic_write_inner(
    path: &Path,
    bytes: &[u8],
    expected: WriteExpected<'_>,
    before_publish: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = path.parent().ok_or("Missing parent folder")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("Saving linked or non-regular files is not supported".into());
        }
        reject_file_links(path)?;
        if meta.permissions().readonly() {
            return Err("This file is read-only".into());
        }
        temp.as_file().set_permissions(meta.permissions())?;
    }
    temp.as_file().sync_all()?;
    before_publish()?;
    check_expected(path, &expected)?;
    temp.persist(path)?;
    // Sync directory metadata after replacement on Unix. Windows does not expose
    // portable directory handles through std; its replacement still stays atomic.
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// No shell, hooks, signing, fsmonitor, global attributes or interactive prompt.
/// Domain callers supply fixed Git argument vectors, never UI command strings.
pub fn hardened_git(root: &Path, args: &[&str]) -> Result<Output> {
    Ok(Command::new("git")
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_EXTERNAL_DIFF")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.attributesFile=/dev/null",
        ])
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn dropping_owner_unlocks_even_while_a_duplicated_descriptor_stays_open() {
        let temp = tempfile::tempdir().unwrap();
        let guard = lock_project(temp.path(), PrivateNamespace::Sidebar).unwrap();
        let inherited = guard._file.try_clone().unwrap();
        assert!(lock_project(temp.path(), PrivateNamespace::Sidebar).is_err());
        drop(guard);
        let next = lock_project(temp.path(), PrivateNamespace::Sidebar).unwrap();
        drop(inherited);
        assert!(lock_project(temp.path(), PrivateNamespace::Sidebar).is_err());
        drop(next);
        assert!(lock_project(temp.path(), PrivateNamespace::Sidebar).is_ok());
    }
    #[test]
    fn interrupted_or_stale_atomic_publish_keeps_a_complete_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.json");
        let old = b"{\"version\":1}";
        fs::write(&path, old).unwrap();
        assert!(
            atomic_write_inner(&path, b"{\"version\":2}", WriteExpected::Exact(old), || {
                Err("Interrupted before rename".into())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), old);
        assert!(
            atomic_write_inner(&path, b"{\"version\":2}", WriteExpected::Exact(old), || {
                fs::write(&path, b"{\"version\":3}")?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"{\"version\":3}");
    }
    #[test]
    fn abrupt_process_exit_before_rename_preserves_json_and_releases_lock() {
        const CHILD_ROOT: &str = "CATALYST_ATOMIC_CRASH_TEST_ROOT";
        let old = b"{\"version\":1}";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let _guard = lock_project(&root, PrivateNamespace::Sidebar).unwrap();
            let _ = atomic_write_inner(
                &root.join("project.json"),
                b"{\"version\":2}",
                WriteExpected::Exact(old),
                || std::process::exit(88),
            );
            panic!("Child should exit before rename");
        }
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("project.json"), old).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::tests::abrupt_process_exit_before_rename_preserves_json_and_releases_lock",
            ])
            .env(CHILD_ROOT, temp.path())
            .output()
            .unwrap()
            .status;
        assert_eq!(status.code(), Some(88));
        assert_eq!(fs::read(temp.path().join("project.json")).unwrap(), old);
        assert!(lock_project(temp.path(), PrivateNamespace::Sidebar).is_ok());
    }
    #[test]
    fn streaming_outputs_publish_once_and_cleanup_cancelled_temporary_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        create_directory(&root, "media").unwrap();
        let mut pending = PendingFile::prepare(&root, "media/video.mp4").unwrap();
        pending.file_mut().write_all(b"complete bytes").unwrap();
        let path = pending.publish().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"complete bytes");
        assert!(PendingFile::prepare(&root, "media/video.mp4").is_err());
        let mut collision = PendingFile::prepare(&root, "media/collision.mp4").unwrap();
        collision.file_mut().write_all(b"new").unwrap();
        fs::write(root.join("media/collision.mp4"), b"keep").unwrap();
        assert!(collision.publish().is_err());
        assert_eq!(fs::read(root.join("media/collision.mp4")).unwrap(), b"keep");
        let cancelled = PendingFile::prepare(&root, "media/cancelled.mp4").unwrap();
        let temporary = cancelled.path().to_owned();
        drop(cancelled);
        assert!(!temporary.exists() && !root.join("media/cancelled.mp4").exists());
    }
    #[test]
    fn project_paths_and_private_folders_are_constrained() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for bad in [
            "../outside",
            "/absolute",
            "media/../x",
            "media\\x",
            "media//x",
            ".env",
            "x:y",
        ] {
            assert!(
                confined_file(
                    &root,
                    bad,
                    PathPolicy::LocalProject,
                    FileMode::NewOrExisting
                )
                .is_err()
            );
        }
        create_directory(&root, "media").unwrap();
        assert!(
            confined_file(
                &root,
                "media/new.mp4",
                PathPolicy::LocalProject,
                FileMode::NewOrExisting
            )
            .is_ok()
        );
        let private = private_dir(&root, PrivateNamespace::Sidebar, true).unwrap();
        assert!(private.ends_with(".sidebar/private"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            assert_eq!(
                fs::metadata(&private).unwrap().permissions().mode() & 0o777,
                0o700
            );
            fs::write(root.join("media/a.mp4"), b"media").unwrap();
            fs::hard_link(root.join("media/a.mp4"), root.join("media/b.mp4")).unwrap();
            assert!(
                confined_file(
                    &root,
                    "media/a.mp4",
                    PathPolicy::LocalProject,
                    FileMode::Existing
                )
                .is_err()
            );
            symlink(dir.path(), root.join("linked")).unwrap();
            assert!(
                confined_file(
                    &root,
                    "linked/x",
                    PathPolicy::LocalProject,
                    FileMode::NewOrExisting
                )
                .is_err()
            );
        }
    }
}
