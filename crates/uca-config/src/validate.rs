// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Semantic validation of a profile with its boards and themes
//! (02 "Validation rules", 04 "Validation", 09).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use uca_types::suggest::closest;
use uca_types::TriggerId;

use crate::action::{Action, ActionSeq, Repeat};
use crate::board::{Binding, BoardFile, Item, Layer, LayoutKind, RESERVED_SET_NAMES};
use crate::pattern::{Input, Modifier, Pattern, PatternError, Term};
use crate::profile::{Filter, FilterKind, Profile};
use crate::theme::ThemeFile;
use crate::values::{Border, Color, Font, Groups, Id, Length, Shadow, States, Style};

/// The built-in action set every profile has.
pub const DEFAULT_SET: &str = "default";
/// The built-in font family (02 "Fonts").
pub const DEFAULT_FONT: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    pub file: PathBuf,
    /// JSON path, e.g. `layers[0].items[3].action`.
    pub path: String,
    pub kind: ProblemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemKind {
    MissingEscape,
    InvalidPattern {
        pattern: String,
        error: PatternError,
    },
    EscapeClash {
        pattern: String,
    },
    UnknownSwitch {
        name: String,
    },
    ActionAndActions,
    DoAndStages,
    UnknownActionSet {
        id: String,
        suggestion: Option<String>,
    },
    ReservedSetName {
        id: String,
    },
    HoldOutsideStart,
    OffDefault,
    DuplicateId {
        id: String,
    },
    UnknownTemplate {
        name: String,
        suggestion: Option<String>,
    },
    TemplateCycle {
        chain: Vec<String>,
    },
    UnknownMacro {
        id: String,
        suggestion: Option<String>,
    },
    MacroCycle {
        chain: Vec<String>,
    },
    UnknownColorVar {
        name: String,
    },
    UnknownBoard {
        id: String,
        suggestion: Option<String>,
    },
    UnknownTheme {
        name: String,
    },
    UnknownLayer {
        id: String,
        suggestion: Option<String>,
    },
    PointerSourceNotEnabled {
        source: String,
    },
    FilterParamWrongKind {
        param: &'static str,
        kind: &'static str,
    },
    GridOnly {
        key: &'static str,
    },
    ZoneOutsideFree,
    OutsideGrid {
        key: &'static str,
    },
    CellOverlap {
        other: String,
    },
    UnusedActionSet {
        id: String,
    },
    RepeatNeverRuns,
    SetWithoutExit {
        id: String,
    },
    NoWayOut {
        board: String,
    },
    UnknownGroup {
        name: String,
    },
    UnknownFont {
        family: String,
    },
    FreeItemWithoutSize,
    PositionIgnored,
    ReserveWithoutEdge,
    AreaIgnored,
    GridUnitOutsideGrid,
    GridOverflow {
        layer: String,
    },
    SingleAlsoFires {
        pattern: String,
    },
    PredictionWithAction,
    DuplicatePredictionSlot {
        slot: u32,
    },
    PredictionWeightsZero,
}

impl ProblemKind {
    /// Fluent message id in `locales/en-US/config.ftl`.
    pub fn message_id(&self) -> &'static str {
        use ProblemKind::*;
        match self {
            MissingEscape => "config-missing-escape",
            InvalidPattern { .. } => "config-invalid-pattern",
            EscapeClash { .. } => "config-escape-clash",
            UnknownSwitch { .. } => "config-unknown-switch",
            ActionAndActions => "config-action-and-actions",
            DoAndStages => "config-do-and-stages",
            UnknownActionSet { .. } => "config-unknown-action-set",
            ReservedSetName { .. } => "config-reserved-set-name",
            HoldOutsideStart => "config-hold-outside-start",
            OffDefault => "config-off-default",
            DuplicateId { .. } => "config-duplicate-id",
            UnknownTemplate { .. } => "config-unknown-template",
            TemplateCycle { .. } => "config-template-cycle",
            UnknownMacro { .. } => "config-unknown-macro",
            MacroCycle { .. } => "config-macro-cycle",
            UnknownColorVar { .. } => "config-unknown-color-var",
            UnknownBoard { .. } => "config-unknown-board",
            UnknownTheme { .. } => "config-unknown-theme",
            UnknownLayer { .. } => "config-unknown-layer",
            PointerSourceNotEnabled { .. } => "config-pointer-source-not-enabled",
            FilterParamWrongKind { .. } => "config-filter-param-wrong-kind",
            GridOnly { .. } => "config-grid-only",
            ZoneOutsideFree => "config-zone-outside-free",
            OutsideGrid { .. } => "config-outside-grid",
            CellOverlap { .. } => "config-cell-overlap",
            UnusedActionSet { .. } => "config-unused-action-set",
            RepeatNeverRuns => "config-repeat-never-runs",
            SetWithoutExit { .. } => "config-set-without-exit",
            NoWayOut { .. } => "config-no-way-out",
            UnknownGroup { .. } => "config-unknown-group",
            UnknownFont { .. } => "config-unknown-font",
            FreeItemWithoutSize => "config-free-item-without-size",
            PositionIgnored => "config-position-ignored",
            ReserveWithoutEdge => "config-reserve-without-edge",
            AreaIgnored => "config-area-ignored",
            GridUnitOutsideGrid => "config-grid-unit-outside-grid",
            GridOverflow { .. } => "config-grid-overflow",
            SingleAlsoFires { .. } => "config-single-also-fires",
            PredictionWithAction => "config-prediction-with-action",
            DuplicatePredictionSlot { .. } => "config-duplicate-prediction-slot",
            PredictionWeightsZero => "config-prediction-weights-zero",
        }
    }

    /// Fluent arguments for [`Self::message_id`].
    pub fn args(&self) -> Vec<(&'static str, String)> {
        use ProblemKind::*;
        // Fluent selects on "none" when there is no suggestion.
        let hint = |s: &Option<String>| ("suggestion", s.clone().unwrap_or_else(|| "none".into()));
        match self {
            MissingEscape
            | ActionAndActions
            | DoAndStages
            | HoldOutsideStart
            | OffDefault
            | ZoneOutsideFree
            | RepeatNeverRuns
            | PredictionWithAction
            | PredictionWeightsZero => {
                vec![]
            }
            DuplicatePredictionSlot { slot } => vec![("slot", slot.to_string())],
            InvalidPattern { pattern, error } => {
                let mut args = vec![("pattern", pattern.clone()), ("error", error.to_string())];
                args.extend(error.args());
                args
            }
            EscapeClash { pattern } => vec![("pattern", pattern.clone())],
            UnknownSwitch { name } => vec![("name", name.clone())],
            UnknownActionSet { id, suggestion }
            | UnknownMacro { id, suggestion }
            | UnknownBoard { id, suggestion }
            | UnknownLayer { id, suggestion } => vec![("id", id.clone()), hint(suggestion)],
            UnknownTemplate { name, suggestion } => vec![("name", name.clone()), hint(suggestion)],
            ReservedSetName { id }
            | DuplicateId { id }
            | UnusedActionSet { id }
            | SetWithoutExit { id } => vec![("id", id.clone())],
            TemplateCycle { chain } | MacroCycle { chain } => vec![("chain", chain.join(" → "))],
            UnknownColorVar { name } | UnknownTheme { name } | UnknownGroup { name } => {
                vec![("name", name.clone())]
            }
            PointerSourceNotEnabled { source } => vec![("source", source.clone())],
            FilterParamWrongKind { param, kind } => {
                vec![("param", (*param).to_owned()), ("kind", (*kind).to_owned())]
            }
            GridOnly { key } | OutsideGrid { key } => vec![("key", (*key).to_owned())],
            CellOverlap { other } => vec![("other", other.clone())],
            NoWayOut { board } => vec![("board", board.clone())],
            UnknownFont { family } => vec![("family", family.clone())],
            FreeItemWithoutSize | PositionIgnored | ReserveWithoutEdge | AreaIgnored
            | GridUnitOutsideGrid => {
                vec![]
            }
            GridOverflow { layer } => vec![("layer", layer.clone())],
            SingleAlsoFires { pattern } => vec![("pattern", pattern.clone())],
        }
    }
}

fn did_you_mean(s: &Option<String>) -> String {
    s.as_ref()
        .map(|s| format!(". Did you mean {s:?}?"))
        .unwrap_or_default()
}

impl fmt::Display for ProblemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ProblemKind::*;
        match self {
            MissingEscape => write!(
                f,
                "the profile has no safety escape; add `safety: {{ escape: \"<pattern>\" }}` (SAFETY-01)"
            ),
            InvalidPattern { pattern, error } => write!(f, "invalid pattern {pattern:?}: {error}"),
            EscapeClash { pattern } => write!(
                f,
                "binding {pattern:?} has the same meaning as the safety escape; the escape cannot be overridden (SAFETY-01)"
            ),
            UnknownSwitch { name } => write!(
                f,
                "unknown switch {name:?}; add it under profile.inputs.switches"
            ),
            ActionAndActions => write!(f, "use either `action` or `actions`, not both"),
            DoAndStages => write!(f, "use either `do` or stages (start, repeat, stop), not both"),
            UnknownActionSet { id, suggestion } => write!(
                f,
                "unknown action set {id:?}{}",
                did_you_mean(suggestion)
            ),
            ReservedSetName { id } => write!(
                f,
                "{id:?} cannot be an action set id; it is reserved for the stage shorthand"
            ),
            HoldOutsideStart => write!(f, "action_set.hold can only be used in a start stage"),
            OffDefault => write!(f, "the default action set cannot be turned off"),
            DuplicateId { id } => write!(f, "duplicate id {id:?}"),
            UnknownTemplate { name, suggestion } => write!(
                f,
                "unknown template {name:?}{}",
                did_you_mean(suggestion)
            ),
            TemplateCycle { chain } => write!(f, "template cycle: {}", chain.join(" → ")),
            UnknownMacro { id, suggestion } => {
                write!(f, "unknown macro {id:?}{}", did_you_mean(suggestion))
            }
            MacroCycle { chain } => write!(f, "macro cycle: {}", chain.join(" → ")),
            UnknownColorVar { name } => write!(
                f,
                "unknown color variable ${name}; define it in the theme's vars"
            ),
            UnknownBoard { id, suggestion } => {
                write!(f, "unknown board {id:?}{}", did_you_mean(suggestion))
            }
            UnknownTheme { name } => write!(f, "unknown theme {name:?}"),
            UnknownLayer { id, suggestion } => write!(
                f,
                "no board has a layer {id:?}{}",
                did_you_mean(suggestion)
            ),
            PointerSourceNotEnabled { source } => write!(
                f,
                "pointer source {source:?} must be listed under sources with enabled: true"
            ),
            FilterParamWrongKind { param, kind } => {
                write!(f, "{param} does not apply to the {kind} filter")
            }
            GridOnly { key } => write!(f, "{key} can only be used in a grid layer"),
            ZoneOutsideFree => write!(f, "zone items can only be used in a free layer"),
            OutsideGrid { key } => write!(f, "{key} lies outside the grid"),
            CellOverlap { other } => write!(f, "this pinned item overlaps {other:?}"),
            UnusedActionSet { id } => write!(f, "action set {id:?} is declared but never used"),
            RepeatNeverRuns => write!(
                f,
                "this repeat never runs, because the binding's pattern is always instant"
            ),
            SetWithoutExit { id } => write!(
                f,
                "action set {id:?} can be turned on, but while it is on top nothing turns it off (SAFETY-14)"
            ),
            NoWayOut { board } => write!(
                f,
                "board {board:?} has no item or binding that leads to another board (SAFETY-09)"
            ),
            UnknownGroup { name } => {
                write!(f, "unknown group {name:?}; the item is treated as `others`")
            }
            FreeItemWithoutSize => write!(f, "items in a free layer need a size with width and height"),
            PositionIgnored => write!(f, "position is only used with anchor: free"),
            ReserveWithoutEdge => write!(
                f,
                "reserve_space needs an edge anchor (top, bottom, left or right); nothing is reserved"
            ),
            AreaIgnored => write!(
                f,
                "area is only used by boards that reserve space; the full bounds are used"
            ),
            GridUnitOutsideGrid => write!(f, "the grid unit can only be used inside a grid layer"),
            SingleAlsoFires { pattern } => write!(
                f,
                "{pattern:?} is also bound on its own, which fires on the first press of every double"
            ),
            GridOverflow { layer } => write!(f, "some items in layer {layer:?} do not fit its rows and are not shown"),
            UnknownFont { family } => write!(
                f,
                "unknown font family {family:?}; the default font is used"
            ),
            PredictionWithAction => write!(
                f,
                "a prediction slot inserts its word, so it can't also have `action` or `actions`"
            ),
            DuplicatePredictionSlot { slot } => write!(
                f,
                "prediction slot {slot} is used more than once on this board"
            ),
            PredictionWeightsZero => write!(
                f,
                "the three prediction weights can't all be 0; set at least one above 0"
            ),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(
            f,
            "{} {}: {level}: {}",
            self.file.display(),
            self.path,
            self.kind
        )
    }
}

/// A profile with the boards and themes it uses.
pub struct ConfigSet<'a> {
    pub profile_file: &'a Path,
    pub profile: &'a Profile,
    pub boards: Vec<(&'a Path, &'a BoardFile)>,
    /// Keyed by theme name (the file name without `.yaml`).
    pub themes: BTreeMap<String, (&'a Path, &'a ThemeFile)>,
}

/// Validates the whole set; problems come in file order, errors and warnings mixed.
pub fn validate(set: &ConfigSet) -> Vec<Problem> {
    let mut v = Validator::new(set);
    v.run();
    v.problems
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Start,
    Repeat,
    Stop,
    /// A macro body: it runs inside whichever stage calls it.
    Macro,
}

struct Validator<'a> {
    set: &'a ConfigSet<'a>,
    problems: Vec<Problem>,
    file: PathBuf,
    sets: BTreeSet<String>,
    used_sets: BTreeSet<String>,
    boards: BTreeSet<String>,
    layers: BTreeSet<String>,
    escape: Vec<Term>,
    /// Every binding's normalised terms: (file, path, original pattern, terms).
    binding_terms: Vec<(PathBuf, String, String, Vec<Term>)>,
}

impl<'a> Validator<'a> {
    fn new(set: &'a ConfigSet<'a>) -> Self {
        let sets = std::iter::once(DEFAULT_SET.to_owned())
            .chain(set.profile.action_sets.keys().map(|k| k.0.clone()))
            .collect();
        let boards = set
            .boards
            .iter()
            .map(|(_, b)| b.board.id.0.clone())
            .collect();
        let layers = set
            .boards
            .iter()
            .flat_map(|(_, b)| b.layers.iter().map(|l| l.id.0.clone()))
            .collect();
        Self {
            set,
            problems: Vec::new(),
            file: set.profile_file.to_owned(),
            sets,
            used_sets: BTreeSet::new(),
            boards,
            layers,
            escape: Vec::new(),
            binding_terms: Vec::new(),
        }
    }

    fn report(&mut self, severity: Severity, path: impl Into<String>, kind: ProblemKind) {
        self.problems.push(Problem {
            severity,
            file: self.file.clone(),
            path: path.into(),
            kind,
        });
    }

    fn error(&mut self, path: impl Into<String>, kind: ProblemKind) {
        self.report(Severity::Error, path, kind);
    }

    fn warn(&mut self, path: impl Into<String>, kind: ProblemKind) {
        self.report(Severity::Warning, path, kind);
    }

    fn run(&mut self) {
        self.profile();
        let boards = self.set.boards.clone();
        let mut seen = BTreeSet::new();
        for (file, board) in boards {
            self.file = file.to_owned();
            if !seen.insert(board.board.id.0.clone()) {
                self.error(
                    "board.id",
                    ProblemKind::DuplicateId {
                        id: board.board.id.0.clone(),
                    },
                );
            }
            self.board(board);
        }
        self.file = self.set.profile_file.to_owned();
        self.action_set_usage();
        self.single_and_double();
    }

    // ---------------------------------------------------------------- profile

    fn profile(&mut self) {
        let p = self.set.profile;
        for id in p.action_sets.keys() {
            if RESERVED_SET_NAMES.contains(&id.0.as_str()) || id.0 == DEFAULT_SET {
                self.error(
                    format!("profile.action_sets.{id}"),
                    ProblemKind::ReservedSetName { id: id.0.clone() },
                );
            }
        }
        match &p.safety {
            None => self.error("profile.safety.escape", ProblemKind::MissingEscape),
            Some(safety) => {
                if let Some(pattern) = self.pattern(&safety.escape, "profile.safety.escape") {
                    self.escape = normalize(&pattern, p);
                }
            }
        }
        if !self.boards.contains(&p.start_board.0) {
            let kind = self.unknown_board(&p.start_board.0);
            self.error("profile.start_board", kind);
        }
        self.inputs(p);
        if p.prediction.weights.resolved().iter().all(|&w| w == 0.0) {
            self.error(
                "profile.prediction.weights",
                ProblemKind::PredictionWeightsZero,
            );
        }
        let macros: Vec<_> = p.macros.iter().collect();
        for (id, m) in macros {
            self.sequence(&m.do_, Stage::Macro, &format!("profile.macros.{id}.do"));
            self.macro_cycles(id);
        }
        for (i, b) in p.bindings.iter().enumerate() {
            self.binding(b, &format!("profile.bindings[{i}]"));
        }
    }

    fn inputs(&mut self, p: &Profile) {
        if let Some(src) = &p.inputs.pointer_source {
            if !p.inputs.sources.get(src).is_some_and(|s| s.enabled) {
                self.error(
                    "profile.inputs.pointer_source",
                    ProblemKind::PointerSourceNotEnabled {
                        source: src.clone(),
                    },
                );
            }
        }
        for (id, source) in &p.inputs.sources {
            if let Some(filter) = &source.filter {
                self.filter(filter, &format!("profile.inputs.sources.{id}.filter"));
            }
        }
        for key in p.inputs.trigger_settings.keys() {
            let path = format!("profile.inputs.trigger_settings.{key}");
            if key.contains(':') {
                if let Err(e) = TriggerId::parse(key) {
                    let error = PatternError {
                        column: 1,
                        kind: crate::pattern::PatternErrorKind::InvalidTrigger {
                            text: key.clone(),
                            reason: e.to_string(),
                        },
                    };
                    self.error(
                        path,
                        ProblemKind::InvalidPattern {
                            pattern: key.clone(),
                            error,
                        },
                    );
                }
            } else if !p.inputs.switches.contains_key(&Id(key.clone())) {
                self.error(path, ProblemKind::UnknownSwitch { name: key.clone() });
            }
        }
    }

    fn filter(&mut self, f: &Filter, path: &str) {
        let (kind, allowed): (&'static str, &[&str]) = match f.kind {
            FilterKind::None => ("none", &[]),
            FilterKind::Ema => ("ema", &["alpha"]),
            FilterKind::OneEuro => ("one-euro", &["min_cutoff", "beta", "d_cutoff"]),
        };
        let params = [
            ("min_cutoff", f.min_cutoff.is_some()),
            ("beta", f.beta.is_some()),
            ("d_cutoff", f.d_cutoff.is_some()),
            ("alpha", f.alpha.is_some()),
        ];
        for (param, present) in params {
            if present && !allowed.contains(&param) {
                self.error(
                    format!("{path}.{param}"),
                    ProblemKind::FilterParamWrongKind { param, kind },
                );
            }
        }
    }

    fn macro_cycles(&mut self, start: &Id) {
        let macros = &self.set.profile.macros;
        let mut stack = vec![(start.0.clone(), vec![start.0.clone()])];
        let mut seen = BTreeSet::new();
        while let Some((id, chain)) = stack.pop() {
            let Some(m) = macros.get(&Id(id.clone())) else {
                continue;
            };
            for a in &m.do_.0 {
                if let Action::MacroRun(next) = a {
                    let mut chain = chain.clone();
                    chain.push(next.0.clone());
                    if next.0 == start.0 {
                        self.error(
                            format!("profile.macros.{start}.do"),
                            ProblemKind::MacroCycle { chain },
                        );
                        return;
                    }
                    if seen.insert(next.0.clone()) {
                        stack.push((next.0.clone(), chain));
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- patterns and bindings

    fn pattern(&mut self, text: &str, path: &str) -> Option<Pattern> {
        match Pattern::parse(text) {
            Ok(p) => {
                for input in p.inputs() {
                    if let Input::Alias(name) = input {
                        if !self
                            .set
                            .profile
                            .inputs
                            .switches
                            .contains_key(&Id(name.clone()))
                        {
                            self.error(path, ProblemKind::UnknownSwitch { name: name.clone() });
                        }
                    }
                }
                Some(p)
            }
            Err(error) => {
                self.error(
                    path,
                    ProblemKind::InvalidPattern {
                        pattern: text.to_owned(),
                        error,
                    },
                );
                None
            }
        }
    }

    fn binding(&mut self, b: &Binding, path: &str) {
        let has_stages = b.start.is_some() || b.repeat.is_some() || b.stop.is_some();
        if b.do_.is_some() && has_stages {
            self.error(path, ProblemKind::DoAndStages);
        }
        let when_path = format!("{path}.when");
        if let Some(pattern) = self.pattern(&b.when, &when_path) {
            let norm = normalize(&pattern, self.set.profile);
            self.binding_terms.push((
                self.file.clone(),
                when_path.clone(),
                b.when.clone(),
                norm.clone(),
            ));
            if norm.iter().any(|t| self.escape.contains(t)) {
                self.error(
                    when_path,
                    ProblemKind::EscapeClash {
                        pattern: b.when.clone(),
                    },
                );
            }
            if b.repeat.is_some() && pattern.terms.iter().all(is_instant) {
                self.warn(format!("{path}.repeat"), ProblemKind::RepeatNeverRuns);
            }
        }
        if let Some(set) = b.context.as_ref().and_then(|c| c.action_set.as_ref()) {
            self.set_ref(set, &format!("{path}.in.action_set"));
        }
        if let Some(seq) = &b.do_ {
            self.sequence(seq, Stage::Start, &format!("{path}.do"));
        }
        self.stages(b.start.as_ref(), b.repeat.as_ref(), b.stop.as_ref(), path);
    }

    fn stages(
        &mut self,
        start: Option<&ActionSeq>,
        repeat: Option<&Repeat>,
        stop: Option<&ActionSeq>,
        path: &str,
    ) {
        if let Some(seq) = start {
            self.sequence(seq, Stage::Start, &format!("{path}.start"));
        }
        if let Some(Repeat::Seq(seq)) = repeat {
            self.sequence(seq, Stage::Repeat, &format!("{path}.repeat"));
        }
        if let Some(seq) = stop {
            self.sequence(seq, Stage::Stop, &format!("{path}.stop"));
        }
    }

    // ---------------------------------------------------------------- actions

    fn sequence(&mut self, seq: &ActionSeq, stage: Stage, path: &str) {
        for (i, a) in seq.0.iter().enumerate() {
            let path = if seq.0.len() == 1 {
                path.to_owned()
            } else {
                format!("{path}[{i}]")
            };
            self.action(a, stage, &path);
        }
    }

    fn action(&mut self, a: &Action, stage: Stage, path: &str) {
        match a {
            Action::ActionSetHold(id) => {
                if matches!(stage, Stage::Repeat | Stage::Stop) {
                    self.error(path, ProblemKind::HoldOutsideStart);
                }
                self.set_ref(id, path);
            }
            Action::ActionSetOff(id) => {
                if id.0 == DEFAULT_SET {
                    self.error(path, ProblemKind::OffDefault);
                }
                self.set_ref(id, path);
            }
            Action::ActionSetOn(id)
            | Action::ActionSetToggle(id)
            | Action::ActionSetSwitch(id)
            | Action::ActionSetOnce(id) => self.set_ref(id, path),
            Action::ActionSetCycle(ids) => {
                for id in ids {
                    self.set_ref(id, path);
                }
            }
            Action::BoardOpen(id) | Action::BoardReplace(id) => {
                if !self.boards.contains(&id.0) {
                    let kind = self.unknown_board(&id.0);
                    self.error(path, kind);
                }
            }
            Action::LayerShow(id) | Action::LayerHide(id) | Action::LayerToggle(id) => {
                if !self.layers.contains(&id.0) {
                    let suggestion =
                        closest(&id.0, self.layers.iter().map(String::as_str)).map(str::to_owned);
                    self.error(
                        path,
                        ProblemKind::UnknownLayer {
                            id: id.0.clone(),
                            suggestion,
                        },
                    );
                }
            }
            Action::MacroRun(id) if !self.set.profile.macros.contains_key(id) => {
                let suggestion =
                    closest(&id.0, self.set.profile.macros.keys().map(|k| k.0.as_str()))
                        .map(str::to_owned);
                self.error(
                    path,
                    ProblemKind::UnknownMacro {
                        id: id.0.clone(),
                        suggestion,
                    },
                );
            }
            _ => {}
        }
    }

    fn set_ref(&mut self, id: &Id, path: &str) {
        if self.sets.contains(&id.0) {
            self.used_sets.insert(id.0.clone());
        } else {
            let suggestion =
                closest(&id.0, self.sets.iter().map(String::as_str)).map(str::to_owned);
            self.error(
                path,
                ProblemKind::UnknownActionSet {
                    id: id.0.clone(),
                    suggestion,
                },
            );
        }
    }

    fn unknown_board(&self, id: &str) -> ProblemKind {
        ProblemKind::UnknownBoard {
            id: id.to_owned(),
            suggestion: closest(id, self.boards.iter().map(String::as_str)).map(str::to_owned),
        }
    }

    // ---------------------------------------------------------------- boards

    fn board(&mut self, file: &'a BoardFile) {
        let theme = match &file.board.theme {
            Some(name) => match self.set.themes.get(name) {
                Some((_, t)) => Some(*t),
                None => {
                    self.error(
                        "board.theme",
                        ProblemKind::UnknownTheme { name: name.clone() },
                    );
                    None
                }
            },
            None => None,
        };
        let scope = StyleScope { file, theme };

        let b = &file.board;
        if b.position.is_some() && b.anchor != Some(crate::board::Anchor::Free) {
            self.warn("board.position", ProblemKind::PositionIgnored);
        }
        if b.reserve_space == Some(true) && !b.anchor.is_some_and(|a| a.is_edge()) {
            self.warn("board.reserve_space", ProblemKind::ReserveWithoutEdge);
        }
        if b.area.is_some() && !b.reserves_space() {
            self.warn("board.area", ProblemKind::AreaIgnored);
        }
        if lengths_of(BaseLengths::from(b), &b.states).any(|l| matches!(l, Length::Grid(_))) {
            self.error("board", ProblemKind::GridUnitOutsideGrid);
        }
        self.base(
            &scope,
            "board",
            &b.extends,
            &b.group,
            BaseStyle::of(b),
            &b.states,
        );
        for (i, binding) in file.bindings.iter().enumerate() {
            self.binding(binding, &format!("bindings[{i}]"));
        }
        for (name, group) in &file.groups {
            let path = format!("groups.{name}");
            self.style(&scope, &path, &group.style);
        }
        for (name, t) in &file.templates {
            let path = format!("templates.{name}");
            self.item_style(&scope, t, &path);
            self.template_cycle(&scope, name, &path);
        }

        let mut layer_ids = BTreeSet::new();
        for (li, layer) in file.layers.iter().enumerate() {
            let path = format!("layers[{li}]");
            if !layer_ids.insert(layer.id.0.clone()) {
                self.error(
                    format!("{path}.id"),
                    ProblemKind::DuplicateId {
                        id: layer.id.0.clone(),
                    },
                );
            }
            self.layer(&scope, layer, &path);
        }
        let mut slots = BTreeSet::new();
        for (li, layer) in file.layers.iter().enumerate() {
            for (ii, item) in layer.items.iter().enumerate() {
                if let Some(slot) = item.prediction {
                    if !slots.insert(slot) {
                        self.warn(
                            format!("layers[{li}].items[{ii}].prediction"),
                            ProblemKind::DuplicatePredictionSlot { slot: slot.get() },
                        );
                    }
                }
            }
        }

        if !self.has_way_out(file) {
            self.warn(
                "board",
                ProblemKind::NoWayOut {
                    board: b.id.0.clone(),
                },
            );
        }
    }

    fn layer(&mut self, scope: &StyleScope<'a>, layer: &Layer, path: &str) {
        if lengths_of(BaseLengths::from(layer), &layer.states).any(|l| matches!(l, Length::Grid(_)))
        {
            self.error(path, ProblemKind::GridUnitOutsideGrid);
        }
        let mut grid_cells = 0u64;
        self.base(
            scope,
            path,
            &layer.extends,
            &layer.group,
            BaseStyle::of(layer),
            &layer.states,
        );
        for (i, b) in layer.bindings.iter().enumerate() {
            self.binding(b, &format!("{path}.bindings[{i}]"));
        }
        let kind = layer.layout.kind;
        let columns = Some(
            layer
                .layout
                .columns
                .unwrap_or_else(|| (layer.items.len() as f64).sqrt().ceil().max(1.0) as u32),
        );
        let rows = layer.layout.rows;
        let mut ids = BTreeSet::new();
        let mut pinned: Vec<(String, (u32, u32, u32, u32))> = Vec::new();
        for (ii, item) in layer.items.iter().enumerate() {
            let ipath = format!("{path}.items[{ii}]");
            let id = item
                .id
                .as_ref()
                .map_or_else(|| format!("item-{ii}"), |i| i.0.clone());
            if !ids.insert(id.clone()) {
                self.error(
                    format!("{ipath}.id"),
                    ProblemKind::DuplicateId { id: id.clone() },
                );
            }
            self.item(scope, item, &ipath);

            if kind != LayoutKind::Grid {
                if item.span.is_some() {
                    self.error(
                        format!("{ipath}.span"),
                        ProblemKind::GridOnly { key: "span" },
                    );
                }
                if item.cell.is_some() {
                    self.error(
                        format!("{ipath}.cell"),
                        ProblemKind::GridOnly { key: "cell" },
                    );
                }
            }
            if kind != LayoutKind::Free && item.zone == Some(true) {
                self.error(format!("{ipath}.zone"), ProblemKind::ZoneOutsideFree);
            }
            if kind == LayoutKind::Free
                && item.empty != Some(true)
                && !item
                    .size
                    .is_some_and(|s| s.width.is_some() && s.height.is_some())
            {
                self.error(format!("{ipath}.size"), ProblemKind::FreeItemWithoutSize);
            }
            if kind != LayoutKind::Grid
                && lengths_of(BaseLengths::from(item), &item.states)
                    .any(|l| matches!(l, Length::Grid(_)))
            {
                self.error(ipath.clone(), ProblemKind::GridUnitOutsideGrid);
            }
            if kind == LayoutKind::Grid {
                grid_cells += item
                    .span
                    .map_or(1, |s| u64::from(s.cols.max(1)) * u64::from(s.rows.max(1)));
                let span = item.span.map_or((1, 1), |s| (s.cols.max(1), s.rows.max(1)));
                if columns.is_some_and(|c| span.0 > c) || rows.is_some_and(|r| span.1 > r) {
                    self.error(
                        format!("{ipath}.span"),
                        ProblemKind::OutsideGrid { key: "span" },
                    );
                }
                if let Some(cell) = item.cell {
                    let outside = columns.is_some_and(|c| cell.col + span.0 > c)
                        || rows.is_some_and(|r| cell.row + span.1 > r);
                    if outside {
                        self.error(
                            format!("{ipath}.cell"),
                            ProblemKind::OutsideGrid { key: "cell" },
                        );
                    }
                    let rect = (cell.col, cell.row, cell.col + span.0, cell.row + span.1);
                    let overlap = pinned.iter().find(|(_, r)| {
                        rect.0 < r.2 && r.0 < rect.2 && rect.1 < r.3 && r.1 < rect.3
                    });
                    if let Some((other, _)) = overlap {
                        let other = other.clone();
                        self.error(format!("{ipath}.cell"), ProblemKind::CellOverlap { other });
                    }
                    pinned.push((id, rect));
                }
            }
        }
        if let (LayoutKind::Grid, Some(rows)) = (layer.layout.kind, layer.layout.rows) {
            let n = layer.items.len() as f64;
            let columns = layer
                .layout
                .columns
                .map_or_else(|| n.sqrt().ceil().max(1.0) as u64, u64::from);
            if grid_cells > columns * u64::from(rows) {
                self.warn(
                    path,
                    ProblemKind::GridOverflow {
                        layer: layer.id.0.clone(),
                    },
                );
            }
        }
    }

    fn item(&mut self, scope: &StyleScope<'a>, item: &Item, path: &str) {
        self.item_style(scope, item, path);
        if item.action.is_some() && item.actions.is_some() {
            self.error(path, ProblemKind::ActionAndActions);
        }
        if item.prediction.is_some() && (item.action.is_some() || item.actions.is_some()) {
            self.error(
                format!("{path}.prediction"),
                ProblemKind::PredictionWithAction,
            );
        }
        if let Some(seq) = &item.action {
            self.sequence(seq, Stage::Start, &format!("{path}.action"));
        }
        if let Some(actions) = &item.actions {
            for (set, entry) in &actions.0 {
                let epath = format!("{path}.actions.{set}");
                self.set_ref(set, &epath);
                self.stages(
                    entry.start.as_ref(),
                    entry.repeat.as_ref(),
                    entry.stop.as_ref(),
                    &epath,
                );
            }
        }
        for (i, b) in item.bindings.iter().enumerate() {
            self.binding(b, &format!("{path}.bindings[{i}]"));
        }
    }

    fn item_style(&mut self, scope: &StyleScope<'a>, item: &Item, path: &str) {
        self.base(
            scope,
            path,
            &item.extends,
            &item.group,
            BaseStyle::of(item),
            &item.states,
        );
    }

    // ---------------------------------------------------------------- styles

    fn base(
        &mut self,
        scope: &StyleScope<'a>,
        path: &str,
        extends: &Option<String>,
        groups: &Option<Groups>,
        style: BaseStyle<'_>,
        states: &States,
    ) {
        if let Some(name) = extends {
            if scope.template(name).is_none() {
                let suggestion = closest(name, scope.template_names()).map(str::to_owned);
                self.error(
                    format!("{path}.extends"),
                    ProblemKind::UnknownTemplate {
                        name: name.clone(),
                        suggestion,
                    },
                );
            }
        }
        if let Some(Groups(names)) = groups {
            for name in names {
                if !scope.has_group(name) {
                    self.warn(
                        format!("{path}.group"),
                        ProblemKind::UnknownGroup { name: name.clone() },
                    );
                }
            }
        }
        self.colors(scope, path, style.colors());
        self.fonts(scope, path, style.font);
        for (state, s) in states {
            let spath = format!("{path}.states.{}", state_name(*state));
            self.style(scope, &spath, s);
        }
    }

    fn style(&mut self, scope: &StyleScope<'a>, path: &str, s: &Style) {
        let base = BaseStyle {
            background: s.background.as_ref(),
            border: s.border.as_ref(),
            font: s.font.as_ref(),
            shadow: s.shadow.as_ref(),
        };
        self.colors(scope, path, base.colors());
        self.fonts(scope, path, base.font);
    }

    fn colors(&mut self, scope: &StyleScope<'a>, path: &str, colors: Vec<&Color>) {
        for c in colors {
            if let Color::Var(name) = c {
                if !scope.has_var(name) {
                    self.error(path, ProblemKind::UnknownColorVar { name: name.clone() });
                }
            }
        }
    }

    fn fonts(&mut self, scope: &StyleScope<'a>, path: &str, font: Option<&Font>) {
        if let Some(family) = font.and_then(|f| f.family.as_ref()) {
            if family != DEFAULT_FONT && !scope.has_font(family) {
                self.warn(
                    format!("{path}.font.family"),
                    ProblemKind::UnknownFont {
                        family: family.clone(),
                    },
                );
            }
        }
    }

    fn template_cycle(&mut self, scope: &StyleScope<'a>, start: &str, path: &str) {
        let mut chain = vec![start.to_owned()];
        let mut current = start.to_owned();
        while let Some(next) = scope.template(&current).and_then(|t| t.extends.clone()) {
            if next == start {
                chain.push(next);
                self.error(
                    format!("{path}.extends"),
                    ProblemKind::TemplateCycle { chain },
                );
                return;
            }
            if chain.contains(&next) {
                // A cycle that does not include `start`; reported from its own template.
                return;
            }
            chain.push(next.clone());
            current = next;
        }
    }

    // ---------------------------------------------------------------- safety warnings

    /// SAFETY-09: something on the board, or a global binding, leads to another board.
    fn has_way_out(&self, file: &BoardFile) -> bool {
        let profile = self.set.profile;
        let mut seqs: Vec<&ActionSeq> = Vec::new();
        collect_bindings(&file.bindings, &mut seqs);
        collect_bindings(&profile.bindings, &mut seqs);
        for layer in &file.layers {
            collect_bindings(&layer.bindings, &mut seqs);
            for item in &layer.items {
                collect_item(item, &mut seqs);
            }
        }
        seqs.iter().any(|seq| {
            flatten(seq, profile).iter().any(|a| {
                matches!(
                    a,
                    Action::BoardOpen(_) | Action::BoardBack | Action::BoardReplace(_)
                )
            })
        })
    }

    /// 04: when `x` and `x double(ms)` are both bound, `x` also fires on the first press.
    fn single_and_double(&mut self) {
        let singles: Vec<Input> = self
            .binding_terms
            .iter()
            .flat_map(|(_, _, _, terms)| terms.iter())
            .filter_map(|t| match t {
                Term::Input {
                    input,
                    modifier: Some(Modifier::Press),
                } => Some(input.clone()),
                _ => None,
            })
            .collect();
        let doubles: Vec<(PathBuf, String, String)> = self
            .binding_terms
            .iter()
            .filter(|(_, _, _, terms)| {
                terms.iter().any(|t| {
                    matches!(t, Term::Input { input, modifier: Some(Modifier::Double(_)) } if singles.contains(input))
                })
            })
            .map(|(f, p, w, _)| (f.clone(), p.clone(), w.clone()))
            .collect();
        for (file, path, pattern) in doubles {
            self.file = file;
            self.warn(path, ProblemKind::SingleAlsoFires { pattern });
        }
        self.file = self.set.profile_file.to_owned();
    }

    /// Unused sets, and SAFETY-14 for sets that can be latched on.
    fn action_set_usage(&mut self) {
        let profile = self.set.profile;
        let declared: Vec<String> = profile.action_sets.keys().map(|k| k.0.clone()).collect();
        let mut all: Vec<&ActionSeq> = Vec::new();
        collect_bindings(&profile.bindings, &mut all);
        for (_, file) in &self.set.boards {
            collect_bindings(&file.bindings, &mut all);
            for layer in &file.layers {
                collect_bindings(&layer.bindings, &mut all);
                for item in &layer.items {
                    collect_item(item, &mut all);
                }
            }
        }
        let actions: Vec<&Action> = all.iter().flat_map(|s| flatten(s, profile)).collect();
        for id in declared {
            let path = format!("profile.action_sets.{id}");
            if !self.used_sets.contains(&id) {
                self.warn(path, ProblemKind::UnusedActionSet { id });
                continue;
            }
            let latchable = actions.iter().any(|a| match a {
                Action::ActionSetOn(s)
                | Action::ActionSetToggle(s)
                | Action::ActionSetSwitch(s) => s.0 == id,
                Action::ActionSetCycle(list) => list.iter().any(|s| s.0 == id),
                _ => false,
            });
            if latchable && !self.has_exit(&id) {
                self.warn(path, ProblemKind::SetWithoutExit { id });
            }
        }
    }

    /// Whether something available while `set` is on top turns it off.
    fn has_exit(&self, set: &str) -> bool {
        let profile = self.set.profile;
        let mut seqs: Vec<&ActionSeq> = Vec::new();
        let applies = |b: &Binding| {
            b.context
                .as_ref()
                .and_then(|c| c.action_set.as_ref())
                .is_none_or(|s| s.0 == set)
        };
        let mut bindings: Vec<&Binding> = profile.bindings.iter().filter(|b| applies(b)).collect();
        for (_, file) in &self.set.boards {
            bindings.extend(file.bindings.iter().filter(|b| applies(b)));
            for layer in &file.layers {
                bindings.extend(layer.bindings.iter().filter(|b| applies(b)));
                for item in &layer.items {
                    bindings.extend(item.bindings.iter().filter(|b| applies(b)));
                    // While `set` is on top, an item uses its `set` entry, else its default.
                    match &item.actions {
                        Some(actions) => {
                            let entry =
                                actions.0.iter().find(|(id, _)| id.0 == set).or_else(|| {
                                    actions.0.iter().find(|(id, _)| id.0 == DEFAULT_SET)
                                });
                            if let Some((_, e)) = entry {
                                seqs.extend(e.start.iter());
                                if let Some(Repeat::Seq(r)) = &e.repeat {
                                    seqs.push(r);
                                }
                                seqs.extend(e.stop.iter());
                            }
                        }
                        None => seqs.extend(item.action.iter()),
                    }
                }
            }
        }
        for b in bindings {
            seqs.extend(b.do_.iter().chain(b.start.iter()).chain(b.stop.iter()));
            if let Some(Repeat::Seq(r)) = &b.repeat {
                seqs.push(r);
            }
        }
        seqs.iter().any(|seq| {
            flatten(seq, profile).iter().any(|a| match a {
                Action::ActionSetOff(s) | Action::ActionSetToggle(s) => s.0 == set,
                Action::ActionSetCycle(list) => list.iter().any(|s| s.0 == set),
                Action::ActionSetSwitch(_) | Action::ActionSetReset => true,
                _ => false,
            })
        })
    }
}

/// The `Length`-valued base properties of a board, layer or item.
struct BaseLengths<'s> {
    position: Option<&'s crate::values::Position>,
    size: Option<&'s crate::values::Size>,
    padding: Option<&'s crate::values::Four<Length>>,
    border: Option<&'s Border>,
    font: Option<&'s Font>,
    shadow: Option<&'s Shadow>,
}

macro_rules! base_lengths {
    ($($t:ty),*) => {$(
        impl<'s> From<&'s $t> for BaseLengths<'s> {
            fn from(x: &'s $t) -> Self {
                BaseLengths {
                    position: x.position.as_ref(),
                    size: x.size.as_ref(),
                    padding: x.padding.as_ref(),
                    border: x.border.as_ref(),
                    font: x.font.as_ref(),
                    shadow: x.shadow.as_ref(),
                }
            }
        }
    )*};
}

base_lengths!(crate::board::Board, Layer, Item);

fn four(f: Option<&crate::values::Four<Length>>) -> Vec<Length> {
    match f {
        None => vec![],
        Some(crate::values::Four::All(l)) => vec![*l],
        Some(crate::values::Four::Each(e)) => e.to_vec(),
    }
}

fn style_lengths(
    border: Option<&Border>,
    font: Option<&Font>,
    shadow: Option<&Shadow>,
    padding: Option<&crate::values::Four<Length>>,
) -> Vec<Length> {
    let mut out = four(padding);
    if let Some(b) = border {
        out.extend(b.width);
        out.extend(four(b.radius.as_ref()));
    }
    out.extend(font.and_then(|f| f.size));
    if let Some(s) = shadow {
        out.extend([s.x, s.y, s.blur].into_iter().flatten());
    }
    out
}

/// Every length of a board, layer or item, including its states.
fn lengths_of(b: BaseLengths<'_>, states: &States) -> impl Iterator<Item = Length> {
    let mut out = Vec::new();
    if let Some(p) = b.position {
        out.extend([p.x, p.y].into_iter().flatten());
    }
    if let Some(s) = b.size {
        out.extend([s.width, s.height].into_iter().flatten());
    }
    out.extend(style_lengths(b.border, b.font, b.shadow, b.padding));
    for s in states.values() {
        out.extend(style_lengths(
            s.border.as_ref(),
            s.font.as_ref(),
            s.shadow.as_ref(),
            s.padding.as_ref(),
        ));
    }
    out.into_iter()
}

/// Theme and board context for style references.
struct StyleScope<'a> {
    file: &'a BoardFile,
    theme: Option<&'a ThemeFile>,
}

impl<'a> StyleScope<'a> {
    /// A template, with the board's version merged over the theme's (02).
    fn template(&self, name: &str) -> Option<Item> {
        crate::cascade::Cascade::new(self.file, self.theme).template(name)
    }

    fn template_names(&self) -> impl Iterator<Item = &'a str> {
        let theme = self.theme.into_iter().flat_map(|t| t.templates.keys());
        self.file.templates.keys().chain(theme).map(String::as_str)
    }

    fn has_group(&self, name: &str) -> bool {
        self.file.groups.contains_key(name)
            || self.theme.is_some_and(|t| t.groups.contains_key(name))
    }

    fn has_var(&self, name: &str) -> bool {
        self.theme
            .is_some_and(|t| t.vars.contains_key(&Id(name.to_owned())))
    }

    fn has_font(&self, family: &str) -> bool {
        self.theme.is_some_and(|t| t.fonts.contains_key(family))
    }
}

/// The colour- and font-bearing base properties of a board, layer or item.
struct BaseStyle<'s> {
    background: Option<&'s Color>,
    border: Option<&'s Border>,
    font: Option<&'s Font>,
    shadow: Option<&'s Shadow>,
}

trait HasBase {
    fn base(&self) -> BaseStyle<'_>;
}

macro_rules! has_base {
    ($($t:ty),*) => {$(
        impl HasBase for $t {
            fn base(&self) -> BaseStyle<'_> {
                BaseStyle {
                    background: self.background.as_ref(),
                    border: self.border.as_ref(),
                    font: self.font.as_ref(),
                    shadow: self.shadow.as_ref(),
                }
            }
        }
    )*};
}

has_base!(crate::board::Board, Layer, Item);

impl<'s> BaseStyle<'s> {
    fn of(x: &'s impl HasBase) -> Self {
        x.base()
    }

    fn colors(&self) -> Vec<&'s Color> {
        let mut out: Vec<&'s Color> = Vec::new();
        out.extend(self.background);
        out.extend(self.border.and_then(|b| b.color.as_ref()));
        out.extend(self.font.and_then(|f| f.color.as_ref()));
        out.extend(self.shadow.and_then(|s| s.color.as_ref()));
        out
    }
}

fn state_name(s: crate::values::State) -> &'static str {
    use crate::values::State::*;
    match s {
        Hover => "hover",
        Dwell => "dwell",
        Active => "active",
        Disabled => "disabled",
        Latched => "latched",
        Forget => "forget",
    }
}

/// Every action sequence of a list of bindings.
fn collect_bindings<'b>(bindings: &'b [Binding], out: &mut Vec<&'b ActionSeq>) {
    for b in bindings {
        out.extend(b.do_.iter().chain(b.start.iter()).chain(b.stop.iter()));
        if let Some(Repeat::Seq(r)) = &b.repeat {
            out.push(r);
        }
    }
}

/// Every action sequence of an item, in all sets, including its bindings.
fn collect_item<'b>(item: &'b Item, out: &mut Vec<&'b ActionSeq>) {
    out.extend(item.action.iter());
    if let Some(actions) = &item.actions {
        for (_, e) in &actions.0 {
            out.extend(e.start.iter().chain(e.stop.iter()));
            if let Some(Repeat::Seq(r)) = &e.repeat {
                out.push(r);
            }
        }
    }
    collect_bindings(&item.bindings, out);
}

/// The actions of a sequence with `macro.run` expanded (each macro once).
fn flatten<'p>(seq: &'p ActionSeq, profile: &'p Profile) -> Vec<&'p Action> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&'p Action> = seq.0.iter().rev().collect();
    while let Some(a) = stack.pop() {
        if let Action::MacroRun(id) = a {
            if seen.insert(id.0.clone()) {
                if let Some(m) = profile.macros.get(id) {
                    stack.extend(m.do_.0.iter().rev());
                }
            }
        }
        out.push(a);
    }
    out
}

/// Terms with aliases resolved to triggers and `x` meaning `x press` (02 "Escape clash").
fn normalize(p: &Pattern, profile: &Profile) -> Vec<Term> {
    let resolve = |i: &Input| match i {
        Input::Alias(name) => profile
            .inputs
            .switches
            .get(&Id(name.clone()))
            .map_or_else(|| i.clone(), |t| Input::Trigger(t.clone())),
        other => other.clone(),
    };
    p.terms
        .iter()
        .map(|t| match t {
            Term::Input { input, modifier } => Term::Input {
                input: resolve(input),
                modifier: Some(modifier.unwrap_or(Modifier::Press)),
            },
            Term::Sequence { inputs, within_ms } => Term::Sequence {
                inputs: inputs.iter().map(resolve).collect(),
                within_ms: *within_ms,
            },
            Term::Chord(inputs) => Term::Chord(inputs.iter().map(resolve).collect()),
        })
        .collect()
}

/// Patterns with no ongoing hold give an instant activation (04 "Activations and stages").
fn is_instant(t: &Term) -> bool {
    match t {
        Term::Input { input, modifier } => {
            matches!(input, Input::Gesture(_))
                || matches!(modifier, Some(Modifier::Release | Modifier::Double(_)))
        }
        Term::Sequence { .. } => true,
        Term::Chord(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::Pattern;

    const EN_US_CONFIG: &str = include_str!("../../../locales/en-US/config.ftl");

    fn has_message(id: &str) -> bool {
        let prefix = format!("{id} =");
        EN_US_CONFIG.lines().any(|l| l.starts_with(&prefix))
    }

    #[test]
    fn every_problem_has_en_us_text() {
        use ProblemKind::*;
        let s = || String::new();
        let error = Pattern::parse("").expect_err("empty");
        let kinds = vec![
            MissingEscape,
            InvalidPattern {
                pattern: s(),
                error: error.clone(),
            },
            EscapeClash { pattern: s() },
            UnknownSwitch { name: s() },
            ActionAndActions,
            DoAndStages,
            UnknownActionSet {
                id: s(),
                suggestion: None,
            },
            ReservedSetName { id: s() },
            HoldOutsideStart,
            OffDefault,
            DuplicateId { id: s() },
            UnknownTemplate {
                name: s(),
                suggestion: None,
            },
            TemplateCycle { chain: vec![] },
            UnknownMacro {
                id: s(),
                suggestion: None,
            },
            MacroCycle { chain: vec![] },
            UnknownColorVar { name: s() },
            UnknownBoard {
                id: s(),
                suggestion: None,
            },
            UnknownTheme { name: s() },
            UnknownLayer {
                id: s(),
                suggestion: None,
            },
            PointerSourceNotEnabled { source: s() },
            FilterParamWrongKind {
                param: "",
                kind: "",
            },
            GridOnly { key: "" },
            ZoneOutsideFree,
            OutsideGrid { key: "" },
            CellOverlap { other: s() },
            UnusedActionSet { id: s() },
            RepeatNeverRuns,
            SetWithoutExit { id: s() },
            NoWayOut { board: s() },
            UnknownGroup { name: s() },
            UnknownFont { family: s() },
            FreeItemWithoutSize,
            PositionIgnored,
            ReserveWithoutEdge,
            AreaIgnored,
            GridUnitOutsideGrid,
            GridOverflow { layer: s() },
            SingleAlsoFires { pattern: s() },
        ];
        for kind in &kinds {
            assert!(
                has_message(kind.message_id()),
                "{} missing",
                kind.message_id()
            );
            for (name, _) in kind.args() {
                assert!(
                    EN_US_CONFIG.contains(&format!("${name}")) || name == "column",
                    "{}: argument {name} is never used",
                    kind.message_id()
                );
            }
        }
        for text in [
            "",
            "a hold(0)",
            "a tap",
            "A",
            "key:F99",
            "gesture:",
            "a hold(x)",
        ] {
            let e = Pattern::parse(text).expect_err(text);
            assert!(has_message(e.message_id()), "{} missing", e.message_id());
        }
    }
}
