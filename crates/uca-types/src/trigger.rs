// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trigger ids: `<kind>:<name>` (01).

use std::fmt;

use smol_str::SmolStr;

const MOUSE_NAMES: &[&str] = &["left", "right", "middle", "x1", "x2"];
const TOUCH_NAMES: &[&str] = &["tap", "long-press"];
const PLUGIN_PREFIX: &str = "plugin.";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TriggerKind {
    /// Platform-neutral key name (07).
    Key,
    /// `left | right | middle | x1 | x2`.
    Mouse,
    /// Blendshape name from a plugin with `trigger_namespace: face` (15).
    Face,
    /// Switch alias from the profile.
    Switch,
    /// `tap | long-press`.
    Touch,
    /// Plugin-defined trigger; holds the plugin id.
    Plugin(SmolStr),
}

/// A validated trigger id. Build it with [`TriggerId::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TriggerId {
    kind: TriggerKind,
    name: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TriggerIdError {
    #[error("trigger id {0:?} has no ':' between kind and name")]
    MissingSeparator(String),
    #[error("trigger id {0:?} has an empty name")]
    EmptyName(String),
    #[error("trigger id {0:?} has an empty plugin id")]
    EmptyPluginId(String),
    #[error("trigger id {id:?} has unknown kind {kind:?}")]
    UnknownKind { id: String, kind: String },
    #[error("trigger id {id:?}: {name:?} is not one of {allowed:?}")]
    UnknownName {
        id: String,
        name: String,
        allowed: &'static [&'static str],
    },
}

impl TriggerId {
    pub fn parse(s: &str) -> Result<Self, TriggerIdError> {
        let (kind_str, name) = s
            .split_once(':')
            .ok_or_else(|| TriggerIdError::MissingSeparator(s.to_owned()))?;
        if name.is_empty() {
            return Err(TriggerIdError::EmptyName(s.to_owned()));
        }
        let kind = match kind_str {
            "key" => TriggerKind::Key,
            "mouse" => TriggerKind::Mouse,
            "face" => TriggerKind::Face,
            "switch" => TriggerKind::Switch,
            "touch" => TriggerKind::Touch,
            other => match other.strip_prefix(PLUGIN_PREFIX) {
                Some("") => return Err(TriggerIdError::EmptyPluginId(s.to_owned())),
                Some(plugin) => TriggerKind::Plugin(plugin.into()),
                None => {
                    return Err(TriggerIdError::UnknownKind {
                        id: s.to_owned(),
                        kind: other.to_owned(),
                    })
                }
            },
        };
        let allowed = match kind {
            TriggerKind::Mouse => Some(MOUSE_NAMES),
            TriggerKind::Touch => Some(TOUCH_NAMES),
            _ => None,
        };
        if let Some(allowed) = allowed {
            if !allowed.contains(&name) {
                return Err(TriggerIdError::UnknownName {
                    id: s.to_owned(),
                    name: name.to_owned(),
                    allowed,
                });
            }
        }
        Ok(Self {
            kind,
            name: name.into(),
        })
    }

    pub fn kind(&self) -> &TriggerKind {
        &self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for TriggerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TriggerKind::Key => write!(f, "key:{}", self.name),
            TriggerKind::Mouse => write!(f, "mouse:{}", self.name),
            TriggerKind::Face => write!(f, "face:{}", self.name),
            TriggerKind::Switch => write!(f, "switch:{}", self.name),
            TriggerKind::Touch => write!(f, "touch:{}", self.name),
            TriggerKind::Plugin(id) => write!(f, "{PLUGIN_PREFIX}{id}:{}", self.name),
        }
    }
}

impl std::str::FromStr for TriggerId {
    type Err = TriggerIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_examples() {
        for (s, kind, name) in [
            ("key:F13", TriggerKind::Key, "F13"),
            ("mouse:right", TriggerKind::Mouse, "right"),
            ("face:jawOpen", TriggerKind::Face, "jawOpen"),
            ("switch:1", TriggerKind::Switch, "1"),
            ("switch:bridge/3", TriggerKind::Switch, "bridge/3"),
            ("touch:long-press", TriggerKind::Touch, "long-press"),
            (
                "plugin.com.example.gazepoint:wink",
                TriggerKind::Plugin("com.example.gazepoint".into()),
                "wink",
            ),
        ] {
            let id = TriggerId::parse(s).expect(s);
            assert_eq!(id.kind(), &kind, "{s}");
            assert_eq!(id.name(), name, "{s}");
            assert_eq!(id.to_string(), s, "round trip");
        }
    }

    #[test]
    fn name_may_contain_colons() {
        let id = TriggerId::parse("plugin.x:a:b").expect("valid");
        assert_eq!(id.name(), "a:b");
        assert_eq!(id.to_string(), "plugin.x:a:b");
    }

    #[test]
    fn rejects_malformed_ids() {
        assert!(matches!(
            TriggerId::parse("F13"),
            Err(TriggerIdError::MissingSeparator(_))
        ));
        assert!(matches!(
            TriggerId::parse("key:"),
            Err(TriggerIdError::EmptyName(_))
        ));
        assert!(matches!(
            TriggerId::parse("plugin.:x"),
            Err(TriggerIdError::EmptyPluginId(_))
        ));
        assert!(matches!(
            TriggerId::parse("plugin:x"),
            Err(TriggerIdError::UnknownKind { .. })
        ));
        assert!(matches!(
            TriggerId::parse("joystick:a"),
            Err(TriggerIdError::UnknownKind { .. })
        ));
        assert!(matches!(
            TriggerId::parse("mouse:x3"),
            Err(TriggerIdError::UnknownName { .. })
        ));
        assert!(matches!(
            TriggerId::parse("touch:swipe"),
            Err(TriggerIdError::UnknownName { .. })
        ));
    }
}
