// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Style and behavior cascade for items (02 "Style cascade").
//!
//! Lowest to highest priority: built-in defaults, theme, template chain,
//! groups in listed order, board and layer inheritable keys (`font`,
//! `behavior`), the item itself, then active states in fixed order.
//! Maps merge deeply; lists and scalars are replaced.

use std::collections::BTreeMap;

use crate::board::{ActionSetEntry, Board, BoardFile, Group, Item, Layer};
use crate::settings_registry::{self, SettingValue};
use crate::theme::ThemeFile;
use crate::values::{
    Align, Behavior, Border, BorderStyle, Color, DwellIndicator, DwellIndicatorKind, Font, Four,
    Groups, Hold, Length, Opacity, Shadow, State, States, Style,
};

/// The group an item belongs to when it names none (02).
pub const OTHERS_GROUP: &str = "others";

/// State overrides apply in this order; later wins (02 cascade level 8).
pub const STATE_ORDER: &[State] = &[
    State::Disabled,
    State::Latched,
    State::Hover,
    State::Dwell,
    State::Active,
];

// ------------------------------------------------------------------ merging

/// Deep merge: `over` wins where it sets a value.
pub(crate) trait Merge {
    fn merge_from(&mut self, over: &Self);
}

fn replace<T: Clone>(base: &mut Option<T>, over: &Option<T>) {
    if over.is_some() {
        base.clone_from(over);
    }
}

fn deep<T: Merge + Clone>(base: &mut Option<T>, over: &Option<T>) {
    match (base.as_mut(), over) {
        (Some(b), Some(o)) => b.merge_from(o),
        (None, Some(o)) => *base = Some(o.clone()),
        _ => {}
    }
}

impl Merge for Border {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.width, &o.width);
        replace(&mut self.color, &o.color);
        replace(&mut self.radius, &o.radius);
        replace(&mut self.style, &o.style);
    }
}

impl Merge for Font {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.family, &o.family);
        replace(&mut self.size, &o.size);
        replace(&mut self.weight, &o.weight);
        replace(&mut self.color, &o.color);
        replace(&mut self.align, &o.align);
    }
}

impl Merge for Shadow {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.x, &o.x);
        replace(&mut self.y, &o.y);
        replace(&mut self.blur, &o.blur);
        replace(&mut self.color, &o.color);
    }
}

impl Merge for Style {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.opacity, &o.opacity);
        replace(&mut self.background, &o.background);
        deep(&mut self.border, &o.border);
        replace(&mut self.padding, &o.padding);
        deep(&mut self.font, &o.font);
        deep(&mut self.shadow, &o.shadow);
    }
}

impl Merge for Behavior {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.dwell_ms, &o.dwell_ms);
        replace(&mut self.dwell_grace_ms, &o.dwell_grace_ms);
        replace(&mut self.cooldown_ms, &o.cooldown_ms);
        replace(&mut self.hold, &o.hold);
        replace(&mut self.repeat_delay_ms, &o.repeat_delay_ms);
        replace(&mut self.repeat_interval_ms, &o.repeat_interval_ms);
        replace(&mut self.confirm, &o.confirm);
        replace(&mut self.confirm_window_ms, &o.confirm_window_ms);
        replace(&mut self.enter_delay_ms, &o.enter_delay_ms);
        replace(&mut self.active_flash_ms, &o.active_flash_ms);
    }
}

impl Merge for DwellIndicator {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.kind, &o.kind);
        replace(&mut self.color, &o.color);
        replace(&mut self.width, &o.width);
    }
}

impl Merge for States {
    fn merge_from(&mut self, o: &Self) {
        for (state, style) in o {
            self.entry(*state).or_default().merge_from(style);
        }
    }
}

impl Merge for Group {
    fn merge_from(&mut self, o: &Self) {
        self.style.merge_from(&o.style);
        deep(&mut self.behavior, &o.behavior);
        deep(&mut self.dwell_indicator, &o.dwell_indicator);
    }
}

impl Merge for ActionSetEntry {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.start, &o.start);
        replace(&mut self.repeat, &o.repeat);
        replace(&mut self.stop, &o.stop);
        replace(&mut self.label, &o.label);
        replace(&mut self.label_shift, &o.label_shift);
        replace(&mut self.icon, &o.icon);
        replace(&mut self.tooltip, &o.tooltip);
        deep(&mut self.behavior, &o.behavior);
    }
}

impl Merge for crate::board::Actions {
    fn merge_from(&mut self, o: &Self) {
        for (id, entry) in &o.0 {
            match self.0.iter_mut().find(|(i, _)| i == id) {
                Some((_, e)) => e.merge_from(entry),
                None => self.0.push((id.clone(), entry.clone())),
            }
        }
    }
}

impl Merge for Item {
    fn merge_from(&mut self, o: &Self) {
        replace(&mut self.position, &o.position);
        replace(&mut self.size, &o.size);
        replace(&mut self.opacity, &o.opacity);
        replace(&mut self.visible, &o.visible);
        replace(&mut self.background, &o.background);
        deep(&mut self.border, &o.border);
        replace(&mut self.padding, &o.padding);
        deep(&mut self.font, &o.font);
        deep(&mut self.shadow, &o.shadow);
        deep(&mut self.behavior, &o.behavior);
        deep(&mut self.dwell_indicator, &o.dwell_indicator);
        self.states.merge_from(&o.states);
        replace(&mut self.group, &o.group);
        replace(&mut self.extends, &o.extends);
        replace(&mut self.id, &o.id);
        replace(&mut self.label, &o.label);
        replace(&mut self.label_shift, &o.label_shift);
        replace(&mut self.icon, &o.icon);
        replace(&mut self.tooltip, &o.tooltip);
        replace(&mut self.span, &o.span);
        replace(&mut self.cell, &o.cell);
        replace(&mut self.action, &o.action);
        deep(&mut self.actions, &o.actions);
        if !o.bindings.is_empty() {
            self.bindings.clone_from(&o.bindings);
        }
        replace(&mut self.zone, &o.zone);
        replace(&mut self.empty, &o.empty);
    }
}

/// The style keys of an item (its own level), without behavior or states.
fn own_style(item: &Item) -> Style {
    Style {
        opacity: item.opacity,
        background: item.background.clone(),
        border: item.border.clone(),
        padding: item.padding,
        font: item.font.clone(),
        shadow: item.shadow.clone(),
    }
}

// ------------------------------------------------------------------ resolved values

pub type Rgba = [u8; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBorder {
    pub width: Length,
    pub color: Rgba,
    /// Top-left, top-right, bottom-right, bottom-left.
    pub radius: [Length; 4],
    pub style: BorderStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFont {
    pub family: String,
    pub size: Length,
    pub weight: u16,
    pub color: Rgba,
    pub align: Align,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedShadow {
    pub x: Length,
    pub y: Length,
    pub blur: Length,
    pub color: Rgba,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStyle {
    /// Multiplied through board and layer.
    pub opacity: f64,
    pub background: Rgba,
    pub border: ResolvedBorder,
    /// Top, right, bottom, left.
    pub padding: [Length; 4],
    pub font: ResolvedFont,
    pub shadow: Option<ResolvedShadow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBehavior {
    /// `None` when dwell is off (`dwell_ms: 0`).
    pub dwell_ms: Option<f64>,
    pub dwell_grace_ms: f64,
    pub cooldown_ms: f64,
    pub hold: Hold,
    pub repeat_delay_ms: f64,
    pub repeat_interval_ms: f64,
    pub confirm: bool,
    pub confirm_window_ms: f64,
    pub enter_delay_ms: f64,
    pub active_flash_ms: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedDwellIndicator {
    pub kind: DwellIndicatorKind,
    pub color: Rgba,
    pub width: Length,
}

/// Built-in style defaults (02 cascade level 1).
pub fn builtin_style() -> Style {
    let white = Color::Rgba([255, 255, 255, 255]);
    Style {
        opacity: Some(Opacity(1.0)),
        background: Some(Color::Rgba([0, 0, 0, 255])),
        border: Some(Border {
            width: Some(Length::Px(2.0)),
            color: Some(white.clone()),
            radius: Some(Four::All(Length::Px(0.0))),
            style: Some(BorderStyle::Solid),
        }),
        padding: Some(Four::All(Length::Px(0.0))),
        font: Some(Font {
            family: Some(crate::validate::DEFAULT_FONT.to_owned()),
            size: Some(Length::Px(24.0)),
            weight: Some(crate::values::FontWeight(700)),
            color: Some(white),
            align: Some(Align::Center),
        }),
        shadow: None,
    }
}

/// Built-in dwell indicator (05): a ring, #ffcc00, 6 logical px.
pub fn builtin_dwell_indicator() -> DwellIndicator {
    DwellIndicator {
        kind: Some(DwellIndicatorKind::Ring),
        color: Some(Color::Rgba([0xff, 0xcc, 0x00, 255])),
        width: Some(Length::Px(6.0)),
    }
}

/// Built-in behavior defaults: registry defaults, `hold: release`, `confirm: false`.
pub fn builtin_behavior() -> Behavior {
    let default = |key: &str| settings_registry::get(key).ok().map(|d| d.default);
    Behavior {
        dwell_ms: default("behavior.dwell_ms"),
        dwell_grace_ms: default("behavior.dwell_grace_ms"),
        cooldown_ms: default("behavior.cooldown_ms"),
        hold: Some(Hold::Release),
        repeat_delay_ms: default("behavior.repeat_delay_ms"),
        repeat_interval_ms: default("behavior.repeat_interval_ms"),
        confirm: Some(false),
        confirm_window_ms: default("behavior.confirm_window_ms"),
        enter_delay_ms: default("behavior.enter_delay_ms"),
        active_flash_ms: default("behavior.active_flash_ms"),
    }
}

/// Built-in behavior with `over` merged on top (bindings' `behavior`, 04).
pub fn behavior_over_builtin(over: Option<&Behavior>) -> ResolvedBehavior {
    let mut b = builtin_behavior();
    if let Some(o) = over {
        b.merge_from(o);
    }
    resolve_behavior(&b)
}

// ------------------------------------------------------------------ the cascade

/// Cascade context for one board and its theme.
pub struct Cascade<'a> {
    board: &'a BoardFile,
    theme: Option<&'a ThemeFile>,
}

/// An item after the cascade: everything but the active states is resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedItem {
    /// The item with its template chain merged in (labels, actions, …).
    pub item: Item,
    pub behavior: ResolvedBehavior,
    pub dwell_indicator: ResolvedDwellIndicator,
    base: Style,
    states: States,
    parent_opacity: f64,
    vars: BTreeMap<String, Rgba>,
}

impl<'a> Cascade<'a> {
    pub fn new(board: &'a BoardFile, theme: Option<&'a ThemeFile>) -> Self {
        Self { board, theme }
    }

    /// A template: the board's version deep-merged over the theme's (02).
    pub fn template(&self, name: &str) -> Option<Item> {
        let theme = self.theme.and_then(|t| t.templates.get(name));
        let board = self.board.templates.get(name);
        match (theme, board) {
            (None, None) => None,
            (Some(t), None) => Some(t.clone()),
            (None, Some(b)) => Some(b.clone()),
            (Some(t), Some(b)) => {
                let mut merged = t.clone();
                merged.merge_from(b);
                Some(merged)
            }
        }
    }

    /// A group: the board's version deep-merged over the theme's (02).
    pub fn group(&self, name: &str) -> Option<Group> {
        let theme = self.theme.and_then(|t| t.groups.get(name));
        let board = self.board.groups.get(name);
        match (theme, board) {
            (None, None) => None,
            (Some(t), None) => Some(t.clone()),
            (None, Some(b)) => Some(b.clone()),
            (Some(t), Some(b)) => {
                let mut merged = t.clone();
                merged.merge_from(b);
                Some(merged)
            }
        }
    }

    /// A colour with theme variables resolved; `None` for an unknown variable.
    pub fn color(&self, c: &Color) -> Option<Rgba> {
        match c {
            Color::Rgba(rgba) => Some(*rgba),
            Color::Var(name) => self.vars().get(name).copied(),
        }
    }

    /// The merged template chain of `extends`, root first; cycles stop the chain.
    fn template_chain(&self, extends: Option<&str>) -> Item {
        let mut chain = Vec::new();
        let mut next = extends.map(str::to_owned);
        while let Some(name) = next {
            if chain.iter().any(|(n, _)| *n == name) {
                break;
            }
            let Some(t) = self.template(&name) else { break };
            next = t.extends.clone();
            chain.push((name, t));
        }
        let mut merged = Item::default();
        for (_, t) in chain.iter().rev() {
            merged.merge_from(t);
        }
        merged
    }

    fn vars(&self) -> BTreeMap<String, Rgba> {
        let Some(theme) = self.theme else {
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for name in theme.vars.keys() {
            let mut seen = Vec::new();
            let mut color = Color::Var(name.0.clone());
            while let Color::Var(n) = &color {
                if seen.contains(n) {
                    break;
                }
                seen.push(n.clone());
                match theme.vars.get(&crate::values::Id(n.clone())) {
                    Some(c) => color = c.clone(),
                    None => break,
                }
            }
            if let Color::Rgba(rgba) = color {
                out.insert(name.0.clone(), rgba);
            }
        }
        out
    }

    pub fn item(&self, layer: &Layer, item: &Item) -> ResolvedItem {
        let board: &Board = &self.board.board;
        let template = self.template_chain(item.extends.as_deref());
        let mut effective = template.clone();
        effective.merge_from(item);

        // Levels 1–2: built-in defaults (the theme adds no item style directly).
        let mut style = builtin_style();
        let mut behavior = builtin_behavior();
        let mut states = States::new();
        let mut indicator = builtin_dwell_indicator();
        deep_into(
            &mut indicator,
            self.theme.and_then(|t| t.dwell_indicator.as_ref()),
        );
        // Level 3: template chain.
        style.merge_from(&own_style(&template));
        if let Some(b) = &template.behavior {
            behavior.merge_from(b);
        }
        states.merge_from(&template.states);
        deep_into(&mut indicator, template.dwell_indicator.as_ref());
        // Level 4: groups in listed order; the last wins.
        let groups = effective
            .group
            .clone()
            .unwrap_or_else(|| Groups(vec![OTHERS_GROUP.to_owned()]));
        for name in &groups.0 {
            let group = self.group(name).or_else(|| self.group(OTHERS_GROUP));
            if let Some(g) = group {
                style.merge_from(&g.style);
                if let Some(b) = &g.behavior {
                    behavior.merge_from(b);
                }
                deep_into(&mut indicator, g.dwell_indicator.as_ref());
            }
        }
        // Levels 5–6: board and layer inheritable keys (font, behavior).
        for (font, b, ind) in [
            (&board.font, &board.behavior, &board.dwell_indicator),
            (&layer.font, &layer.behavior, &layer.dwell_indicator),
        ] {
            deep(&mut style.font, font);
            deep_into(&mut indicator, ind.as_ref());
            if let Some(b) = b {
                behavior.merge_from(b);
            }
        }
        // Level 7: the item itself.
        style.merge_from(&own_style(item));
        if let Some(b) = &item.behavior {
            behavior.merge_from(b);
        }
        states.merge_from(&item.states);
        deep_into(&mut indicator, item.dwell_indicator.as_ref());

        let parent_opacity = opacity(&board.opacity) * opacity(&layer.opacity);
        let vars = self.vars();
        let color = |c: &Option<Color>| match c {
            Some(Color::Rgba(rgba)) => *rgba,
            Some(Color::Var(name)) => vars.get(name).copied().unwrap_or([0xff, 0xcc, 0x00, 255]),
            None => [0xff, 0xcc, 0x00, 255],
        };
        let dwell_indicator = ResolvedDwellIndicator {
            kind: indicator.kind.unwrap_or(DwellIndicatorKind::Ring),
            color: color(&indicator.color),
            width: indicator.width.unwrap_or(Length::Px(6.0)),
        };
        ResolvedItem {
            item: effective,
            behavior: resolve_behavior(&behavior),
            dwell_indicator,
            base: style,
            states,
            parent_opacity,
            vars,
        }
    }
}

fn deep_into(base: &mut DwellIndicator, over: Option<&DwellIndicator>) {
    if let Some(o) = over {
        base.merge_from(o);
    }
}

fn opacity(o: &Option<Opacity>) -> f64 {
    o.map_or(1.0, |o| o.0)
}

impl ResolvedItem {
    /// The style with the active states applied in fixed order (02 level 8).
    pub fn style(&self, active: &[State]) -> ResolvedStyle {
        let mut style = self.base.clone();
        for state in STATE_ORDER {
            if active.contains(state) {
                if let Some(s) = self.states.get(state) {
                    style.merge_from(s);
                }
            }
        }
        self.resolve(&style)
    }

    /// Whether the cascade styles this state; if not, the renderer draws its built-in visual (05).
    pub fn styles_state(&self, state: State) -> bool {
        self.states.contains_key(&state)
    }

    /// Behavior for an action set entry: its `behavior` merged over the item's (04).
    pub fn behavior_for(&self, entry: &ActionSetEntry) -> ResolvedBehavior {
        let Some(over) = &entry.behavior else {
            return self.behavior.clone();
        };
        let mut b = unresolve_behavior(&self.behavior);
        b.merge_from(over);
        resolve_behavior(&b)
    }

    fn color(&self, c: &Option<Color>, fallback: Rgba) -> Rgba {
        match c {
            Some(Color::Rgba(rgba)) => *rgba,
            Some(Color::Var(name)) => self.vars.get(name).copied().unwrap_or(fallback),
            None => fallback,
        }
    }

    fn resolve(&self, s: &Style) -> ResolvedStyle {
        let defaults = builtin_style();
        let d_border = defaults.border.clone().unwrap_or_default();
        let d_font = defaults.font.clone().unwrap_or_default();
        let black = [0, 0, 0, 255];
        let white = [255, 255, 255, 255];
        let border = s.border.clone().unwrap_or_default();
        let font = s.font.clone().unwrap_or_default();
        let four = |f: Option<Four<Length>>| match f.unwrap_or(Four::All(Length::Px(0.0))) {
            Four::All(l) => [l; 4],
            Four::Each(e) => e,
        };
        ResolvedStyle {
            opacity: opacity(&s.opacity) * self.parent_opacity,
            background: self.color(&s.background, black),
            border: ResolvedBorder {
                width: border.width.or(d_border.width).unwrap_or(Length::Px(0.0)),
                color: self.color(&border.color, white),
                radius: four(border.radius.or(d_border.radius)),
                style: border.style.unwrap_or(BorderStyle::Solid),
            },
            padding: four(s.padding),
            font: ResolvedFont {
                family: font
                    .family
                    .or(d_font.family)
                    .unwrap_or_else(|| crate::validate::DEFAULT_FONT.to_owned()),
                size: font.size.or(d_font.size).unwrap_or(Length::Px(24.0)),
                weight: font.weight.or(d_font.weight).map_or(700, |w| w.0),
                color: self.color(&font.color, white),
                align: font.align.or(d_font.align).unwrap_or(Align::Center),
            },
            // A shadow exists once any level sets one; unset parts are 0 / black.
            shadow: s.shadow.as_ref().map(|sh| ResolvedShadow {
                x: sh.x.unwrap_or(Length::Px(0.0)),
                y: sh.y.unwrap_or(Length::Px(0.0)),
                blur: sh.blur.unwrap_or(Length::Px(0.0)),
                color: self.color(&sh.color, black),
            }),
        }
    }
}

fn number(v: Option<SettingValue>, key: &str) -> f64 {
    let fallback = || match settings_registry::get(key).map(|d| d.default) {
        Ok(SettingValue::Number(n)) => n,
        _ => 0.0,
    };
    match v {
        Some(SettingValue::Number(n)) => n,
        _ => fallback(),
    }
}

fn resolve_behavior(b: &Behavior) -> ResolvedBehavior {
    ResolvedBehavior {
        dwell_ms: match b.dwell_ms {
            Some(SettingValue::Off) => None,
            v => Some(number(v, "behavior.dwell_ms")),
        },
        dwell_grace_ms: number(b.dwell_grace_ms, "behavior.dwell_grace_ms"),
        cooldown_ms: number(b.cooldown_ms, "behavior.cooldown_ms"),
        hold: b.hold.unwrap_or(Hold::Release),
        repeat_delay_ms: number(b.repeat_delay_ms, "behavior.repeat_delay_ms"),
        repeat_interval_ms: number(b.repeat_interval_ms, "behavior.repeat_interval_ms"),
        confirm: b.confirm.unwrap_or(false),
        confirm_window_ms: number(b.confirm_window_ms, "behavior.confirm_window_ms"),
        enter_delay_ms: number(b.enter_delay_ms, "behavior.enter_delay_ms"),
        active_flash_ms: number(b.active_flash_ms, "behavior.active_flash_ms"),
    }
}

fn unresolve_behavior(r: &ResolvedBehavior) -> Behavior {
    let n = |v: f64| Some(SettingValue::Number(v));
    Behavior {
        dwell_ms: Some(r.dwell_ms.map_or(SettingValue::Off, SettingValue::Number)),
        dwell_grace_ms: n(r.dwell_grace_ms),
        cooldown_ms: n(r.cooldown_ms),
        hold: Some(r.hold),
        repeat_delay_ms: n(r.repeat_delay_ms),
        repeat_interval_ms: n(r.repeat_interval_ms),
        confirm: Some(r.confirm),
        confirm_window_ms: n(r.confirm_window_ms),
        enter_delay_ms: n(r.enter_delay_ms),
        active_flash_ms: n(r.active_flash_ms),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{parse_board, parse_theme};

    const THEME: &str = r##"schema: uca/1
vars: { accent: "#ffcc00", fg: $accent }
groups:
  others: { style: { background: "#111111" } }
  letters:
    style: { background: "#1f2937", font: { size: 28, color: $fg } }
    behavior: { dwell_ms: 900 }
  modifiers:
    style: { background: "#334155", border: { color: "#ff0000" } }
templates:
  key: { border: { width: 3, radius: 6 }, padding: 4, font: { weight: 400 } }
"##;

    const BOARD: &str = r##"schema: uca/1
board:
  id: main
  theme: default
  opacity: 0.8
  font: { family: Board, size: 20 }
  behavior: { cooldown_ms: 400 }
groups:
  modifiers: { style: { border: { width: 5 } } }
templates:
  key: { padding: 6 }
  wide: { extends: key, font: { align: left } }
layers:
  - id: l
    layout: { type: row }
    opacity: 0.5
    behavior: { dwell_grace_ms: 250 }
    items:
      - id: shift
        extends: wide
        group: [letters, modifiers]
        font: { color: "#00ff00" }
        states:
          hover: { background: "#444444" }
          active: { background: "#555555", opacity: 0.5 }
          disabled: { background: "#666666" }
        actions:
          default: { start: { key.down: shift } }
          build: { behavior: { repeat_delay_ms: 300 }, start: { key.tap: f1 } }
      - id: plain
"##;

    fn fixture() -> (BoardFile, ThemeFile) {
        (
            parse_board(BOARD, Path::new("b.yaml")).expect("board"),
            parse_theme(THEME, Path::new("t.yaml")).expect("theme"),
        )
    }

    /// 02: an item in groups [letters, modifiers] with template + state overrides
    /// resolves to an exact expected style.
    #[test]
    fn cascade_resolves_exactly() {
        let (board, theme) = fixture();
        let cascade = Cascade::new(&board, Some(&theme));
        let layer = &board.layers[0];
        let item = cascade.item(layer, &layer.items[0]);

        let expected = ResolvedStyle {
            // board 0.8 × layer 0.5 × item 1
            opacity: 0.4,
            // modifiers (listed last) beats letters; theme group, board group merged over it
            background: [0x33, 0x41, 0x55, 255],
            border: ResolvedBorder {
                // board `modifiers` group (5) beats the template (3)
                width: Length::Px(5.0),
                color: [255, 0, 0, 255],
                radius: [Length::Px(6.0); 4],
                style: BorderStyle::Solid,
            },
            // board template `key` (6) merged over the theme's (4)
            padding: [Length::Px(6.0); 4],
            font: ResolvedFont {
                // board font (level 5) beats the group's size 28
                family: "Board".into(),
                size: Length::Px(20.0),
                weight: 400,
                // the item's own colour (level 7)
                color: [0, 255, 0, 255],
                align: Align::Left,
            },
            shadow: None,
        };
        assert_eq!(item.style(&[]), expected);

        let hover_and_active = item.style(&[State::Hover, State::Active, State::Disabled]);
        assert_eq!(
            hover_and_active.background,
            [0x55, 0x55, 0x55, 255],
            "active wins"
        );
        assert_eq!(hover_and_active.opacity, 0.2);
        let hover = item.style(&[State::Hover, State::Disabled]);
        assert_eq!(
            hover.background,
            [0x44, 0x44, 0x44, 255],
            "hover beats disabled"
        );
    }

    #[test]
    fn behavior_cascade() {
        let (board, theme) = fixture();
        let cascade = Cascade::new(&board, Some(&theme));
        let layer = &board.layers[0];
        let item = cascade.item(layer, &layer.items[0]);
        assert_eq!(item.behavior.dwell_ms, Some(900.0), "letters group");
        assert_eq!(item.behavior.cooldown_ms, 400.0, "board");
        assert_eq!(item.behavior.dwell_grace_ms, 250.0, "layer");
        assert_eq!(item.behavior.repeat_delay_ms, 500.0, "registry default");
        assert_eq!(item.behavior.hold, Hold::Release);
        let actions = item.item.actions.as_ref().expect("actions");
        let build = &actions.0[1].1;
        let b = item.behavior_for(build);
        assert_eq!(b.repeat_delay_ms, 300.0, "set entry over item");
        assert_eq!(b.dwell_ms, Some(900.0));
    }

    #[test]
    fn plain_item_uses_others_group_and_variables() {
        let (board, theme) = fixture();
        let cascade = Cascade::new(&board, Some(&theme));
        let layer = &board.layers[0];
        let item = cascade.item(layer, &layer.items[1]);
        let s = item.style(&[]);
        assert_eq!(s.background, [0x11, 0x11, 0x11, 255], "others group");
        assert_eq!(s.border.width, Length::Px(2.0), "built-in");
        assert_eq!(s.font.color, [255, 255, 255, 255], "built-in");
        assert_eq!(
            cascade.vars().get("fg"),
            Some(&[0xff, 0xcc, 0x00, 255]),
            "var chain"
        );
    }

    #[test]
    fn no_theme_uses_builtins() {
        let board = parse_board(
            "schema: uca/1\nboard: { id: b }\nlayers: [ { id: l, layout: { type: row }, items: [ { id: x, dwell_ms: 0 } ] } ]\n",
            Path::new("b.yaml"),
        );
        assert!(board.is_err(), "dwell_ms belongs under behavior");
        let board = parse_board(
            "schema: uca/1\nboard: { id: b }\nlayers: [ { id: l, layout: { type: row }, items: [ { id: x, behavior: { dwell_ms: 0 }, shadow: { x: 2 } } ] } ]\n",
            Path::new("b.yaml"),
        )
        .expect("board");
        let cascade = Cascade::new(&board, None);
        let item = cascade.item(&board.layers[0], &board.layers[0].items[0]);
        let s = item.style(&[]);
        assert_eq!(s.background, [0, 0, 0, 255]);
        assert_eq!(s.font.size, Length::Px(24.0));
        assert_eq!(s.font.weight, 700);
        assert_eq!(item.behavior.dwell_ms, None, "dwell off");
        assert_eq!(
            s.shadow,
            Some(ResolvedShadow {
                x: Length::Px(2.0),
                y: Length::Px(0.0),
                blur: Length::Px(0.0),
                color: [0, 0, 0, 255]
            })
        );
    }

    #[test]
    fn dwell_indicator_and_flash_cascade() {
        let theme = parse_theme(
            "schema: uca/1
vars: { hi: \"#00ff00\" }
dwell_indicator: { color: $hi }
",
            Path::new("t.yaml"),
        )
        .expect("theme");
        let board = parse_board(
            "schema: uca/1
board: { id: b, theme: t, dwell_indicator: { width: 10 }, behavior: { active_flash_ms: 400 } }
layers: [ { id: l, layout: { type: row }, items: [ { id: x, dwell_indicator: { kind: bar } }, { id: y } ] } ]
",
            Path::new("b.yaml"),
        )
        .expect("board");
        let cascade = Cascade::new(&board, Some(&theme));
        let layer = &board.layers[0];
        let x = cascade.item(layer, &layer.items[0]);
        assert_eq!(
            x.dwell_indicator,
            ResolvedDwellIndicator {
                kind: DwellIndicatorKind::Bar,
                color: [0, 255, 0, 255],
                width: Length::Px(10.0),
            }
        );
        assert_eq!(x.behavior.active_flash_ms, 400.0);
        let plain = Cascade::new(&board, None).item(layer, &layer.items[1]);
        assert_eq!(plain.dwell_indicator.kind, DwellIndicatorKind::Ring);
        assert_eq!(
            plain.dwell_indicator.color,
            [0xff, 0xcc, 0x00, 255],
            "built-in"
        );
    }

    #[test]
    fn template_cycles_do_not_hang() {
        let board = parse_board(
            "schema: uca/1\nboard: { id: b }\ntemplates: { a: { extends: b }, b: { extends: a } }\nlayers: [ { id: l, layout: { type: row }, items: [ { id: x, extends: a } ] } ]\n",
            Path::new("b.yaml"),
        )
        .expect("board");
        let cascade = Cascade::new(&board, None);
        let _ = cascade.item(&board.layers[0], &board.layers[0].items[0]);
    }
}
