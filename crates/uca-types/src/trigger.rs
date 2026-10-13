// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trigger ids: `<kind>:<name>` (01).

use std::fmt;

use smol_str::SmolStr;

use crate::key::{Key, KeyError};

const MOUSE_NAMES: &[&str] = &["left", "right", "middle", "x1", "x2"];
const TOUCH_NAMES: &[&str] = &["tap", "long-press"];
const PLUGIN_PREFIX: &str = "plugin.";
/// The name of the any-key wildcard, `key:*`.
pub const ANY_KEY: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TriggerKind {
    /// Platform-neutral key (07); the name is stored as its W3C code.
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
    #[error("trigger id {id:?}: {source}")]
    UnknownKey { id: String, source: KeyError },
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
        // Key names are stored as their W3C code, so `key:f13`, `key:F13` and
        // `key:shift` / `key:ShiftLeft` are the same trigger (07).
        let name = match kind {
            // `key:*` matches any keyboard key (04).
            TriggerKind::Key if name == ANY_KEY => name.into(),
            TriggerKind::Key => Key::parse(name)
                .map_err(|source| TriggerIdError::UnknownKey {
                    id: s.to_owned(),
                    source,
                })?
                .code()
                .into(),
            _ => name.into(),
        };
        Ok(Self { kind, name })
    }

    /// The key, for `key:` triggers.
    pub fn key(&self) -> Option<Key> {
        match self.kind {
            TriggerKind::Key => Key::parse(&self.name).ok(),
            _ => None,
        }
    }

    /// `key:*`: matches any keyboard key.
    pub fn is_any_key(&self) -> bool {
        self.kind == TriggerKind::Key && self.name == ANY_KEY
    }

    /// Whether a concrete trigger is matched by this id (`key:*` matches every key).
    pub fn matches(&self, other: &TriggerId) -> bool {
        self == other || (self.is_any_key() && other.kind == TriggerKind::Key)
    }

    pub fn kind(&self) -> &TriggerKind {
        &self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Written as its id string, e.g. `"key:KeyQ"` (11 recordings).
impl serde::Serialize for TriggerId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for TriggerId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        TriggerId::parse(&text).map_err(serde::de::Error::custom)
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
            assert_eq!(id.key().is_some(), kind == TriggerKind::Key, "{s}");
        }
    }

    #[test]
    fn key_names_are_normalised() {
        let a = TriggerId::parse("key:f13").expect("valid");
        assert_eq!(a, TriggerId::parse("key:F13").expect("valid"));
        assert_eq!(a.to_string(), "key:F13");
        assert_eq!(a.key(), Some(Key::F13));
        let shift = TriggerId::parse("key:shift").expect("valid");
        assert_eq!(shift, TriggerId::parse("key:ShiftLeft").expect("valid"));
        assert_ne!(shift, TriggerId::parse("key:ShiftRight").expect("valid"));
        assert!(matches!(
            TriggerId::parse("key:entr"),
            Err(TriggerIdError::UnknownKey { .. })
        ));
    }

    #[test]
    fn any_key_wildcard() {
        let any = TriggerId::parse("key:*").expect("valid");
        assert!(any.is_any_key());
        assert_eq!(any.key(), None);
        assert_eq!(any.to_string(), "key:*");
        let f13 = TriggerId::parse("key:F13").expect("valid");
        assert!(any.matches(&f13));
        assert!(!f13.matches(&any));
        assert!(!any.matches(&TriggerId::parse("mouse:left").expect("valid")));
        assert!(TriggerId::parse("mouse:*").is_err());
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
