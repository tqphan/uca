// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Board files: board, layers, items, groups, templates, bindings (02).

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};

use crate::action::{ActionSeq, KeyArg, Mode, MonitorRef, MonitorRefArg, Repeat};
use crate::values::{
    is_id, Behavior, Border, Color, DwellIndicator, Font, Four, Groups, Id, Length, Opacity,
    Position, Selection, Shadow, Size, States, Style,
};

/// Adds the base properties shared by board, layer and item (02) to a struct.
macro_rules! with_base {
    ($(#[$m:meta])* pub struct $name:ident { $($body:tt)* }) => {
        $(#[$m])*
        pub struct $name {
            pub position: Option<Position>,
            pub size: Option<Size>,
            pub opacity: Option<Opacity>,
            pub visible: Option<bool>,
            pub background: Option<Color>,
            pub border: Option<Border>,
            pub padding: Option<Four<Length>>,
            pub font: Option<Font>,
            pub shadow: Option<Shadow>,
            pub behavior: Option<Behavior>,
            pub dwell_indicator: Option<DwellIndicator>,
            #[serde(default)]
            pub states: States,
            pub group: Option<Groups>,
            pub extends: Option<String>,
            $($body)*
        }
    };
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardFile {
    pub schema: String,
    pub board: Board,
    #[serde(default)]
    pub groups: BTreeMap<String, Group>,
    #[serde(default)]
    pub templates: BTreeMap<String, Item>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub layers: Vec<Layer>,
}

with_base! {
    #[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub struct Board {
        pub id: Id,
        pub title: Option<String>,
        pub monitor: Option<BoardMonitor>,
        pub anchor: Option<Anchor>,
        pub reserve_space: Option<bool>,
        pub click_through: Option<ClickThrough>,
        pub area: Option<Area>,
        pub ignore_covered: Option<bool>,
        pub theme: Option<String>,
        pub show_action_sets: Option<bool>,
    }
}

/// `primary | <index> | "<monitor name>" | follow-pointer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardMonitor {
    Monitor(MonitorRef),
    FollowPointer,
}

impl<'de> Deserialize<'de> for BoardMonitor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let MonitorRefArg(m) = MonitorRefArg::deserialize(d)?;
        Ok(match m {
            MonitorRef::Name(n) if n == "follow-pointer" => BoardMonitor::FollowPointer,
            m => BoardMonitor::Monitor(m),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    Top,
    Bottom,
    Left,
    Right,
    Center,
    Free,
}

impl Anchor {
    /// Docked against a monitor edge.
    pub fn is_edge(self) -> bool {
        matches!(
            self,
            Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right
        )
    }
}

impl Board {
    /// The board's anchor, `center` when absent (02).
    pub fn anchor_or_default(&self) -> Anchor {
        self.anchor.unwrap_or(Anchor::Center)
    }

    /// `click_through`, `none` when absent (02, decided).
    pub fn click_through_or_default(&self) -> ClickThrough {
        self.click_through.unwrap_or(ClickThrough::None)
    }

    /// Whether the board reserves screen space (02, decided): only with an edge
    /// anchor, and by default there.
    pub fn reserves_space(&self) -> bool {
        self.anchor_or_default().is_edge() && self.reserve_space != Some(false)
    }

    /// `ignore_covered`, `true` when absent (02, decided).
    pub fn ignore_covered_or_default(&self) -> bool {
        self.ignore_covered.unwrap_or(true)
    }

    /// What the board is laid out in (02, decided): `work` by default for a
    /// reserving board; always `full` for any other.
    pub fn layout_area(&self) -> Area {
        if self.reserves_space() {
            self.area.unwrap_or(Area::Work)
        } else {
            Area::Full
        }
    }
}

/// `area: full | work` (02, decided).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Area {
    /// The monitor's full bounds: the board may cover the taskbar.
    Full,
    /// The monitor's work area: the taskbar stays visible.
    Work,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ClickThrough {
    None,
    OutsideItems,
    All,
}

with_base! {
    #[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub struct Layer {
        pub id: Id,
        /// Required (02).
        pub layout: Layout,
        #[serde(default)]
        pub bindings: Vec<Binding>,
        #[serde(default, deserialize_with = "de_items")]
        #[schemars(schema_with = "crate::schema::items_schema")]
        pub items: Vec<Item>,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(rename = "type")]
    pub kind: LayoutKind,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub gap: Option<Length>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutKind {
    Free,
    Grid,
    Row,
    Column,
}

with_base! {
    /// An item, a template (a partial item) or an empty grid cell (`- empty`).
    #[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub struct Item {
        pub id: Option<Id>,
        pub label: Option<String>,
        pub label_shift: Option<String>,
        pub icon: Option<String>,
        pub tooltip: Option<String>,
        pub span: Option<Span>,
        pub cell: Option<Cell>,
        pub action: Option<ActionSeq>,
        pub actions: Option<Actions>,
        #[serde(default)]
        pub bindings: Vec<Binding>,
        /// A game zone in a free layer (12).
        pub zone: Option<bool>,
        /// Holds a grid cell without an item.
        pub empty: Option<bool>,
        /// Word-prediction slot n, 1-based (16); no `action` / `actions` with it.
        pub prediction: Option<std::num::NonZeroU32>,
    }
}

/// `items:` entries may be `- empty` for an empty grid cell.
pub(crate) fn de_items<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Item>, D::Error> {
    struct Entry(Item);
    impl<'de> Deserialize<'de> for Entry {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Entry;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("an item or empty")
                }
                fn visit_str<E: de::Error>(self, v: &str) -> Result<Entry, E> {
                    if v == "empty" {
                        Ok(Entry(Item {
                            empty: Some(true),
                            ..Item::default()
                        }))
                    } else {
                        Err(E::custom(format!("expected an item or empty, found {v:?}")))
                    }
                }
                fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Entry, A::Error> {
                    Item::deserialize(de::value::MapAccessDeserializer::new(map)).map(Entry)
                }
            }
            d.deserialize_any(V)
        }
    }
    Ok(Vec::<Entry>::deserialize(d)?
        .into_iter()
        .map(|e| e.0)
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub cols: u32,
    pub rows: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub col: u32,
    pub row: u32,
}

/// One action set's meaning of an item (04 "Action sets").
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionSetEntry {
    pub start: Option<ActionSeq>,
    pub repeat: Option<Repeat>,
    pub stop: Option<ActionSeq>,
    pub label: Option<String>,
    pub label_shift: Option<String>,
    pub icon: Option<String>,
    pub tooltip: Option<String>,
    pub behavior: Option<Behavior>,
}

/// An item's `actions:` map, in file order. The shorthand with stage names
/// directly under `actions` is stored as the `default` entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Actions(pub Vec<(Id, ActionSetEntry)>);

/// Names that may not be action set ids, because the shorthand uses them (04).
pub const RESERVED_SET_NAMES: &[&str] = &[
    "start",
    "repeat",
    "stop",
    "label",
    "label_shift",
    "icon",
    "tooltip",
    "behavior",
];

impl<'de> Deserialize<'de> for Actions {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Actions;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of action set ids to stages, or stages directly")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Actions, A::Error> {
                let mut sets: Vec<(Id, ActionSetEntry)> = Vec::new();
                let mut short = ActionSetEntry::default();
                let mut has_short = false;
                while let Some(key) = map.next_key::<String>()? {
                    if RESERVED_SET_NAMES.contains(&key.as_str()) {
                        has_short = true;
                        let dup = match key.as_str() {
                            "start" => short.start.replace(map.next_value()?).is_some(),
                            "repeat" => short.repeat.replace(map.next_value()?).is_some(),
                            "stop" => short.stop.replace(map.next_value()?).is_some(),
                            "label" => short.label.replace(map.next_value()?).is_some(),
                            "label_shift" => short.label_shift.replace(map.next_value()?).is_some(),
                            "icon" => short.icon.replace(map.next_value()?).is_some(),
                            "tooltip" => short.tooltip.replace(map.next_value()?).is_some(),
                            _ => short.behavior.replace(map.next_value()?).is_some(),
                        };
                        if dup {
                            return Err(de::Error::custom(format!("duplicate key {key:?}")));
                        }
                    } else {
                        if !is_id(&key) {
                            return Err(de::Error::custom(format!(
                                "invalid action set id {key:?}: use only a-z, 0-9, '_' and '-'"
                            )));
                        }
                        if sets.iter().any(|(id, _)| id.0 == key) {
                            return Err(de::Error::custom(format!(
                                "action set {key:?} appears twice"
                            )));
                        }
                        sets.push((Id(key), map.next_value()?));
                    }
                }
                match (has_short, sets.is_empty()) {
                    (true, false) => Err(de::Error::custom(
                        "actions mixes stage names (start, repeat, stop, …) with action set ids; \
                         put the stages under `default:` instead",
                    )),
                    (true, true) => Ok(Actions(vec![(Id("default".into()), short)])),
                    (false, _) => Ok(Actions(sets)),
                }
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Group {
    #[serde(default)]
    pub style: Style,
    pub behavior: Option<Behavior>,
    pub dwell_indicator: Option<DwellIndicator>,
}

/// `when: <pattern> [in: <context>] do: <actions>` or with stages (04 "Bindings").
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Input pattern (04 "Pattern DSL"); parsed during validation.
    pub when: String,
    #[serde(rename = "in")]
    pub context: Option<ContextFilter>,
    #[serde(rename = "do")]
    pub do_: Option<ActionSeq>,
    pub start: Option<ActionSeq>,
    pub repeat: Option<Repeat>,
    pub stop: Option<ActionSeq>,
    pub behavior: Option<Behavior>,
    pub passthrough: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextFilter {
    pub mode: Option<Mode>,
    /// The top board's resolved selection (04).
    pub selection: Option<Selection>,
    pub board: Option<Id>,
    pub layer: Option<Id>,
    #[serde(default, deserialize_with = "de_modifier")]
    #[schemars(schema_with = "crate::schema::modifier_schema")]
    pub latched: Option<uca_types::Key>,
    /// Foreground app id (platform-dependent).
    pub app: Option<String>,
    pub action_set: Option<Id>,
}

fn de_modifier<'de, D: Deserializer<'de>>(d: D) -> Result<Option<uca_types::Key>, D::Error> {
    let key = KeyArg::deserialize(d)?.0;
    if key.is_modifier() {
        Ok(Some(key))
    } else {
        Err(de::Error::custom(format!(
            "latched needs a modifier key (shift, ctrl, alt, meta), not {key}"
        )))
    }
}
