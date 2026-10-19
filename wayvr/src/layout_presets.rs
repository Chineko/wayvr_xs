//! Named snapshots of the existing overlay state. No capture UUIDs are stored.
use crate::config::AutoState;
use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use wlx_common::config::GeneralConfig;

#[derive(Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub state: AutoState,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Library {
    pub version: u32,
    pub presets: Vec<Preset>,
    pub startup: Option<String>,
    pub pending: Option<String>,
}
#[derive(Clone)]
pub enum Action {
    Save(String),
    Replace(String),
    Rename(String, String),
    Delete(String),
    Startup(Option<String>),
    Load(String),
}
pub fn path() -> PathBuf {
    wlx_common::config_io::get_config_root().join("layout-presets.json")
}
impl Library {
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let data = match std::fs::read(path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let library: Self = serde_json::from_slice(&data)
            .context("Cannot read layout presets; original file was preserved")?;
        ensure!(library.version <= 1, "Unsupported preset version");
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for p in &library.presets {
            ensure!(
                !p.id.is_empty() && ids.insert(&p.id),
                "Duplicate or empty preset ID"
            );
            ensure!(
                names.insert(valid_name(&p.name)?.to_lowercase()),
                "Duplicate preset name"
            );
            ensure!(
                p.state.sets.iter().any(|s| s.name.as_ref() != "global"),
                "Preset has no workspace"
            );
        }
        for id in [&library.startup, &library.pending].into_iter().flatten() {
            library.find(id)?;
        }
        Ok(library)
    }
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }
    pub fn find(&self, id: &str) -> anyhow::Result<&Preset> {
        self.presets
            .iter()
            .find(|p| p.id == id)
            .context("Preset no longer exists")
    }
    pub fn change(
        &mut self,
        action: &Action,
        snapshot: Option<AutoState>,
    ) -> anyhow::Result<String> {
        self.version = 1;
        match action {
            Action::Save(name) => {
                let name = valid_name(name)?;
                ensure!(
                    !self
                        .presets
                        .iter()
                        .any(|p| p.name.eq_ignore_ascii_case(&name)),
                    "A preset with that name already exists"
                );
                let id = uuid::Uuid::new_v4().to_string();
                self.presets.push(Preset {
                    id,
                    name,
                    state: snapshot.context("No layout snapshot")?,
                });
                Ok("Preset saved".into())
            }
            Action::Replace(id) => {
                let state = snapshot.context("No layout snapshot")?;
                self.find(id)?;
                self.presets.iter_mut().find(|p| &p.id == id).unwrap().state = state;
                Ok("Preset replaced".into())
            }
            Action::Rename(id, name) => {
                let name = valid_name(name)?;
                self.find(id)?;
                ensure!(
                    !self
                        .presets
                        .iter()
                        .any(|p| &p.id != id && p.name.eq_ignore_ascii_case(&name)),
                    "A preset with that name already exists"
                );
                self.presets.iter_mut().find(|p| &p.id == id).unwrap().name = name;
                Ok("Preset renamed".into())
            }
            Action::Delete(id) => {
                self.find(id)?;
                self.presets.retain(|p| &p.id != id);
                if self.startup.as_ref() == Some(id) {
                    self.startup = None;
                }
                if self.pending.as_ref() == Some(id) {
                    self.pending = None;
                }
                Ok("Preset deleted".into())
            }
            Action::Startup(id) => {
                if let Some(id) = id {
                    self.find(id)?;
                }
                self.startup.clone_from(id);
                Ok("Startup choice saved".into())
            }
            Action::Load(id) => {
                self.find(id)?;
                self.pending = Some(id.clone());
                Ok("Loading preset; restarting WayVR".into())
            }
        }
    }
}
fn valid_name(name: &str) -> anyhow::Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
        bail!("Use a name of 1–48 characters");
    }
    Ok(name.to_owned())
}
pub fn apply_on_startup(config: &mut GeneralConfig) -> anyhow::Result<()> {
    apply_from(&path(), config)
}
fn apply_from(path: &Path, config: &mut GeneralConfig) -> anyhow::Result<()> {
    let mut library = Library::read(path)?;
    let Some(id) = library.pending.as_ref().or(library.startup.as_ref()) else {
        return Ok(());
    };
    let state = library.find(id)?.state.clone();
    if library.pending.take().is_some() {
        library.write(path)?;
    }
    state.apply(config);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> AutoState {
        let mut hidden = std::collections::HashMap::new();
        let panel = wlx_common::windowing::OverlayWindowState {
            alpha: 0.6,
            curvature: Some(0.2),
            grabbable: false,
            ..Default::default()
        };
        hidden.insert("XSApp-test".into(), panel);
        AutoState {
            app_panels: vec![wlx_common::app_panel::SavedAppPanel {
                name: "XSApp-test".into(),
                app: Some(wlx_common::app_panel::AppIdentity {
                    app_id: "desktop:browser".into(),
                    title: "Browser".into(),
                }),
            }],
            screen_splits: vec![],
            sets: vec![wlx_common::config::SerializedWindowSet {
                name: "Main".into(),
                overlays: Default::default(),
                hidden_overlays: hidden,
            }],
            spawn_overlays: vec![],
            last_set: 0,
            attribs: Default::default(),
        }
    }
    #[test]
    fn lifecycle_keeps_identity_and_clears_deleted_references() {
        let mut library = Library::default();
        library
            .change(&Action::Save(" Gaming ".into()), Some(snapshot()))
            .unwrap();
        let id = library.presets[0].id.clone();
        assert_eq!(library.presets[0].name, "Gaming");
        assert!(
            library
                .change(&Action::Save("gaming".into()), Some(snapshot()))
                .is_err()
        );
        library
            .change(&Action::Rename(id.clone(), "Media".into()), None)
            .unwrap();
        let mut changed = snapshot();
        changed.sets[0]
            .hidden_overlays
            .get_mut("XSApp-test")
            .unwrap()
            .alpha = 0.8;
        library
            .change(&Action::Replace(id.clone()), Some(changed))
            .unwrap();
        assert_eq!(
            library.find(&id).unwrap().state.sets[0].hidden_overlays["XSApp-test"].alpha,
            0.8
        );
        library
            .change(&Action::Startup(Some(id.clone())), None)
            .unwrap();
        library.change(&Action::Load(id.clone()), None).unwrap();
        library.change(&Action::Delete(id), None).unwrap();
        assert!(
            library.presets.is_empty() && library.startup.is_none() && library.pending.is_none()
        );
    }
    #[test]
    fn storage_roundtrip_and_corruption_preserve_original() {
        let path =
            std::env::temp_dir().join(format!("wayvr-presets-{}.json", uuid::Uuid::new_v4()));
        let mut library = Library::default();
        library
            .change(&Action::Save("MEDIA".into()), Some(snapshot()))
            .unwrap();
        library.write(&path).unwrap();
        let restored = Library::read(&path).unwrap();
        let panel = &restored.presets[0].state;
        assert_eq!(
            panel.app_panels[0].app.as_ref().unwrap().app_id,
            "desktop:browser"
        );
        assert!(!panel.sets[0].hidden_overlays["XSApp-test"].grabbable);
        assert_eq!(
            panel.sets[0].hidden_overlays["XSApp-test"].curvature,
            Some(0.2)
        );
        std::fs::write(&path, b"broken preset file").unwrap();
        assert!(Library::read(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken preset file");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn pending_load_overrides_startup_once_without_changing_settings() {
        let path =
            std::env::temp_dir().join(format!("wayvr-startup-{}.json", uuid::Uuid::new_v4()));
        let mut library = Library::default();
        library
            .change(&Action::Save("FIRST".into()), Some(snapshot()))
            .unwrap();
        let first = library.presets[0].id.clone();
        let mut second = snapshot();
        second.sets[0]
            .hidden_overlays
            .get_mut("XSApp-test")
            .unwrap()
            .alpha = 0.9;
        library
            .change(&Action::Save("SECOND".into()), Some(second))
            .unwrap();
        let second = library.presets[1].id.clone();
        library.change(&Action::Startup(Some(first)), None).unwrap();
        library.change(&Action::Load(second), None).unwrap();
        library.write(&path).unwrap();
        let mut config: GeneralConfig = serde_json::from_str("{}").unwrap();
        let theme = config.theme_path.clone();
        apply_from(&path, &mut config).unwrap();
        assert_eq!(config.sets[0].hidden_overlays["XSApp-test"].alpha, 0.9);
        assert_eq!(config.theme_path, theme);
        assert!(Library::read(&path).unwrap().pending.is_none());
        apply_from(&path, &mut config).unwrap();
        assert_eq!(config.sets[0].hidden_overlays["XSApp-test"].alpha, 0.6);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn invalid_names_and_missing_presets_are_rejected() {
        let mut library = Library::default();
        for name in ["", "   ", "bad\nname"] {
            assert!(
                library
                    .change(&Action::Save(name.into()), Some(snapshot()))
                    .is_err()
            );
        }
        for action in [
            Action::Load("missing".into()),
            Action::Delete("missing".into()),
            Action::Startup(Some("missing".into())),
        ] {
            assert!(library.change(&action, None).is_err());
        }
    }
}
