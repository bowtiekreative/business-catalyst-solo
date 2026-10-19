use catalyst_core::{Project, check_secrets, create_demo};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false"
            ])
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn scanner_finds_pages_missing_metadata_and_broken_links() {
    let report = Project::open(fixture("static")).unwrap().scan().unwrap();
    assert_eq!(report.site.pages.len(), 2);
    assert_eq!(
        report
            .issues
            .iter()
            .filter(|i| i.message == "Add a meta description")
            .count(),
        2
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.message == "Broken internal link: /blog")
    );
    assert!(!report.issues.iter().any(|i| i.message.contains("LAKA")));
    let value = serde_json::to_value(&report.site).unwrap();
    assert!(value.get("footerLinks").is_some());
    assert!(value.get("integrations").is_some());
}

#[test]
fn scanner_detects_frameworks_without_rewriting_custom_source() {
    for (folder, stack, pages) in [
        ("astro", "Astro", 1),
        ("next", "Next.js", 1),
        ("wordpress", "WordPress theme", 0),
    ] {
        let report = Project::open(fixture(folder)).unwrap().scan().unwrap();
        assert_eq!(report.site.stack, stack);
        assert_eq!(report.site.pages.len(), pages);
        if pages > 0 {
            assert_eq!(report.site.pages[0].secs[0].code, "custom");
        }
    }
}

#[test]
fn save_review_commit_undo_and_stale_draft() {
    let temp = tempfile::tempdir().unwrap();
    let project = create_demo(temp.path().join("site")).unwrap();
    let before = project.read_source("index.html").unwrap();
    let after = before.replace("A place for your next idea", "A clearer headline");
    let review = project
        .review("index.html", after.clone(), "Update headline".into())
        .unwrap();
    assert_eq!(
        fs::read_to_string(project.root().join("index.html")).unwrap(),
        before
    );
    let saved = project.apply(&review).unwrap();
    assert_eq!(project.read_source("index.html").unwrap(), after);
    assert_eq!(project.history().unwrap().len(), 1);
    assert!(matches!(saved.commit.len(), 40 | 64));
    assert!(project.apply(&review).is_err());
    let undo = project.review_undo(&saved).unwrap();
    project.apply(&undo).unwrap();
    assert_eq!(project.read_source("index.html").unwrap(), before);
    assert_eq!(project.history().unwrap().len(), 2);
}

#[test]
fn dirty_repository_and_credentials_block_saves() {
    let temp = tempfile::tempdir().unwrap();
    let project = create_demo(temp.path().join("site")).unwrap();
    let before = project.read_source("index.html").unwrap();
    let review = project
        .review(
            "index.html",
            format!("{before}\n<!-- draft -->"),
            "Test save".into(),
        )
        .unwrap();
    fs::write(project.root().join("unrelated.txt"), "Keep this work").unwrap();
    assert!(
        project
            .apply(&review)
            .unwrap_err()
            .to_string()
            .contains("stash")
    );
    assert_eq!(project.read_source("index.html").unwrap(), before);
    assert_eq!(
        fs::read_to_string(project.root().join("unrelated.txt")).unwrap(),
        "Keep this work"
    );
    assert!(check_secrets(&["sk-ant-", &"x".repeat(32)].concat()).is_err());
    assert!(check_secrets("API_KEY=\nUse a secret manager").is_ok());
    assert!(project.read_source("../outside.html").is_err());
    assert!(project.read_source(".env").is_err());
}

#[test]
fn failed_commit_restores_source_and_removes_staging() {
    let temp = tempfile::tempdir().unwrap();
    let project = create_demo(temp.path().join("site")).unwrap();
    let before = project.read_source("index.html").unwrap();
    let review = project
        .review(
            "index.html",
            format!("{before}\n<!-- draft -->"),
            "Test failure".into(),
        )
        .unwrap();
    git(project.root(), &["config", "--local", "user.name", ""]);
    let result = project.apply(&review);
    assert!(result.is_err());
    assert_eq!(project.read_source("index.html").unwrap(), before);
    let status = Command::new("git")
        .current_dir(project.root())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.stdout.is_empty());
    assert!(project.history().unwrap().is_empty());
}

#[test]
fn existing_demo_folder_is_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("keep.txt"), "Original").unwrap();
    assert!(create_demo(temp.path()).is_err());
    assert_eq!(
        fs::read_to_string(temp.path().join("keep.txt")).unwrap(),
        "Original"
    );
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_expose_source_or_private_history() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let project = create_demo(temp.path().join("site")).unwrap();
    fs::write(temp.path().join("outside.html"), "Private").unwrap();
    symlink(
        temp.path().join("outside.html"),
        project.root().join("linked.html"),
    )
    .unwrap();
    assert!(project.read_source("linked.html").is_err());
    fs::create_dir(project.root().join(".catalyst")).unwrap();
    symlink(temp.path(), project.root().join(".catalyst/private")).unwrap();
    assert!(project.history().is_err());
}

#[test]
fn catalog_contains_the_original_twelve_families() {
    let catalog: serde_json::Value = serde_json::from_str(catalyst_core::SECTION_CATALOG).unwrap();
    let families = catalog["families"].as_array().unwrap();
    assert_eq!(families.len(), 12);
    assert_eq!(
        families
            .iter()
            .map(|f| f["items"].as_array().unwrap().len())
            .sum::<usize>(),
        118
    );
}
