// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! JSON Schema for every config file, generated from the Rust types (02).
//!
//! The schema is for editors, the board editor and docs. Loading never
//! validates against it: typed deserialisation is the runtime check (02).
//! Types with hand-written deserialisers get hand-written schemas here.

use std::borrow::Cow;

use schemars::generate::SchemaSettings;
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde_json::{json, Value};

use crate::action::{
    Action, ActionSeq, AppLaunch, DebugLevel, Mode, MouseButton, MouseScroll, Repeat,
};
use crate::app::AppFile;
use crate::board::{ActionSetEntry, Actions, BoardFile, BoardMonitor, Item, RESERVED_SET_NAMES};
use crate::profile::ProfileFile;
use crate::settings_registry::{self, OffEncoding};
use crate::theme::ThemeFile;
use crate::values::{Color, FontWeight, Four, Groups, Id, Length, Opacity};

/// Where the generated schema is committed, relative to the repo root.
pub const SCHEMA_PATH: &str = "schema/uca-1.schema.json";

/// The schema for all four file kinds: board, profile, theme and `app.yaml`.
pub fn json_schema() -> Schema {
    let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
    let roots = [
        generator.subschema_for::<BoardFile>(),
        generator.subschema_for::<ProfileFile>(),
        generator.subschema_for::<ThemeFile>(),
        generator.subschema_for::<AppFile>(),
    ];
    let defs = generator.take_definitions(true);
    json_schema!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "UCA configuration (uca/1)",
        "description": "Board, profile, theme and app.yaml files. Generated from the Rust types in uca-config; do not edit.",
        "anyOf": roots,
        "$defs": defs,
    })
}

/// The schema as committed: pretty JSON with a trailing newline.
pub fn json_schema_text() -> String {
    let mut text = serde_json::to_string_pretty(&json_schema()).unwrap_or_default();
    text.push('\n');
    text
}

/// Schema of a registry setting: its number type and range, plus its "off" form.
pub(crate) fn setting_schema(key: &str) -> Schema {
    let Ok(def) = settings_registry::get(key) else {
        return json_schema!({});
    };
    let kind = if def.unit.is_integer() {
        "integer"
    } else {
        "number"
    };
    let range = json!({
        "type": kind,
        "minimum": def.min,
        "maximum": def.max,
        "description": format!("Setting `{}` ({})", def.key, def.unit),
    });
    match def.off {
        None => Schema::try_from(range).unwrap_or_default(),
        Some(OffEncoding::Zero) => {
            json_schema!({ "anyOf": [{ "const": 0, "description": "off" }, range] })
        }
        Some(OffEncoding::Null) => {
            json_schema!({ "anyOf": [{ "type": "null", "description": "off" }, range] })
        }
    }
}

macro_rules! simple_schema {
    ($ty:ty, $name:literal, $schema:tt) => {
        impl JsonSchema for $ty {
            fn schema_name() -> Cow<'static, str> {
                $name.into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!($schema)
            }
        }
    };
}

simple_schema!(Length, "Length", {
    "description": "A number (logical px), \"12px\", \"50%\" or \"2grid\".",
    "anyOf": [
        { "type": "number" },
        { "type": "string", "pattern": "^\\s*-?[0-9]+(\\.[0-9]+)?\\s*(px|%|grid)?\\s*$" }
    ]
});

simple_schema!(Color, "Color", {
    "description": "\"#rgb\", \"#rrggbb\", \"#rrggbbaa\", \"rgba(r,g,b,a)\" or a theme variable \"$name\".",
    "type": "string",
    "pattern": "^\\s*(#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})|rgba\\(.*\\)|\\$[a-z0-9_-]+)\\s*$"
});

simple_schema!(Id, "Id", {
    "type": "string",
    "pattern": "^[a-z0-9_-]+$"
});

simple_schema!(FontWeight, "FontWeight", {
    "type": "integer",
    "minimum": 100,
    "maximum": 900
});

simple_schema!(Opacity, "Opacity", {
    "type": "number",
    "minimum": 0,
    "maximum": 1
});

simple_schema!(Groups, "Groups", {
    "description": "A group name or a list of group names.",
    "anyOf": [
        { "type": "string" },
        { "type": "array", "items": { "type": "string" } }
    ]
});

simple_schema!(BoardMonitor, "BoardMonitor", {
    "description": "primary, follow-pointer, a monitor index, or a monitor name.",
    "anyOf": [
        { "type": "integer", "minimum": 0 },
        { "type": "string" }
    ]
});

impl<T: JsonSchema> JsonSchema for Four<T> {
    fn schema_name() -> Cow<'static, str> {
        format!("Four_{}", T::schema_name()).into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let one = generator.subschema_for::<T>();
        json_schema!({
            "description": "One value for all four sides or corners, or a list of four.",
            "anyOf": [one, { "type": "array", "items": one, "minItems": 4, "maxItems": 4 }]
        })
    }
}

fn key_schema() -> Value {
    json!({
        "description": "A key name: a W3C code in any case (KeyA, F13) or an alias (q, shift, enter).",
        "type": ["string", "integer"]
    })
}

fn monitor_ref_schema() -> Value {
    json!({
        "description": "primary, a monitor index, or a monitor name.",
        "anyOf": [{ "type": "integer", "minimum": 0 }, { "type": "string" }]
    })
}

fn no_args_schema() -> Value {
    json!({
        "description": "No arguments.",
        "anyOf": [{ "type": "null" }, { "type": "object", "maxProperties": 0 }]
    })
}

impl JsonSchema for Action {
    fn schema_name() -> Cow<'static, str> {
        "Action".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let key = key_schema();
        let id = Value::from(generator.subschema_for::<Id>());
        let none = no_args_schema();
        let button = Value::from(generator.subschema_for::<MouseButton>());
        let length = Value::from(generator.subschema_for::<Length>());
        let args: Vec<(&str, Value)> = vec![
            ("key.tap", key.clone()),
            ("key.down", key.clone()),
            ("key.up", key.clone()),
            ("key.hold", key.clone()),
            ("key.release", key.clone()),
            ("key.toggle", key.clone()),
            ("key.latch", key),
            ("text.type", json!({ "type": "string" })),
            (
                "mouse.click",
                json!({ "anyOf": [button.clone(), {
                    "type": "object",
                    "properties": { "button": button, "count": { "type": "integer", "minimum": 1 } },
                    "additionalProperties": false
                }] }),
            ),
            ("mouse.down", button.clone()),
            ("mouse.up", button),
            (
                "mouse.move",
                json!({
                    "type": "object",
                    "properties": {
                        "x": length, "y": length,
                        "relative": { "type": "boolean" },
                        "monitor": monitor_ref_schema(),
                        "space": { "enum": ["logical", "physical"] }
                    },
                    "required": ["x", "y"],
                    "additionalProperties": false
                }),
            ),
            (
                "mouse.scroll",
                generator.subschema_for::<MouseScroll>().into(),
            ),
            ("board.open", id.clone()),
            ("board.back", none.clone()),
            ("board.replace", id.clone()),
            ("layer.show", id.clone()),
            ("layer.hide", id.clone()),
            ("layer.toggle", id.clone()),
            ("action_set.hold", id.clone()),
            ("action_set.on", id.clone()),
            ("action_set.off", id.clone()),
            ("action_set.toggle", id.clone()),
            ("action_set.switch", id.clone()),
            ("action_set.cycle", json!({ "type": "array", "items": id })),
            ("action_set.once", id.clone()),
            ("action_set.reset", none.clone()),
            ("mode.set", generator.subschema_for::<Mode>().into()),
            ("select", none.clone()),
            ("pause", none.clone()),
            ("resume", none.clone()),
            ("recalibrate", none.clone()),
            ("settings.open", none.clone()),
            ("settings.close", none.clone()),
            (
                "delay",
                json!({ "type": "integer", "minimum": 0, "description": "Milliseconds." }),
            ),
            ("macro.run", id),
            ("app.launch", generator.subschema_for::<AppLaunch>().into()),
            (
                "window.focus",
                json!({ "anyOf": [{ "const": "next" }, {
                    "type": "object",
                    "properties": { "title_contains": { "type": "string" } },
                    "required": ["title_contains"],
                    "additionalProperties": false
                }] }),
            ),
            ("clipboard.copy", none.clone()),
            ("clipboard.paste", none.clone()),
            ("clipboard.cut", none.clone()),
            (
                "debug.session",
                json!({ "anyOf": [{ "type": "null" }, {
                    "type": "object",
                    "properties": {
                        "minutes": { "type": "integer", "minimum": 1 },
                        "level": generator.subschema_for::<DebugLevel>(),
                        "sensitive": { "type": "boolean" }
                    },
                    "additionalProperties": false
                }] }),
            ),
            ("debug.overlay", none.clone()),
            ("debug.record", none.clone()),
            ("debug.export", none.clone()),
            ("speech.say", json!({ "type": "string" })),
            ("speech.speak_buffer", none.clone()),
            ("speech.buffer.append", json!({ "type": "string" })),
            ("speech.buffer.backspace", none.clone()),
            ("speech.buffer.clear", none.clone()),
            ("speech.buffer.word_back", none.clone()),
            ("speech.compose.on", none.clone()),
            ("speech.compose.off", none.clone()),
            ("speech.compose.toggle", none.clone()),
            ("speech.repeat", none.clone()),
            ("speech.stop", none),
            (
                "speech.voice",
                json!({ "type": "string", "minLength": 1, "description": "An engine voice id." }),
            ),
        ];
        let variants: Vec<Value> = args
            .into_iter()
            .map(|(name, schema)| {
                json!({
                    "type": "object",
                    "properties": { name: schema },
                    "required": [name],
                    "additionalProperties": false
                })
            })
            .collect();
        json_schema!({
            "description": "An action: { <action-name>: <args> }.",
            "oneOf": variants
        })
    }
}

impl JsonSchema for ActionSeq {
    fn schema_name() -> Cow<'static, str> {
        "ActionSeq".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let action = generator.subschema_for::<Action>();
        json_schema!({
            "description": "An action or a list of actions.",
            "anyOf": [action, { "type": "array", "items": action }]
        })
    }
}

impl JsonSchema for Repeat {
    fn schema_name() -> Cow<'static, str> {
        "Repeat".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let seq = generator.subschema_for::<ActionSeq>();
        json_schema!({
            "description": "An action, a list of actions, or start (rerun the start sequence).",
            "anyOf": [{ "const": "start" }, seq]
        })
    }
}

impl JsonSchema for Actions {
    fn schema_name() -> Cow<'static, str> {
        "Actions".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let entry = Value::from(generator.subschema_for::<ActionSetEntry>());
        let shorthand = entry.clone();
        json_schema!({
            "description": "Action set id → stages, or the stages directly (shorthand for `default`).",
            "anyOf": [
                shorthand,
                {
                    "type": "object",
                    "propertyNames": { "pattern": "^[a-z0-9_-]+$", "not": { "enum": RESERVED_SET_NAMES } },
                    "additionalProperties": entry
                }
            ]
        })
    }
}

/// `items:`: items or `empty`.
pub(crate) fn items_schema(generator: &mut SchemaGenerator) -> Schema {
    let item = generator.subschema_for::<Item>();
    json_schema!({
        "type": "array",
        "items": { "anyOf": [item, { "const": "empty" }] }
    })
}

/// `switches:`: alias → trigger id.
pub(crate) fn switches_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "propertyNames": { "pattern": "^[a-z0-9_-]+$" },
        "additionalProperties": {
            "type": "string",
            "pattern": "^(key|mouse|face|switch|touch|plugin\\.[^:]+):.+$"
        }
    })
}

/// `fonts:`: family → weight → path or `system`.
pub(crate) fn fonts_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {
            "type": "object",
            "propertyNames": { "pattern": "^[1-9]00$" },
            "additionalProperties": { "type": "string" }
        }
    })
}

/// A modifier key name.
pub(crate) fn modifier_schema(_: &mut SchemaGenerator) -> Schema {
    Schema::try_from(key_schema()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn committed_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(SCHEMA_PATH)
    }

    /// Keeps `schema/uca-1.schema.json` in sync with the types (02).
    /// Run with `UCA_UPDATE_SCHEMA=1` to rewrite it after changing a type.
    #[test]
    fn committed_schema_is_up_to_date() {
        let fresh = json_schema_text();
        let path = committed_path();
        if std::env::var_os("UCA_UPDATE_SCHEMA").is_some() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).expect("create schema dir");
            }
            std::fs::write(&path, &fresh).expect("write schema");
            return;
        }
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            committed == fresh,
            "{SCHEMA_PATH} is out of date; run `UCA_UPDATE_SCHEMA=1 cargo test -p uca-config` and commit it"
        );
    }

    #[test]
    fn schema_has_every_file_kind_and_action() {
        let schema = serde_json::to_value(json_schema()).expect("serialise");
        assert_eq!(schema["anyOf"].as_array().map(Vec::len), Some(4));
        let text = schema.to_string();
        for name in crate::action::ACTION_NAMES {
            assert!(
                text.contains(&format!("\"{name}\"")),
                "{name} missing from schema"
            );
        }
        let dwell = setting_schema("behavior.dwell_ms");
        let dwell = serde_json::to_value(dwell).expect("serialise");
        assert_eq!(dwell["anyOf"][0]["const"], 0);
        assert_eq!(dwell["anyOf"][1]["minimum"], 100.0);
    }
}
