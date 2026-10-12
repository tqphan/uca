// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Actions in config: `{ <action-name>: <args> }` (04 "Actions", 02 "M1 actions").

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use uca_types::suggest::closest;
use uca_types::Key;

use crate::values::{Id, Length};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    KeyTap(Key),
    KeyDown(Key),
    KeyUp(Key),
    KeyHold(Key),
    KeyRelease(Key),
    KeyToggle(Key),
    /// A modifier key.
    KeyLatch(Key),
    TextType(String),
    MouseClick(MouseClick),
    MouseDown(MouseButton),
    MouseUp(MouseButton),
    MouseMove(MouseMove),
    MouseScroll(MouseScroll),
    BoardOpen(Id),
    BoardBack,
    BoardReplace(Id),
    LayerShow(Id),
    LayerHide(Id),
    LayerToggle(Id),
    ActionSetHold(Id),
    ActionSetOn(Id),
    ActionSetOff(Id),
    ActionSetToggle(Id),
    ActionSetSwitch(Id),
    ActionSetCycle(Vec<Id>),
    ActionSetOnce(Id),
    ActionSetReset,
    ModeSet(Mode),
    Select,
    Pause,
    Resume,
    Recalibrate,
    /// Milliseconds; inside sequences only.
    Delay(u64),
    MacroRun(Id),
    AppLaunch(AppLaunch),
    WindowFocus(WindowQuery),
    ClipboardCopy,
    ClipboardPaste,
    ClipboardCut,
    DebugSession(DebugSession),
    DebugOverlay,
    DebugRecord,
    DebugExport,
}

/// Every action name accepted in M1, for "Did you mean" suggestions.
pub const ACTION_NAMES: &[&str] = &[
    "key.tap",
    "key.down",
    "key.up",
    "key.hold",
    "key.release",
    "key.toggle",
    "key.latch",
    "text.type",
    "mouse.click",
    "mouse.down",
    "mouse.up",
    "mouse.move",
    "mouse.scroll",
    "board.open",
    "board.back",
    "board.replace",
    "layer.show",
    "layer.hide",
    "layer.toggle",
    "action_set.hold",
    "action_set.on",
    "action_set.off",
    "action_set.toggle",
    "action_set.switch",
    "action_set.cycle",
    "action_set.once",
    "action_set.reset",
    "mode.set",
    "select",
    "pause",
    "resume",
    "recalibrate",
    "delay",
    "macro.run",
    "app.launch",
    "window.focus",
    "clipboard.copy",
    "clipboard.paste",
    "clipboard.cut",
    "debug.session",
    "debug.overlay",
    "debug.record",
    "debug.export",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseClick {
    pub button: MouseButton,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MouseScroll {
    /// Notches.
    #[serde(default)]
    pub dx: f32,
    /// Notches.
    #[serde(default)]
    pub dy: f32,
}

/// Which monitor a position refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorRef {
    Primary,
    Index(u32),
    Name(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MouseMove {
    /// `space: logical` (default): on `monitor`, or the monitor with the pointer when `None`.
    Logical {
        x: Length,
        y: Length,
        relative: bool,
        monitor: Option<MonitorRef>,
    },
    /// `space: physical`: virtual-desktop physical px.
    Physical { x: f64, y: f64, relative: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
pub enum Mode {
    Direct,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppLaunch {
    pub path: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowQuery {
    TitleContains(String),
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DebugLevel {
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugSession {
    pub minutes: u32,
    pub level: DebugLevel,
    pub sensitive: bool,
}

/// One action or a list; stored as a list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionSeq(pub Vec<Action>);

/// A `repeat` stage: a sequence, or `start` to rerun the start sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum Repeat {
    Seq(ActionSeq),
    Start,
}

// ---------------------------------------------------------------------------
// Deserialisation

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(ActionVisitor)
    }
}

struct ActionVisitor;

impl<'de> Visitor<'de> for ActionVisitor {
    type Value = Action;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an action: { <action-name>: <args> }")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Action, A::Error> {
        let name: String = map
            .next_key()?
            .ok_or_else(|| de::Error::custom("empty action: expected { <action-name>: <args> }"))?;
        let action = action_args(&name, &mut map)?;
        if map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::custom(
                "an action has exactly one name; use a list for several actions",
            ));
        }
        Ok(action)
    }
}

fn action_args<'de, A: MapAccess<'de>>(name: &str, map: &mut A) -> Result<Action, A::Error> {
    macro_rules! arg {
        ($ty:ty) => {
            map.next_value::<$ty>()?
        };
    }
    macro_rules! none {
        ($variant:expr) => {{
            map.next_value::<NoArgs>()?;
            $variant
        }};
    }
    Ok(match name {
        "key.tap" => Action::KeyTap(arg!(KeyArg).0),
        "key.down" => Action::KeyDown(arg!(KeyArg).0),
        "key.up" => Action::KeyUp(arg!(KeyArg).0),
        "key.hold" => Action::KeyHold(arg!(KeyArg).0),
        "key.release" => Action::KeyRelease(arg!(KeyArg).0),
        "key.toggle" => Action::KeyToggle(arg!(KeyArg).0),
        "key.latch" => {
            let key = arg!(KeyArg).0;
            if !key.is_modifier() {
                return Err(de::Error::custom(format!(
                    "key.latch needs a modifier key (shift, ctrl, alt, meta), not {key}"
                )));
            }
            Action::KeyLatch(key)
        }
        "text.type" => Action::TextType(arg!(String)),
        "mouse.click" => Action::MouseClick(arg!(MouseClickArg).0),
        "mouse.down" => Action::MouseDown(arg!(MouseButton)),
        "mouse.up" => Action::MouseUp(arg!(MouseButton)),
        "mouse.move" => Action::MouseMove(arg!(MouseMoveArg).0),
        "mouse.scroll" => Action::MouseScroll(arg!(MouseScroll)),
        "board.open" => Action::BoardOpen(arg!(Id)),
        "board.back" => none!(Action::BoardBack),
        "board.replace" => Action::BoardReplace(arg!(Id)),
        "layer.show" => Action::LayerShow(arg!(Id)),
        "layer.hide" => Action::LayerHide(arg!(Id)),
        "layer.toggle" => Action::LayerToggle(arg!(Id)),
        "action_set.hold" => Action::ActionSetHold(arg!(Id)),
        "action_set.on" => Action::ActionSetOn(arg!(Id)),
        "action_set.off" => Action::ActionSetOff(arg!(Id)),
        "action_set.toggle" => Action::ActionSetToggle(arg!(Id)),
        "action_set.switch" => Action::ActionSetSwitch(arg!(Id)),
        "action_set.cycle" => Action::ActionSetCycle(arg!(Vec<Id>)),
        "action_set.once" => Action::ActionSetOnce(arg!(Id)),
        "action_set.reset" => none!(Action::ActionSetReset),
        "mode.set" => Action::ModeSet(arg!(Mode)),
        "select" => none!(Action::Select),
        "pause" => none!(Action::Pause),
        "resume" => none!(Action::Resume),
        "recalibrate" => none!(Action::Recalibrate),
        "delay" => Action::Delay(arg!(u64)),
        "macro.run" => Action::MacroRun(arg!(Id)),
        "app.launch" => Action::AppLaunch(arg!(AppLaunch)),
        "window.focus" => Action::WindowFocus(arg!(WindowQueryArg).0),
        "clipboard.copy" => none!(Action::ClipboardCopy),
        "clipboard.paste" => none!(Action::ClipboardPaste),
        "clipboard.cut" => none!(Action::ClipboardCut),
        "debug.session" => Action::DebugSession(arg!(DebugSessionArg).0),
        "debug.overlay" => none!(Action::DebugOverlay),
        "debug.record" => none!(Action::DebugRecord),
        "debug.export" => none!(Action::DebugExport),
        other => {
            let hint = closest(other, ACTION_NAMES.iter().copied())
                .map(|s| format!(". Did you mean {s:?}?"))
                .unwrap_or_default();
            return Err(de::Error::custom(format!("unknown action {other:?}{hint}")));
        }
    })
}

/// `null` or `{}`.
struct NoArgs;

impl<'de> Deserialize<'de> for NoArgs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NoArgs;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("no arguments ({} or nothing)")
            }
            fn visit_unit<E: de::Error>(self) -> Result<NoArgs, E> {
                Ok(NoArgs)
            }
            fn visit_none<E: de::Error>(self) -> Result<NoArgs, E> {
                Ok(NoArgs)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<NoArgs, A::Error> {
                match map.next_key::<String>()? {
                    None => Ok(NoArgs),
                    Some(k) => Err(de::Error::custom(format!(
                        "this action takes no arguments, found {k:?}"
                    ))),
                }
            }
        }
        d.deserialize_any(V)
    }
}

/// A key name (07); YAML may give `1` as a number.
pub(crate) struct KeyArg(pub Key);

impl<'de> Deserialize<'de> for KeyArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = KeyArg;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a key name")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<KeyArg, E> {
                Key::parse(v).map(KeyArg).map_err(E::custom)
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<KeyArg, E> {
                self.visit_str(&v.to_string())
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<KeyArg, E> {
                self.visit_str(&v.to_string())
            }
        }
        d.deserialize_any(V)
    }
}

/// `left` or `{button: left, count: 1}`; both fields default as shown.
struct MouseClickArg(MouseClick);

impl<'de> Deserialize<'de> for MouseClickArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Full {
            button: Option<MouseButton>,
            count: Option<u32>,
        }
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Button(MouseButton),
            Full(Full),
        }
        let (button, count) = match Repr::deserialize(d) {
            Ok(Repr::Button(b)) => (b, 1),
            Ok(Repr::Full(f)) => (f.button.unwrap_or(MouseButton::Left), f.count.unwrap_or(1)),
            Err(_) => {
                return Err(de::Error::custom(
                    "expected a mouse button (left, right, middle, x1, x2) or {button, count}",
                ))
            }
        };
        if count == 0 {
            return Err(de::Error::custom("mouse.click count must be at least 1"));
        }
        Ok(MouseClickArg(MouseClick { button, count }))
    }
}

struct MouseMoveArg(MouseMove);

impl<'de> Deserialize<'de> for MouseMoveArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "kebab-case")]
        enum Space {
            Logical,
            Physical,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            x: Length,
            y: Length,
            #[serde(default)]
            relative: bool,
            monitor: Option<MonitorRefArg>,
            space: Option<Space>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(MouseMoveArg(match raw.space.unwrap_or(Space::Logical) {
            Space::Logical => MouseMove::Logical {
                x: raw.x,
                y: raw.y,
                relative: raw.relative,
                monitor: raw.monitor.map(|m| m.0),
            },
            Space::Physical => {
                if raw.monitor.is_some() {
                    return Err(de::Error::custom(
                        "mouse.move with space: physical cannot name a monitor",
                    ));
                }
                let (Length::Px(x), Length::Px(y)) = (raw.x, raw.y) else {
                    return Err(de::Error::custom(
                        "mouse.move with space: physical takes plain pixel numbers for x and y",
                    ));
                };
                MouseMove::Physical {
                    x,
                    y,
                    relative: raw.relative,
                }
            }
        }))
    }
}

/// `primary`, an index, or a monitor name.
pub(crate) struct MonitorRefArg(pub MonitorRef);

impl<'de> Deserialize<'de> for MonitorRefArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = MonitorRefArg;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("primary, a monitor index or a monitor name")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<MonitorRefArg, E> {
                u32::try_from(v)
                    .map(|i| MonitorRefArg(MonitorRef::Index(i)))
                    .map_err(|_| E::custom("monitor index is too large"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<MonitorRefArg, E> {
                Ok(MonitorRefArg(if v == "primary" {
                    MonitorRef::Primary
                } else {
                    MonitorRef::Name(v.to_owned())
                }))
            }
        }
        d.deserialize_any(V)
    }
}

/// `next` or `{title_contains: "..."}`.
struct WindowQueryArg(WindowQuery);

impl<'de> Deserialize<'de> for WindowQueryArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Title {
            title_contains: String,
        }
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Word(String),
            Title(Title),
        }
        match Repr::deserialize(d) {
            Ok(Repr::Word(w)) if w == "next" => Ok(WindowQueryArg(WindowQuery::Next)),
            Ok(Repr::Title(t)) => Ok(WindowQueryArg(WindowQuery::TitleContains(t.title_contains))),
            _ => Err(de::Error::custom(
                "expected next or {title_contains: \"...\"}",
            )),
        }
    }
}

/// `{minutes, level, sensitive}`, defaults 60 / debug / false (11); `{}` or nothing for all defaults.
struct DebugSessionArg(DebugSession);

impl<'de> Deserialize<'de> for DebugSessionArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize, Default)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            minutes: Option<u32>,
            level: Option<DebugLevel>,
            sensitive: Option<bool>,
        }
        let raw = Option::<Raw>::deserialize(d)?.unwrap_or_default();
        let minutes = raw.minutes.unwrap_or(60);
        if minutes == 0 {
            return Err(de::Error::custom(
                "debug.session minutes must be at least 1",
            ));
        }
        Ok(DebugSessionArg(DebugSession {
            minutes,
            level: raw.level.unwrap_or(DebugLevel::Debug),
            sensitive: raw.sensitive.unwrap_or(false),
        }))
    }
}

impl<'de> Deserialize<'de> for ActionSeq {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = ActionSeq;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an action or a list of actions")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ActionSeq, A::Error> {
                ActionVisitor.visit_map(map).map(|a| ActionSeq(vec![a]))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ActionSeq, A::Error> {
                let mut out = Vec::new();
                while let Some(a) = seq.next_element::<Action>()? {
                    out.push(a);
                }
                Ok(ActionSeq(out))
            }
        }
        d.deserialize_any(V)
    }
}

impl<'de> Deserialize<'de> for Repeat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Repeat;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an action, a list of actions, or start")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Repeat, E> {
                if v == "start" {
                    Ok(Repeat::Start)
                } else {
                    Err(E::custom(format!(
                        "repeat must be an action, a list of actions, or start; found {v:?}"
                    )))
                }
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Repeat, A::Error> {
                ActionVisitor
                    .visit_map(map)
                    .map(|a| Repeat::Seq(ActionSeq(vec![a])))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Repeat, A::Error> {
                let mut out = Vec::new();
                while let Some(a) = seq.next_element::<Action>()? {
                    out.push(a);
                }
                Ok(Repeat::Seq(ActionSeq(out)))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(s: &str) -> Result<Action, String> {
        serde_norway::from_str(s).map_err(|e| e.to_string())
    }

    fn id(s: &str) -> Id {
        Id(s.into())
    }

    #[test]
    fn key_actions() {
        assert_eq!(act("key.tap: q"), Ok(Action::KeyTap(Key::KeyQ)));
        assert_eq!(act("key.tap: 1"), Ok(Action::KeyTap(Key::Digit1)));
        assert_eq!(act("key.down: shift"), Ok(Action::KeyDown(Key::ShiftLeft)));
        assert_eq!(act("key.hold: w"), Ok(Action::KeyHold(Key::KeyW)));
        assert_eq!(
            act("key.latch: ctrl"),
            Ok(Action::KeyLatch(Key::ControlLeft))
        );
        assert!(act("key.latch: q")
            .unwrap_err()
            .contains("needs a modifier key"));
        let err = act("key.tap: entr").unwrap_err();
        assert!(err.contains("Did you mean \"Enter\""), "{err}");
        assert!(
            act("key.tap: { key: s, mods: [ctrl] }").is_err(),
            "mods were removed (04)"
        );
    }

    #[test]
    fn unknown_action_suggests() {
        let err = act("key.tapp: q").unwrap_err();
        assert!(
            err.contains("unknown action \"key.tapp\". Did you mean \"key.tap\"?"),
            "{err}"
        );
        let err = act("speech.say: hi").unwrap_err();
        assert!(err.contains("unknown action \"speech.say\""), "{err}");
    }

    #[test]
    fn one_name_per_action() {
        let err = act("{ key.tap: q, key.tap: w }").unwrap_err();
        assert!(!err.is_empty());
        let err = act("{ key.tap: q, text.type: hi }").unwrap_err();
        assert!(err.contains("exactly one name"), "{err}");
    }

    #[test]
    fn no_arg_actions_take_null_or_empty_map() {
        assert_eq!(act("board.back:"), Ok(Action::BoardBack));
        assert_eq!(act("board.back: {}"), Ok(Action::BoardBack));
        assert_eq!(act("select: {}"), Ok(Action::Select));
        assert!(act("pause: { now: true }").is_err());
    }

    #[test]
    fn mouse_actions() {
        assert_eq!(
            act("mouse.click: right"),
            Ok(Action::MouseClick(MouseClick {
                button: MouseButton::Right,
                count: 1
            }))
        );
        assert_eq!(
            act("mouse.click: { count: 2 }"),
            Ok(Action::MouseClick(MouseClick {
                button: MouseButton::Left,
                count: 2
            }))
        );
        assert!(act("mouse.click: { count: 0 }").is_err());
        assert_eq!(
            act("mouse.down: x1"),
            Ok(Action::MouseDown(MouseButton::X1))
        );
        assert_eq!(
            act("mouse.scroll: { dy: -3 }"),
            Ok(Action::MouseScroll(MouseScroll { dx: 0.0, dy: -3.0 }))
        );
    }

    #[test]
    fn mouse_move_spaces() {
        assert_eq!(
            act("mouse.move: { x: 50%, y: 50%, monitor: primary }"),
            Ok(Action::MouseMove(MouseMove::Logical {
                x: Length::Percent(50.0),
                y: Length::Percent(50.0),
                relative: false,
                monitor: Some(MonitorRef::Primary),
            }))
        );
        assert_eq!(
            act("mouse.move: { x: 10, y: -5, relative: true, monitor: 1 }"),
            Ok(Action::MouseMove(MouseMove::Logical {
                x: Length::Px(10.0),
                y: Length::Px(-5.0),
                relative: true,
                monitor: Some(MonitorRef::Index(1)),
            }))
        );
        assert_eq!(
            act("mouse.move: { x: 2400, y: 300, space: physical }"),
            Ok(Action::MouseMove(MouseMove::Physical {
                x: 2400.0,
                y: 300.0,
                relative: false
            }))
        );
        assert!(act("mouse.move: { x: 1, y: 1, space: physical, monitor: primary }").is_err());
        assert!(act("mouse.move: { x: 50%, y: 1, space: physical }").is_err());
    }

    #[test]
    fn action_set_and_board_actions() {
        assert_eq!(
            act("board.open: aac-home"),
            Ok(Action::BoardOpen(id("aac-home")))
        );
        assert_eq!(
            act("layer.toggle: letters"),
            Ok(Action::LayerToggle(id("letters")))
        );
        assert_eq!(
            act("action_set.cycle: [default, sprint, build]"),
            Ok(Action::ActionSetCycle(vec![
                id("default"),
                id("sprint"),
                id("build")
            ]))
        );
        assert_eq!(act("action_set.reset:"), Ok(Action::ActionSetReset));
        assert!(act("board.open: Main Board").is_err());
    }

    #[test]
    fn other_actions() {
        assert_eq!(act("mode.set: Paused"), Ok(Action::ModeSet(Mode::Paused)));
        assert!(act("mode.set: AutoScan").is_err(), "not built in M1");
        assert_eq!(act("delay: 200"), Ok(Action::Delay(200)));
        assert_eq!(act("macro.run: greet"), Ok(Action::MacroRun(id("greet"))));
        assert_eq!(
            act("text.type: \"Hello \""),
            Ok(Action::TextType("Hello ".into()))
        );
        assert_eq!(
            act("app.launch: { path: notepad.exe }"),
            Ok(Action::AppLaunch(AppLaunch {
                path: "notepad.exe".into(),
                args: vec![]
            }))
        );
        assert_eq!(
            act("window.focus: next"),
            Ok(Action::WindowFocus(WindowQuery::Next))
        );
        assert_eq!(
            act("window.focus: { title_contains: Notepad }"),
            Ok(Action::WindowFocus(WindowQuery::TitleContains(
                "Notepad".into()
            )))
        );
        assert_eq!(act("clipboard.paste:"), Ok(Action::ClipboardPaste));
        assert_eq!(
            act("debug.session: {}"),
            Ok(Action::DebugSession(DebugSession {
                minutes: 60,
                level: DebugLevel::Debug,
                sensitive: false
            }))
        );
        assert_eq!(
            act("debug.session: { minutes: 15, level: trace }"),
            Ok(Action::DebugSession(DebugSession {
                minutes: 15,
                level: DebugLevel::Trace,
                sensitive: false
            }))
        );
    }

    #[test]
    fn sequences_and_repeat() {
        let seq: ActionSeq = serde_norway::from_str("key.tap: a").expect("one");
        assert_eq!(seq.0.len(), 1);
        let seq: ActionSeq =
            serde_norway::from_str("[ { key.down: a }, { delay: 500 }, { key.down: b } ]")
                .expect("list");
        assert_eq!(
            seq.0,
            vec![
                Action::KeyDown(Key::KeyA),
                Action::Delay(500),
                Action::KeyDown(Key::KeyB)
            ]
        );
        let r: Repeat = serde_norway::from_str("start").expect("start");
        assert_eq!(r, Repeat::Start);
        let r: Repeat = serde_norway::from_str("key.tap: f1").expect("seq");
        assert_eq!(r, Repeat::Seq(ActionSeq(vec![Action::KeyTap(Key::F1)])));
        assert!(serde_norway::from_str::<Repeat>("stop").is_err());
    }

    #[test]
    fn action_names_cover_every_variant() {
        for name in ACTION_NAMES {
            let err = act(&format!("{name}: 1")).err().unwrap_or_default();
            assert!(!err.contains("unknown action"), "{name}: {err}");
        }
    }
}
