//! Stable app-panel definitions; compositor window UUIDs are session-local.
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIdentity {
	pub app_id: String,
	pub title: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedAppPanel {
	pub name: Arc<str>,
	#[serde(default)]
	pub app: Option<AppIdentity>,
}

/// Reconnect only when app identity identifies one window, using an exact title
/// to distinguish multiple windows. Never fall back to titles across apps.
pub fn reconnect_index(saved: &AppIdentity, choices: &[AppIdentity]) -> Option<usize> {
	if saved.app_id.is_empty() {
		return None;
	}
	let apps: Vec<_> = choices
		.iter()
		.enumerate()
		.filter(|(_, w)| w.app_id == saved.app_id)
		.collect();
	let exact: Vec<_> = apps.iter().filter(|(_, w)| w.title == saved.title).collect();
	if exact.len() == 1 {
		return Some(exact[0].0);
	}
	if apps.len() == 1 {
		return Some(apps[0].0);
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;
	fn app(id: &str, title: &str) -> AppIdentity {
		AppIdentity {
			app_id: id.into(),
			title: title.into(),
		}
	}
	#[test]
	fn reconnect_requires_unique_app_identity() {
		let saved = app("browser", "Work");
		assert_eq!(reconnect_index(&saved, &[app("discord", "Work")]), None);
		assert_eq!(reconnect_index(&saved, &[app("browser", "Renamed")]), Some(0));
		assert_eq!(
			reconnect_index(&saved, &[app("browser", "Home"), saved.clone()]),
			Some(1)
		);
		assert_eq!(reconnect_index(&saved, &[saved.clone(), saved.clone()]), None);
		assert_eq!(
			reconnect_index(&saved, &[app("browser", "Home"), app("browser", "Other")]),
			None
		);
		assert_eq!(reconnect_index(&app("", "Work"), &[app("", "Work")]), None);
	}
	#[test]
	fn saved_panel_round_trips_without_session_uuid() {
		let panel = SavedAppPanel {
			name: "XSApp-test".into(),
			app: Some(app("browser", "Work")),
		};
		let json = serde_json::to_string(&panel).unwrap();
		assert!(!json.contains("uuid"));
		let restored: SavedAppPanel = serde_json::from_str(&json).unwrap();
		assert_eq!(restored.app, panel.app);
	}
	#[test]
	fn panel_identity_stays_linked_to_hidden_layout_and_attributes() {
		use crate::{
			config::SerializedWindowSet,
			windowing::{OverlayWindowState, Positioning},
		};
		use glam::{Affine3A, Quat, Vec3};
		let panel = SavedAppPanel {
			name: "XSApp-layout".into(),
			app: Some(app("browser", "Work")),
		};
		let pose =
			Affine3A::from_scale_rotation_translation(Vec3::splat(0.8), Quat::from_rotation_y(0.2), Vec3::new(1., 0.5, -2.));
		let state = OverlayWindowState {
			saved_transform: Some(pose),
			alpha: 0.7,
			curvature: Some(0.3),
			positioning: Positioning::Static,
			..Default::default()
		};
		let set = SerializedWindowSet {
			name: "global".into(),
			overlays: Default::default(),
			hidden_overlays: [(panel.name.clone(), state)].into(),
		};
		let saved = serde_json::to_vec(&(vec![panel], vec![set])).unwrap();
		let (panels, sets): (Vec<SavedAppPanel>, Vec<SerializedWindowSet>) = serde_json::from_slice(&saved).unwrap();
		let restored = &sets[0].hidden_overlays[&panels[0].name];
		assert!(restored.saved_transform.unwrap().abs_diff_eq(pose, 0.0001));
		assert!(matches!(restored.positioning, Positioning::Static));
		assert!((restored.alpha - 0.7).abs() < 0.0001);
		assert_eq!(restored.curvature, Some(0.3));
		assert!(sets[0].overlays.is_empty());
	}
}
