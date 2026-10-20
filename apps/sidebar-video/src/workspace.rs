//! UI session state. Slow file dialogs and project/Git work stay off the paint thread.
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use video_core::{Edit, EditOutcome, Editor, ProjectStore, SaveVersion, Studio};

type Store = Arc<Mutex<ProjectStore>>;
enum Message {
    Opened {
        store: Store,
        studio: Studio,
    },
    Attached {
        store: Store,
        snapshot: Studio,
        warning: Option<String>,
    },
    Saved {
        snapshot: Studio,
        version: Option<SaveVersion>,
        warning: Option<String>,
    },
    Cancelled,
    Failed(String),
}
struct Pending {
    receiver: mpsc::Receiver<Message>,
    loading: bool,
}

pub struct Workspace {
    pub editor: Editor,
    baseline: Studio,
    store: Option<Store>,
    pub folder: Option<PathBuf>,
    pending: Option<Pending>,
    pub status: String,
    pub error: Option<String>,
}
impl Default for Workspace {
    fn default() -> Self {
        let studio = Studio::default();
        Self {
            editor: Editor::new(studio.clone()).expect("Default studio is valid"),
            baseline: studio,
            store: None,
            folder: None,
            pending: None,
            status: "Create a project or open a project folder to begin.".into(),
            error: None,
        }
    }
}
impl Workspace {
    pub fn studio(&self) -> &Studio {
        self.editor.studio()
    }
    pub fn dirty(&self) -> bool {
        self.studio() != &self.baseline
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn loading(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| p.loading)
    }
    pub fn edit(&mut self, edit: Edit) -> Option<EditOutcome> {
        if self.loading() {
            return None;
        }
        match self.editor.apply(edit) {
            Ok(outcome) => {
                self.error = None;
                Some(outcome)
            }
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }
    pub fn edit_batch(&mut self, edits: Vec<Edit>) -> bool {
        if self.loading() {
            return false;
        }
        match self.editor.apply_batch(edits) {
            Ok(_) => {
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }
    pub fn new_project(&mut self, ctx: &egui::Context) {
        self.start(
            ctx,
            true,
            "Choose a parent folder for the new project…",
            || {
                let Some(parent) = rfd::FileDialog::new()
                    .set_title("New Storyfold project — choose a parent folder")
                    .pick_folder()
                else {
                    return Message::Cancelled;
                };
                match ProjectStore::create_in(parent, "Untitled recording") {
                    Ok((store, studio)) => Message::Opened {
                        store: Arc::new(Mutex::new(store)),
                        studio,
                    },
                    Err(error) => Message::Failed(error.to_string()),
                }
            },
        );
    }
    pub fn open_project(&mut self, ctx: &egui::Context) {
        self.start(ctx, true, "Choose a Storyfold project folder…", || {
            let Some(folder) = rfd::FileDialog::new()
                .set_title("Open Storyfold project folder")
                .pick_folder()
            else {
                return Message::Cancelled;
            };
            match ProjectStore::open(folder) {
                Ok((store, studio)) => Message::Opened {
                    store: Arc::new(Mutex::new(store)),
                    studio,
                },
                Err(error) => Message::Failed(error.to_string()),
            }
        });
    }
    pub fn save(&mut self, ctx: &egui::Context, summary: Option<String>) {
        let snapshot = self.studio().clone();
        let Some(store) = self.store.clone() else {
            self.start(
                ctx,
                false,
                "Choose where to save this project…",
                move || {
                    let Some(parent) = rfd::FileDialog::new()
                        .set_title("Save Storyfold project — choose a parent folder")
                        .pick_folder()
                    else {
                        return Message::Cancelled;
                    };
                    match ProjectStore::create_in(parent, snapshot.name()).and_then(
                        |(mut store, _)| {
                            store.save(&snapshot)?;
                            let warning = summary.and_then(|summary| {
                                store
                                    .save_version(&snapshot, &summary)
                                    .err()
                                    .map(|e| e.to_string())
                            });
                            Ok((store, warning))
                        },
                    ) {
                        Ok((store, warning)) => Message::Attached {
                            store: Arc::new(Mutex::new(store)),
                            snapshot,
                            warning,
                        },
                        Err(error) => Message::Failed(error.to_string()),
                    }
                },
            );
            return;
        };
        self.start(ctx, false, "Saving project locally…", move || {
            let Ok(mut store) = store.lock() else {
                return Message::Failed(
                    "Project worker failed. Reopen the project before saving.".into(),
                );
            };
            if let Some(summary) = summary {
                match store.save_version(&snapshot, &summary) {
                    Ok(version) => Message::Saved {
                        snapshot,
                        version: Some(version),
                        warning: None,
                    },
                    Err(error) if !store.is_dirty(&snapshot) => Message::Saved {
                        snapshot,
                        version: None,
                        warning: Some(error.to_string()),
                    },
                    Err(error) => Message::Failed(error.to_string()),
                }
            } else {
                match store.save(&snapshot) {
                    Ok(()) => Message::Saved {
                        snapshot,
                        version: None,
                        warning: None,
                    },
                    Err(error) => Message::Failed(error.to_string()),
                }
            }
        });
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        loading: bool,
        status: &str,
        task: impl FnOnce() -> Message + Send + 'static,
    ) {
        if self.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-project".into())
            .spawn(move || {
                let _ = sender.send(task());
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(Pending { receiver, loading });
                self.status = status.into();
                self.error = None;
            }
            Err(error) => self.error = Some(format!("Could not start project worker: {error}")),
        }
    }
    /// Returns true only when loading a different project, so UI selection can reset.
    pub fn poll(&mut self, ctx: &egui::Context) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        let message = match pending.receiver.try_recv() {
            Ok(message) => message,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return false;
            }
            Err(mpsc::TryRecvError::Disconnected) => Message::Failed(
                "Project worker stopped unexpectedly. Your draft is still in this window.".into(),
            ),
        };
        self.pending = None;
        self.accept(message)
    }
    fn accept(&mut self, message: Message) -> bool {
        match message {
            Message::Opened { store, studio } => {
                let folder = store
                    .lock()
                    .expect("New project store lock")
                    .root()
                    .to_owned();
                match Editor::new(studio.clone()) {
                    Ok(editor) => {
                        self.editor = editor;
                        self.baseline = studio;
                        self.store = Some(store);
                        self.folder = Some(folder);
                        self.error = None;
                        self.status =
                            "Project opened. Saves and versions stay on this computer.".into();
                        return true;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            Message::Attached {
                store,
                snapshot,
                warning,
            } => {
                self.folder = Some(
                    store
                        .lock()
                        .expect("New project store lock")
                        .root()
                        .to_owned(),
                );
                self.store = Some(store);
                self.baseline = snapshot;
                self.status = "Project saved locally.".into();
                self.error = warning;
            }
            Message::Saved {
                snapshot,
                version,
                warning,
            } => {
                // A save ACK belongs to its snapshot, never to a newer draft.
                self.baseline = snapshot;
                self.status = match version {
                    Some(version) if version.created => format!(
                        "Local version {} saved.",
                        &version.commit[..version.commit.len().min(8)]
                    ),
                    Some(_) => "Already at the latest local version.".into(),
                    None => "Project saved locally.".into(),
                };
                self.error = warning;
            }
            Message::Cancelled => {
                self.status = "Folder selection cancelled. Your project is unchanged.".into()
            }
            Message::Failed(error) => {
                self.error = Some(error);
                self.status = "Operation failed. Your draft remains in this window.".into();
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_ack_does_not_replace_or_mark_a_newer_draft_clean() {
        let mut workspace = Workspace::default();
        workspace.edit(Edit::Rename("First edit".into()));
        let snapshot = workspace.studio().clone();
        workspace.edit(Edit::Rename("Typed while saving".into()));
        workspace.accept(Message::Saved {
            snapshot,
            version: None,
            warning: None,
        });
        assert_eq!(workspace.studio().name(), "Typed while saving");
        assert!(workspace.dirty());
        assert!(workspace.editor.undo());
        assert!(!workspace.dirty());
    }
    #[test]
    fn loading_blocks_edits_and_worker_failure_keeps_draft() {
        let mut workspace = Workspace::default();
        workspace.edit(Edit::Rename("Keep this".into()));
        let (_sender, receiver) = mpsc::channel();
        workspace.pending = Some(Pending {
            receiver,
            loading: true,
        });
        assert!(workspace.edit(Edit::Rename("Lost edit".into())).is_none());
        workspace.accept(Message::Failed("Example failure".into()));
        assert_eq!(workspace.studio().name(), "Keep this");
        assert!(workspace.dirty());
    }
}
