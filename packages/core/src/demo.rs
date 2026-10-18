use crate::{Project, Result, check_secrets};
use std::{fs, path::Path};

pub fn create_demo(path: impl AsRef<Path>) -> Result<Project> {
    // create_dir deliberately refuses to overwrite an existing folder.
    fs::create_dir(path.as_ref())?;
    for (name, content) in [
        ("index.html", include_str!("../../../fixtures/static/index.html")),
        ("about.html", include_str!("../../../fixtures/static/about.html")),
        (".gitignore", ".catalyst/private/\n.env\n.env.*\n"),
    ] {
        check_secrets(content)?;
        fs::write(path.as_ref().join(name), content)?;
    }
    let project = Project::open(path)?;
    project.git(&["init", "-b", "main"])?;
    project.git(&["config", "--local", "user.name", "Catalyst Demo"])?;
    project.git(&["config", "--local", "user.email", "demo@example.invalid"])?;
    project.git(&["add", "--", "index.html", "about.html", ".gitignore"])?;
    project.git(&["commit", "-m", "Create disposable Catalyst sample"])?;
    Ok(project)
}
