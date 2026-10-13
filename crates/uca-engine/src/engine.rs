// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The interaction engine (04): hit-testing, dwell, activations with stages,
//! engine actions, bindings, the escape and the Safe Board (09).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uca_config::action::{Action, ActionSeq, Mode, Repeat};
use uca_config::board::{ActionSetEntry, Binding, BoardFile, Item};
use uca_config::cascade::{behavior_over_builtin, Cascade, ResolvedBehavior, ResolvedItem};
use uca_config::settings_registry::{self, SettingValue};
use uca_config::store::LoadedConfig;
use uca_config::values::{Hold, State};
use uca_mapping::{BindingKey, Ends, Mapping, MappingEvent, MatchContext};
use uca_types::{Clock, FilteredPoint, Key, MonitorInfo, SourceStatus, TriggerEvent, TriggerId};

use crate::layout::{choose_monitor, layout_board, BoardLayout};
use crate::safe_board::{safe_board, SAFE_ITEMS};
use crate::sets::{SetStack, DEFAULT_SET};
use crate::types::{
    ActivationId, BoardSnapshot, Control, DebugSnapshot, DwellSnapshot, EngineControl, EngineInput,
    EngineSnapshot, Intent, ItemSnapshot, Latch, LayerSnapshot, SafeItem, VirtualBoard,
    VirtualItemSnapshot, VirtualSnapshot,
};

/// An item on the shown board (layer index and item index), or with `virt`
/// an item of the virtual board (layer 0, index in its items).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ItemRef {
    virt: bool,
    layer: usize,
    item: usize,
}

impl ItemRef {
    fn board(layer: usize, item: usize) -> Self {
        Self {
            virt: false,
            layer,
            item,
        }
    }

    fn virtual_item(item: usize) -> Self {
        Self {
            virt: true,
            layer: 0,
            item,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Dwell {
    Idle,
    Dwelling {
        item: ItemRef,
        done: Duration,
        since: Instant,
    },
    Grace {
        item: ItemRef,
        done: Duration,
        left_at: Instant,
    },
    /// A `hold: release` activation started by dwell stays open while the point stays.
    Holding {
        item: ItemRef,
        activation: ActivationId,
        left_at: Option<Instant>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    Item(ItemRef),
    Binding(BindingKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageKind {
    Start,
    Repeat,
    Stop,
}

#[derive(Debug, Clone)]
struct Run {
    stage: StageKind,
    steps: Vec<Action>,
    pos: usize,
    next_at: Instant,
    started: Instant,
}

#[derive(Debug, Clone)]
struct Activation {
    id: ActivationId,
    owner: Owner,
    /// Item activations started by `select` inside a binding end with that binding (04).
    ended_by: Option<BindingKey>,
    start: Vec<Action>,
    repeat: Option<Vec<Action>>,
    stop: Vec<Action>,
    hold: Hold,
    behavior: ResolvedBehavior,
    run: Option<Run>,
    next_repeat: Option<Instant>,
    ending: bool,
}

/// The engine. Pure and deterministic: all time comes from the `Clock` and event times.
pub struct Engine {
    config: Arc<LoadedConfig>,
    monitors: Vec<MonitorInfo>,
    mode: Mode,
    stack: Vec<String>,
    safe_open: bool,
    /// Layer visibility per (board, layer index); missing = the YAML value.
    layer_visible: BTreeMap<(String, usize), bool>,
    /// The board on screen (top of the stack, or the Safe Board) and its layout.
    shown: Option<Shown>,
    /// A `uca-ui` screen hit-tested outside the shown board (05 "Settings window").
    virtual_board: Option<VirtualBoard>,
    pointer: Option<FilteredPoint>,
    hover: Option<ItemRef>,
    dwell: Dwell,
    must_leave: Option<ItemRef>,
    cooldown_until: BTreeMap<ItemRef, Instant>,
    armed: Option<(ItemRef, Instant)>,
    latched: BTreeMap<Key, Latch>,
    present: bool,
    presence_resume_at: Option<Instant>,
    sets: SetStack,
    activations: Vec<Activation>,
    next_id: u64,
    mapping: Mapping,
    escape: Option<Mapping>,
    intents: Vec<Intent>,
}

struct Shown {
    file: BoardFile,
    layout: BoardLayout,
    resolved: Vec<Vec<ResolvedItem>>,
    safe: bool,
}

fn millis(n: f64) -> Duration {
    Duration::from_secs_f64(n.max(0.0) / 1000.0)
}

impl Engine {
    pub fn new(config: Arc<LoadedConfig>, monitors: Vec<MonitorInfo>) -> Self {
        let mapping = Mapping::new(&config);
        let profile = &config.profile.profile;
        let escape = profile
            .safety
            .as_ref()
            .and_then(|s| Mapping::single(&s.escape, profile));
        let mut engine = Self {
            stack: vec![profile.start_board.0.clone()],
            config,
            monitors,
            mode: Mode::Direct,
            safe_open: false,
            layer_visible: BTreeMap::new(),
            shown: None,
            virtual_board: None,
            pointer: None,
            hover: None,
            dwell: Dwell::Idle,
            must_leave: None,
            cooldown_until: BTreeMap::new(),
            armed: None,
            latched: BTreeMap::new(),
            present: true,
            presence_resume_at: None,
            sets: SetStack::default(),
            activations: Vec::new(),
            next_id: 0,
            mapping,
            escape,
            intents: Vec::new(),
        };
        engine.show_top();
        engine
    }

    // ------------------------------------------------------------ public API

    pub fn on_signal(&mut self, input: EngineInput, clock: &dyn Clock) {
        let now = clock.now();
        self.advance_to(now);
        match input {
            EngineInput::Point(p) => self.on_point(p, now),
            EngineInput::Escape(e) => self.on_escape(&e, now),
            EngineInput::Trigger(t) => self.on_trigger(&t, now),
            EngineInput::Gesture(g) => {
                if !self.safe_open {
                    let ctx = self.context();
                    let events = self.mapping.on_gesture(&g, Some(&ctx));
                    self.mapping_events(events, now);
                }
            }
            EngineInput::Status(s) => match s.status {
                SourceStatus::UserLost => self.set_presence(false, now),
                SourceStatus::UserFound => self.set_presence(true, now),
                _ => {}
            },
            // Fallback to another method arrives in M2 (SAFETY-06).
            EngineInput::PointerSourceLost(_) => {}
        }
    }

    pub fn on_tick(&mut self, clock: &dyn Clock) {
        self.advance_to(clock.now());
    }

    /// A new valid config (hot reload): safety reset, then start from the new boards.
    pub fn on_config(&mut self, config: Arc<LoadedConfig>, clock: &dyn Clock) {
        let now = clock.now();
        self.advance_to(now);
        self.safety_event(now);
        self.mapping = Mapping::new(&config);
        let profile = &config.profile.profile;
        self.escape = profile
            .safety
            .as_ref()
            .and_then(|s| Mapping::single(&s.escape, profile));
        self.stack
            .retain(|b| config.boards.values().any(|f| f.board.id.0 == *b));
        if self.stack.is_empty() {
            self.stack.push(profile.start_board.0.clone());
        }
        self.config = config;
        self.layer_visible.clear();
        self.cooldown_until.clear();
        self.show_top();
    }

    /// Requests from outside the input path (tray menu, settings).
    pub fn control(&mut self, c: EngineControl, clock: &dyn Clock) {
        let now = clock.now();
        self.advance_to(now);
        match c {
            EngineControl::TogglePause if self.mode == Mode::Paused => self.mode = Mode::Direct,
            EngineControl::TogglePause => self.pause(now),
            EngineControl::OpenSafeBoard => self.open_safe_board(now),
        }
    }

    pub fn set_monitors(&mut self, monitors: Vec<MonitorInfo>) {
        self.monitors = monitors;
        self.show_top();
    }

    /// Registers a `uca-ui` screen's widgets, or removes them with `None`
    /// (05 "Settings window"). An unchanged board is ignored, so this may be called every frame.
    pub fn set_virtual_board(&mut self, board: Option<VirtualBoard>, clock: &dyn Clock) {
        if self.virtual_board == board {
            return;
        }
        let now = clock.now();
        self.advance_to(now);
        let ids = |b: &Option<VirtualBoard>| {
            b.as_ref().map(|b| {
                let items: Vec<String> = b.items.iter().map(|i| i.id.clone()).collect();
                (b.id.clone(), items)
            })
        };
        // Different widgets: references by index would point at the wrong ones.
        if ids(&self.virtual_board) != ids(&board) {
            self.forget_virtual();
        }
        self.virtual_board = board;
        self.refresh_hover(now);
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        let mut d: Vec<Instant> = Vec::new();
        for a in &self.activations {
            if let Some(r) = &a.run {
                d.push(r.next_at);
            } else if !a.ending {
                d.extend(a.next_repeat);
            }
        }
        d.extend(self.dwell_deadline());
        d.extend(self.presence_resume_at);
        d.extend(self.armed.map(|(_, until)| until));
        d.extend(self.mapping.next_deadline());
        d.extend(self.escape.as_ref().and_then(Mapping::next_deadline));
        if let (Some(h), Dwell::Idle) = (self.hover, &self.dwell) {
            d.extend(self.cooldown_until.get(&h).copied());
        }
        d.into_iter().min()
    }

    pub fn drain_intents(&mut self) -> Vec<Intent> {
        std::mem::take(&mut self.intents)
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn board_stack(&self) -> &[String] {
        &self.stack
    }

    pub fn safe_board_open(&self) -> bool {
        self.safe_open
    }

    // ------------------------------------------------------------ time

    /// Processes everything due up to `now`, in time order.
    fn advance_to(&mut self, now: Instant) {
        let mut guard = 0;
        while let Some(t) = self.next_deadline().filter(|t| *t <= now) {
            self.process_at(t);
            guard += 1;
            if guard > 100_000 {
                tracing::error!(target: "uca::engine", "deadline loop did not settle");
                break;
            }
        }
    }

    fn process_at(&mut self, t: Instant) {
        if let Some(esc) = self.escape.as_mut() {
            let events = esc.on_tick(t, None);
            if events
                .iter()
                .any(|e| matches!(e, MappingEvent::Start { .. }))
            {
                self.open_safe_board(t);
            }
        }
        if !self.safe_open {
            let ctx = self.context();
            let events = self.mapping.on_tick(t, Some(&ctx));
            self.mapping_events(events, t);
        }
        if self.presence_resume_at.is_some_and(|r| r <= t) {
            self.presence_resume_at = None;
            self.refresh_hover(t);
        }
        if self.armed.is_some_and(|(_, until)| until <= t) {
            self.armed = None;
        }
        self.dwell_tick(t);
        self.run_activations(t);
    }

    // ------------------------------------------------------------ boards

    fn board_file(&self, id: &str) -> Option<&BoardFile> {
        self.config.boards.values().find(|f| f.board.id.0 == id)
    }

    fn show_top(&mut self) {
        let (file, safe) = if self.safe_open {
            (safe_board(), true)
        } else {
            match self.stack.last().and_then(|id| self.board_file(id)) {
                Some(f) => (f.clone(), false),
                None => {
                    self.shown = None;
                    return;
                }
            }
        };
        let pointer_monitor = self.pointer.as_ref().map(|p| p.monitor);
        let Some(monitor) = choose_monitor(&file.board, &self.monitors, pointer_monitor).cloned()
        else {
            self.shown = None;
            return;
        };
        let layout = layout_board(&file, &monitor);
        let theme = file
            .board
            .theme
            .as_ref()
            .and_then(|t| self.config.themes.get(t))
            .map(|(_, t)| t);
        let cascade = Cascade::new(&file, theme);
        let resolved = file
            .layers
            .iter()
            .map(|l| l.items.iter().map(|i| cascade.item(l, i)).collect())
            .collect();
        self.shown = Some(Shown {
            file,
            layout,
            resolved,
            safe,
        });
        self.hover = None;
        self.dwell = Dwell::Idle;
        self.must_leave = None;
        self.armed = None;
    }

    fn layer_is_visible(&self, board: &str, layer: usize) -> bool {
        self.layer_visible
            .get(&(board.to_owned(), layer))
            .copied()
            .unwrap_or_else(|| {
                self.shown
                    .as_ref()
                    .and_then(|s| s.file.layers.get(layer))
                    .and_then(|l| l.visible)
                    .unwrap_or(true)
            })
    }

    fn shown_id(&self) -> Option<String> {
        self.shown.as_ref().map(|s| s.file.board.id.0.clone())
    }

    fn resolved(&self, r: ItemRef) -> Option<&ResolvedItem> {
        if r.virt {
            return None;
        }
        self.shown.as_ref()?.resolved.get(r.layer)?.get(r.item)
    }

    /// The board changed: items no longer shown end their activations normally (04 rule 5).
    fn board_changed(&mut self, t: Instant) {
        self.show_top();
        for a in &mut self.activations {
            if matches!(a.owner, Owner::Item(_)) {
                a.ending = true;
            }
        }
        self.refresh_hover(t);
    }

    // ------------------------------------------------------------ hit-testing

    fn item_entry(&self, r: ItemRef) -> Option<(String, ActionSetEntry)> {
        let item = &self.resolved(r)?.item;
        resolve_set(item, &self.sets)
    }

    /// The shown board wins wherever it is; the virtual board is hit-tested
    /// only outside it, and never while the Safe Board is open (05, 09).
    fn hit(&self, p: &FilteredPoint) -> Option<ItemRef> {
        let on_board = self.shown.as_ref().is_some_and(|s| {
            p.monitor == s.layout.monitor && s.layout.rect.contains(p.logical.x, p.logical.y)
        });
        if on_board {
            self.board_hit(p)
        } else if self.safe_open {
            None
        } else {
            self.virtual_hit(p)
        }
    }

    /// Points where another window covers the virtual board's window don't
    /// count, as with `ignore_covered` on a board (02).
    fn virtual_hit(&self, p: &FilteredPoint) -> Option<ItemRef> {
        let vb = self.virtual_board.as_ref()?;
        if p.monitor != vb.monitor || p.covered {
            return None;
        }
        vb.items
            .iter()
            .rposition(|i| i.rect.contains(p.logical.x, p.logical.y))
            .map(ItemRef::virtual_item)
    }

    fn board_hit(&self, p: &FilteredPoint) -> Option<ItemRef> {
        let shown = self.shown.as_ref()?;
        if p.covered && shown.file.board.ignore_covered_or_default() {
            return None;
        }
        let board = shown.file.board.id.0.clone();
        for (li, layer) in shown.layout.layers.iter().enumerate().rev() {
            if !self.layer_is_visible(&board, li) {
                continue;
            }
            for item in layer.items.iter().rev() {
                let r = ItemRef::board(li, item.index);
                let Some(res) = self.resolved(r) else {
                    continue;
                };
                let usable = item.placed
                    && res.item.visible != Some(false)
                    && res.item.empty != Some(true)
                    && (shown.safe || self.item_entry(r).is_some());
                if usable && item.rect.contains(p.logical.x, p.logical.y) {
                    return Some(r);
                }
            }
        }
        None
    }

    fn forget_virtual(&mut self) {
        if self.hover.is_some_and(|h| h.virt) {
            self.hover = None;
        }
        if let Dwell::Dwelling { item, .. } | Dwell::Grace { item, .. } = &self.dwell {
            if item.virt {
                self.dwell = Dwell::Idle;
            }
        }
        if self.must_leave.is_some_and(|m| m.virt) {
            self.must_leave = None;
        }
        self.cooldown_until.retain(|r, _| !r.virt);
    }

    fn refresh_hover(&mut self, t: Instant) {
        let hit = self.pointer.clone().and_then(|p| self.hit(&p));
        self.update_hover(hit, t);
    }

    // ------------------------------------------------------------ dwell

    fn interactive(&self) -> bool {
        self.safe_open || self.mode == Mode::Direct
    }

    fn present_now(&self) -> bool {
        self.present && self.presence_resume_at.is_none()
    }

    fn behavior(&self, r: ItemRef) -> Option<ResolvedBehavior> {
        if r.virt {
            // Virtual items have no cascade: the built-in behavior (02).
            return Some(behavior_over_builtin(None));
        }
        let res = self.resolved(r)?;
        Some(match self.item_entry(r) {
            Some((_, entry)) => res.behavior_for(&entry),
            None => res.behavior.clone(),
        })
    }

    fn dwell_ms(&self, r: ItemRef) -> Option<f64> {
        self.behavior(r).and_then(|b| b.dwell_ms)
    }

    fn can_start_dwell(&self, r: ItemRef, t: Instant) -> bool {
        self.interactive()
            && self.present_now()
            && self.must_leave != Some(r)
            && self.cooldown_until.get(&r).is_none_or(|c| *c <= t)
            && self.dwell_ms(r).is_some()
    }

    fn update_hover(&mut self, hit: Option<ItemRef>, t: Instant) {
        self.hover = hit;
        if self.must_leave.is_some() && self.must_leave != hit {
            self.must_leave = None;
        }
        self.dwell = match self.dwell.clone() {
            Dwell::Idle => match hit {
                Some(r) if self.can_start_dwell(r, t) => Dwell::Dwelling {
                    item: r,
                    done: Duration::ZERO,
                    since: t,
                },
                _ => Dwell::Idle,
            },
            Dwell::Dwelling { item, done, since } if hit != Some(item) => Dwell::Grace {
                item,
                done: done + t.saturating_duration_since(since),
                left_at: t,
            },
            Dwell::Grace { item, done, .. } if hit == Some(item) => Dwell::Dwelling {
                item,
                done,
                since: t,
            },
            Dwell::Holding {
                item,
                activation,
                left_at,
            } => Dwell::Holding {
                item,
                activation,
                left_at: if hit == Some(item) {
                    None
                } else {
                    left_at.or(Some(t))
                },
            },
            other => other,
        };
    }

    fn dwell_deadline(&self) -> Option<Instant> {
        match &self.dwell {
            Dwell::Idle => None,
            Dwell::Dwelling { item, done, since } => {
                let total = millis(self.dwell_ms(*item)?);
                Some(*since + total.saturating_sub(*done))
            }
            Dwell::Grace { item, left_at, .. }
            | Dwell::Holding {
                item,
                left_at: Some(left_at),
                ..
            } => Some(*left_at + millis(self.behavior(*item)?.dwell_grace_ms)),
            Dwell::Holding { left_at: None, .. } => None,
        }
    }

    fn dwell_tick(&mut self, t: Instant) {
        if let (Some(h), Dwell::Idle) = (self.hover, &self.dwell) {
            if self.cooldown_until.get(&h).is_some_and(|c| *c <= t) {
                self.cooldown_until.remove(&h);
                self.update_hover(Some(h), t);
            }
        }
        let Some(due) = self.dwell_deadline().filter(|d| *d <= t) else {
            return;
        };
        match self.dwell.clone() {
            Dwell::Dwelling { item, .. } => {
                self.dwell = Dwell::Idle;
                self.must_leave = Some(item);
                // `dwell` as a pattern input fires just before the item's own activation (04).
                if !self.safe_open {
                    let ctx = self.context();
                    let events = self.mapping.on_dwell(due, Some(&ctx));
                    self.mapping_events(events, due);
                }
                if let Some(id) = self.activate_item(item, None, due) {
                    if self
                        .activation(id)
                        .is_some_and(|a| a.hold == Hold::Release && !a.ending)
                    {
                        self.dwell = Dwell::Holding {
                            item,
                            activation: id,
                            left_at: None,
                        };
                    }
                }
            }
            Dwell::Grace { .. } => {
                self.dwell = Dwell::Idle;
                let hover = self.hover;
                self.update_hover(hover, due);
            }
            Dwell::Holding { activation, .. } => {
                self.dwell = Dwell::Idle;
                self.end_activation(activation, due);
                // The point is elsewhere now: it may start dwelling there.
                let hover = self.hover;
                self.update_hover(hover, due);
            }
            Dwell::Idle => {}
        }
    }

    // ------------------------------------------------------------ presence and safety

    fn set_presence(&mut self, present: bool, t: Instant) {
        if present == self.present {
            return;
        }
        self.present = present;
        if present {
            let delay = match settings_registry::get("presence.resume_delay_ms").map(|d| d.default)
            {
                Ok(SettingValue::Number(n)) => n,
                _ => 0.0,
            };
            let delay = match self
                .config
                .profile
                .profile
                .inputs
                .presence
                .as_ref()
                .and_then(|p| p.resume_delay_ms)
            {
                Some(SettingValue::Number(n)) => n,
                _ => delay,
            };
            self.presence_resume_at = Some(t + millis(delay));
        } else {
            // SAFETY-03: presence loss cancels dwell and open activations.
            tracing::debug!(target: "uca::engine", "presence lost");
            self.presence_resume_at = None;
            self.safety_event(t);
        }
    }

    /// Cancels open activations (no stop stage) and releases everything (04 rule 6, SAFETY-08).
    fn safety_event(&mut self, _t: Instant) {
        for a in std::mem::take(&mut self.activations) {
            self.sets.release(a.id);
        }
        self.intents.push(Intent::ReleaseAll);
        self.dwell = Dwell::Idle;
        self.armed = None;
        self.mapping.reset();
    }

    fn on_escape(&mut self, e: &TriggerEvent, t: Instant) {
        let Some(esc) = self.escape.as_mut() else {
            return;
        };
        let events = esc.on_trigger(e, None);
        if events
            .iter()
            .any(|e| matches!(e, MappingEvent::Start { .. }))
        {
            self.open_safe_board(t);
        }
    }

    fn open_safe_board(&mut self, t: Instant) {
        if self.safe_open {
            return;
        }
        tracing::info!(target: "uca::engine", "safe board opened");
        self.safety_event(t);
        // SAFETY-14: the action-set stack resets.
        self.sets.reset();
        self.safe_open = true;
        self.show_top();
        self.refresh_hover(t);
    }

    fn close_safe_board(&mut self, t: Instant) {
        self.safety_event(t);
        self.safe_open = false;
        self.mode = Mode::Direct;
        self.show_top();
        self.refresh_hover(t);
    }

    // ------------------------------------------------------------ pointer and triggers

    fn on_point(&mut self, p: FilteredPoint, t: Instant) {
        let presence = p.presence;
        self.pointer = Some(p);
        self.set_presence(presence, t);
        self.refresh_hover(t);
    }

    fn safe_select_trigger(&self, t: &TriggerId) -> bool {
        t.to_string() == "mouse:left"
            || self
                .config
                .profile
                .profile
                .inputs
                .switches
                .values()
                .any(|s| s.matches(t))
    }

    fn on_trigger(&mut self, e: &TriggerEvent, t: Instant) {
        if self.safe_open {
            if e.phase == uca_types::Phase::Down && self.safe_select_trigger(&e.trigger) {
                if let Some(h) = self.hover {
                    self.activate_item(h, None, t);
                }
            }
            return;
        }
        let ctx = self.context();
        let events = self.mapping.on_trigger(e, Some(&ctx));
        self.mapping_events(events, t);
    }

    fn context(&self) -> MatchContext {
        let board = self.stack.last().cloned().unwrap_or_default();
        let visible_layers = self
            .shown
            .as_ref()
            .map(|s| {
                s.file
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| self.layer_is_visible(&board, *i))
                    .map(|(_, l)| l.id.0.clone())
                    .collect()
            })
            .unwrap_or_default();
        let hovered = self.hover.filter(|h| !h.virt).and_then(|h| {
            let shown = self.shown.as_ref()?;
            let layer = shown.layout.layers.get(h.layer)?;
            let item = layer.items.get(h.item)?;
            Some((board.clone(), layer.id.clone(), item.id.clone()))
        });
        MatchContext {
            mode: self.mode,
            top_board: board,
            visible_layers,
            hovered,
            latched: self.latched.keys().copied().collect(),
            app: None,
            action_sets: self.sets.ids(),
        }
    }

    fn mapping_events(&mut self, events: Vec<MappingEvent>, t: Instant) {
        for ev in events {
            match ev {
                MappingEvent::Start { binding, ends, .. } => {
                    let Some(b) = self.mapping.binding(&binding).cloned() else {
                        continue;
                    };
                    // In Paused, only bindings that ask for it fire (04).
                    let wants_paused =
                        b.context.as_ref().and_then(|c| c.mode) == Some(Mode::Paused);
                    if self.mode == Mode::Paused && !wants_paused {
                        continue;
                    }
                    self.start_binding(binding, &b, ends, t);
                }
                MappingEvent::End { binding, .. } => {
                    let ids: Vec<ActivationId> = self
                        .activations
                        .iter()
                        .filter(|a| {
                            a.owner == Owner::Binding(binding.clone())
                                || a.ended_by.as_ref() == Some(&binding)
                        })
                        .map(|a| a.id)
                        .collect();
                    for id in ids {
                        self.end_activation(id, t);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ activations

    fn new_id(&mut self) -> ActivationId {
        self.next_id += 1;
        ActivationId(self.next_id)
    }

    fn activation(&self, id: ActivationId) -> Option<&Activation> {
        self.activations.iter().find(|a| a.id == id)
    }

    fn expand(&self, seq: Option<&ActionSeq>) -> Vec<Action> {
        let mut out = Vec::new();
        let mut stack: Vec<(Action, usize)> = seq
            .map(|s| s.0.iter().rev().map(|a| (a.clone(), 0)).collect())
            .unwrap_or_default();
        while let Some((a, depth)) = stack.pop() {
            match &a {
                Action::MacroRun(id) if depth < 32 => {
                    if let Some(m) = self.config.profile.profile.macros.get(id) {
                        stack.extend(m.do_.0.iter().rev().map(|x| (x.clone(), depth + 1)));
                    }
                }
                Action::MacroRun(_) => {}
                _ => out.push(a),
            }
        }
        out
    }

    fn start_binding(&mut self, key: BindingKey, b: &Binding, ends: Ends, t: Instant) {
        let id = self.new_id();
        let start = self.expand(b.do_.as_ref().or(b.start.as_ref()));
        let stop = self.expand(b.stop.as_ref());
        let repeat = match &b.repeat {
            None => None,
            Some(Repeat::Start) => Some(start.clone()),
            Some(Repeat::Seq(s)) => Some(self.expand(Some(s))),
        };
        let hold = match ends {
            Ends::Instant => Hold::Instant,
            Ends::OnRelease(_) => Hold::Release,
        };
        self.sets.consume_once(id);
        self.begin(
            Activation {
                id,
                owner: Owner::Binding(key),
                ended_by: None,
                start,
                repeat,
                stop,
                hold,
                behavior: behavior_over_builtin(b.behavior.as_ref()),
                run: None,
                next_repeat: None,
                ending: false,
            },
            t,
        );
    }

    /// Activates an item by dwell or `select`. Returns the activation, if one started.
    fn activate_item(
        &mut self,
        r: ItemRef,
        ended_by: Option<BindingKey>,
        t: Instant,
    ) -> Option<ActivationId> {
        if !self.interactive() {
            return None;
        }
        if r.virt {
            self.activate_virtual(r, t);
            return None;
        }
        if self.shown.as_ref().is_some_and(|s| s.safe) {
            let safe = SAFE_ITEMS.get(r.item).copied()?;
            self.safe_action(safe, t);
            return None;
        }
        // A toggled activation of this item ends on its next activation (04 hold: toggle).
        if let Some(open) = self
            .activations
            .iter()
            .find(|a| a.owner == Owner::Item(r) && a.hold == Hold::Toggle && !a.ending)
            .map(|a| a.id)
        {
            self.end_activation(open, t);
            return None;
        }
        let behavior = self.behavior(r)?;
        if behavior.confirm {
            match self.armed {
                Some((armed, until)) if armed == r && t <= until => self.armed = None,
                _ => {
                    self.armed = Some((r, t + millis(behavior.confirm_window_ms)));
                    return None;
                }
            }
        }
        let (_set, entry) = self.item_entry(r)?;
        let id = self.new_id();
        let start = self.expand(entry.start.as_ref());
        let stop = self.expand(entry.stop.as_ref());
        let repeat = match &entry.repeat {
            None => None,
            Some(Repeat::Start) => Some(start.clone()),
            Some(Repeat::Seq(s)) => Some(self.expand(Some(s))),
        };
        self.sets.consume_once(id);
        self.begin(
            Activation {
                id,
                owner: Owner::Item(r),
                ended_by,
                start,
                repeat,
                stop,
                hold: behavior.hold,
                behavior,
                run: None,
                next_repeat: None,
                ending: false,
            },
            t,
        );
        Some(id)
    }

    /// Virtual items run no stages: `uca-ui` performs the widget's own action.
    fn activate_virtual(&mut self, r: ItemRef, t: Instant) {
        let Some(vb) = self.virtual_board.as_ref() else {
            return;
        };
        let Some(item) = vb.items.get(r.item) else {
            return;
        };
        tracing::debug!(target: "uca::engine", board = %vb.id, item = %item.id, "virtual item activated");
        self.intents.push(Intent::Virtual {
            board: vb.id.clone(),
            item: item.id.clone(),
        });
        let cooldown = millis(behavior_over_builtin(None).cooldown_ms);
        if !cooldown.is_zero() {
            self.cooldown_until.insert(r, t + cooldown);
        }
    }

    fn safe_action(&mut self, item: SafeItem, t: Instant) {
        match item {
            SafeItem::Resume => self.close_safe_board(t),
            SafeItem::Recalibrate => self.intents.push(Intent::Control(Control::Recalibrate)),
            SafeItem::Restart => self.intents.push(Intent::Control(Control::RestartApp)),
            SafeItem::SwitchMethod => {}
        }
    }

    fn begin(&mut self, mut a: Activation, t: Instant) {
        tracing::debug!(target: "uca::engine", id = a.id.0, "activation start");
        a.run = Some(Run {
            stage: StageKind::Start,
            steps: a.start.clone(),
            pos: 0,
            next_at: t,
            started: t,
        });
        let id = a.id;
        self.activations.push(a);
        self.step(id, t);
    }

    fn end_activation(&mut self, id: ActivationId, t: Instant) {
        if let Some(a) = self.activations.iter_mut().find(|a| a.id == id) {
            a.ending = true;
            if a.run.is_none() {
                a.run = Some(Run {
                    stage: StageKind::Stop,
                    steps: a.stop.clone(),
                    pos: 0,
                    next_at: t,
                    started: t,
                });
            }
        }
        self.step(id, t);
    }

    fn run_activations(&mut self, t: Instant) {
        let ids: Vec<ActivationId> = self.activations.iter().map(|a| a.id).collect();
        for id in ids {
            let due_repeat = self.activation(id).is_some_and(|a| {
                a.run.is_none() && !a.ending && a.next_repeat.is_some_and(|r| r <= t)
            });
            if due_repeat {
                if let Some(a) = self.activations.iter_mut().find(|a| a.id == id) {
                    let at = a.next_repeat.take().unwrap_or(t);
                    let steps = a.repeat.clone().unwrap_or_default();
                    a.run = Some(Run {
                        stage: StageKind::Repeat,
                        steps,
                        pos: 0,
                        next_at: at,
                        started: at,
                    });
                }
            }
            self.step(id, t);
        }
    }

    /// Runs the activation's current run up to `now`: actions at their exact times,
    /// `delay` steps in between; then schedules the next stage (04 timing rules).
    fn step(&mut self, id: ActivationId, now: Instant) {
        loop {
            let Some(a) = self.activations.iter().find(|a| a.id == id) else {
                return;
            };
            let Some(run) = a.run.clone() else { return };
            if run.next_at > now {
                return;
            }
            let at = run.next_at;
            // Run steps from `pos` until a delay or the end.
            let mut pos = run.pos;
            let mut device = Vec::new();
            let mut next_at = None;
            while pos < run.steps.len() {
                let action = run.steps[pos].clone();
                pos += 1;
                if let Action::Delay(ms) = action {
                    next_at = Some(at + Duration::from_millis(ms));
                    break;
                }
                if let Some(d) = self.engine_action(&action, id, run.stage, at) {
                    device.extend(d);
                }
            }
            if !device.is_empty() {
                self.intents.push(Intent::Execute {
                    activation: id,
                    actions: device,
                });
            }
            let Some(a) = self.activations.iter_mut().find(|a| a.id == id) else {
                return;
            };
            if let Some(next) = next_at {
                if let Some(r) = a.run.as_mut() {
                    r.pos = pos;
                    r.next_at = next;
                }
                continue;
            }
            // The run is complete at `at`.
            a.run = None;
            match run.stage {
                StageKind::Start | StageKind::Repeat => {
                    if a.hold == Hold::Instant {
                        a.ending = true;
                    }
                    if a.ending {
                        a.run = Some(Run {
                            stage: StageKind::Stop,
                            steps: a.stop.clone(),
                            pos: 0,
                            next_at: at,
                            started: at,
                        });
                    } else if a.repeat.is_some() {
                        a.next_repeat = Some(match run.stage {
                            StageKind::Start => at + millis(a.behavior.repeat_delay_ms),
                            _ => (run.started + millis(a.behavior.repeat_interval_ms)).max(at),
                        });
                    }
                }
                StageKind::Stop => {
                    self.finish(id, at);
                    return;
                }
            }
        }
    }

    /// The activation is over: release its presses, its held sets, start cooldown.
    fn finish(&mut self, id: ActivationId, at: Instant) {
        let Some(pos) = self.activations.iter().position(|a| a.id == id) else {
            return;
        };
        let a = self.activations.remove(pos);
        tracing::debug!(target: "uca::engine", id = id.0, "activation end");
        self.sets.release(id);
        self.intents.push(Intent::Release(id));
        if let Owner::Item(r) = a.owner {
            let cooldown = millis(a.behavior.cooldown_ms);
            if !cooldown.is_zero() {
                self.cooldown_until.insert(r, at + cooldown);
            }
            if matches!(self.dwell, Dwell::Holding { activation, .. } if activation == id) {
                self.dwell = Dwell::Idle;
            }
        }
    }

    /// Applies engine actions; returns device actions for the executor.
    fn engine_action(
        &mut self,
        a: &Action,
        by: ActivationId,
        stage: StageKind,
        t: Instant,
    ) -> Option<Vec<Action>> {
        match a {
            Action::BoardOpen(b) => {
                if self.board_file(&b.0).is_some() {
                    self.stack.push(b.0.clone());
                    self.board_changed(t);
                }
            }
            Action::BoardBack => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                    self.board_changed(t);
                }
            }
            Action::BoardReplace(b) => {
                if self.board_file(&b.0).is_some() {
                    self.stack.pop();
                    self.stack.push(b.0.clone());
                    self.board_changed(t);
                }
            }
            Action::LayerShow(l) | Action::LayerHide(l) | Action::LayerToggle(l) => {
                let board = self.shown_id()?;
                let index = self
                    .shown
                    .as_ref()
                    .and_then(|s| s.file.layers.iter().position(|x| x.id == *l));
                match index {
                    None => {
                        tracing::debug!(target: "uca::engine", layer = %l, "layer not on the top board")
                    }
                    Some(i) => {
                        let now = self.layer_is_visible(&board, i);
                        let next = match a {
                            Action::LayerShow(_) => true,
                            Action::LayerHide(_) => false,
                            _ => !now,
                        };
                        self.layer_visible.insert((board, i), next);
                        self.refresh_hover(t);
                    }
                }
            }
            Action::ActionSetHold(s) if stage == StageKind::Start => self.sets.hold(&s.0, by),
            Action::ActionSetHold(_) => {}
            Action::ActionSetOn(s) => self.sets.on(&s.0),
            Action::ActionSetOff(s) => self.sets.off(&s.0),
            Action::ActionSetToggle(s) => self.sets.toggle(&s.0),
            Action::ActionSetSwitch(s) => self.sets.switch(&s.0),
            Action::ActionSetCycle(list) => {
                let ids: Vec<String> = list.iter().map(|i| i.0.clone()).collect();
                self.sets.cycle(&ids);
            }
            Action::ActionSetOnce(s) => self.sets.once(&s.0, by),
            Action::ActionSetReset => self.sets.reset(),
            Action::ModeSet(Mode::Paused) | Action::Pause => self.pause(t),
            Action::ModeSet(Mode::Direct) | Action::Resume => self.mode = Mode::Direct,
            Action::Select => {
                if let Some(h) = self.hover {
                    let binding = self.activation(by).and_then(|a| match &a.owner {
                        Owner::Binding(k) if a.hold == Hold::Release => Some(k.clone()),
                        _ => None,
                    });
                    self.activate_item(h, binding, t);
                }
            }
            Action::Recalibrate => self.intents.push(Intent::Control(Control::Recalibrate)),
            Action::SettingsOpen => self.intents.push(Intent::Control(Control::OpenSettings)),
            Action::SettingsClose => self.intents.push(Intent::Control(Control::CloseSettings)),
            Action::KeyLatch(k) => {
                let next = match self.latched.get(k) {
                    None => Some(Latch::Latched),
                    Some(Latch::Latched) => Some(Latch::Locked),
                    Some(Latch::Locked) => None,
                };
                match next {
                    Some(l) => self.latched.insert(*k, l),
                    None => self.latched.remove(k),
                };
            }
            Action::KeyTap(k) if !self.latched.is_empty() => {
                // Latched modifiers wrap the tap, then non-locked latches clear (04).
                let mods: Vec<Key> = self.latched.keys().copied().collect();
                let mut out: Vec<Action> = mods.iter().map(|m| Action::KeyDown(*m)).collect();
                out.push(Action::KeyTap(*k));
                out.extend(mods.iter().rev().map(|m| Action::KeyUp(*m)));
                self.latched.retain(|_, l| *l == Latch::Locked);
                return Some(out);
            }
            Action::MacroRun(_) | Action::Delay(_) => {}
            device => return Some(vec![device.clone()]),
        }
        None
    }

    fn pause(&mut self, t: Instant) {
        if self.mode != Mode::Paused {
            self.mode = Mode::Paused;
            self.safety_event(t);
        }
    }

    // ------------------------------------------------------------ snapshot

    pub fn snapshot(&self, clock: &dyn Clock) -> EngineSnapshot {
        let now = clock.now();
        let board = self.shown.as_ref().map(|s| {
            let board_id = s.file.board.id.0.clone();
            BoardSnapshot {
                id: board_id.clone(),
                monitor: s.layout.monitor,
                rect: s.layout.rect,
                layers: s
                    .layout
                    .layers
                    .iter()
                    .enumerate()
                    .map(|(li, l)| LayerSnapshot {
                        id: l.id.clone(),
                        visible: self.layer_is_visible(&board_id, li),
                        rect: l.rect,
                        items: l
                            .items
                            .iter()
                            .map(|it| self.item_snapshot(s, li, it, now))
                            .collect(),
                    })
                    .collect(),
            }
        });
        EngineSnapshot {
            mode: self.mode,
            safe_board_open: self.safe_open,
            board,
            pointer: self.pointer.clone(),
            presence: self.present_now(),
            action_sets: self.sets.ids(),
            latched: self.latched.iter().map(|(k, l)| (*k, *l)).collect(),
            virtual_board: self.virtual_board.as_ref().map(|vb| VirtualSnapshot {
                id: vb.id.clone(),
                monitor: vb.monitor,
                items: vb
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        let r = ItemRef::virtual_item(i);
                        VirtualItemSnapshot {
                            id: item.id.clone(),
                            hover: self.hover == Some(r),
                            dwell_progress: self.dwell_progress(r, now),
                        }
                    })
                    .collect(),
            }),
            debug: self.debug_snapshot(now),
        }
    }

    fn debug_snapshot(&self, now: Instant) -> DebugSnapshot {
        DebugSnapshot {
            hovered: self.hover.and_then(|h| self.item_name(h)),
            dwell: self.dwell_snapshot(now),
        }
    }

    fn dwell_snapshot(&self, now: Instant) -> Option<DwellSnapshot> {
        let (Dwell::Dwelling { item, .. } | Dwell::Grace { item, .. }) = &self.dwell else {
            return None;
        };
        let remaining = self.dwell_deadline()?.saturating_duration_since(now);
        Some(DwellSnapshot {
            item: self.item_name(*item)?,
            progress: self.dwell_progress(*item, now)?,
            remaining_ms: u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX),
            grace: matches!(self.dwell, Dwell::Grace { .. }),
        })
    }

    /// `board/layer/item`, or `virtual-board/item`.
    fn item_name(&self, r: ItemRef) -> Option<String> {
        if r.virt {
            let vb = self.virtual_board.as_ref()?;
            let item = vb.items.get(r.item)?;
            return Some(format!("{}/{}", vb.id, item.id));
        }
        let shown = self.shown.as_ref()?;
        let layer = shown.layout.layers.get(r.layer)?;
        let item = layer.items.iter().find(|i| i.index == r.item)?;
        Some(format!(
            "{}/{}/{}",
            shown.file.board.id.0, layer.id, item.id
        ))
    }

    /// Dwell progress 0..1 while dwelling on `r` or in its grace.
    fn dwell_progress(&self, r: ItemRef, now: Instant) -> Option<f32> {
        match &self.dwell {
            Dwell::Dwelling { item, done, since } if *item == r => self.dwell_ms(r).map(|ms| {
                ((*done + now.saturating_duration_since(*since)).as_secs_f64() * 1000.0 / ms)
                    .min(1.0) as f32
            }),
            Dwell::Grace { item, done, .. } if *item == r => self
                .dwell_ms(r)
                .map(|ms| (done.as_secs_f64() * 1000.0 / ms).min(1.0) as f32),
            _ => None,
        }
    }

    fn item_snapshot(
        &self,
        s: &Shown,
        li: usize,
        it: &crate::layout::ItemLayout,
        now: Instant,
    ) -> ItemSnapshot {
        let r = ItemRef::board(li, it.index);
        let res = self.resolved(r);
        let entry = if s.safe { None } else { self.item_entry(r) };
        let open = self.activations.iter().find(|a| a.owner == Owner::Item(r));
        let latch = res
            .and_then(|res| latch_key(&res.item))
            .and_then(|k| self.latched.get(&k).copied());
        let mut states = Vec::new();
        if !s.safe && entry.is_none() {
            states.push(State::Disabled);
        }
        if open.is_some_and(|a| a.hold == Hold::Toggle) || latch.is_some() {
            states.push(State::Latched);
        }
        if self.hover == Some(r) {
            states.push(State::Hover);
        }
        let progress = self.dwell_progress(r, now);
        if progress.is_some() {
            states.push(State::Dwell);
        }
        let armed = self.armed.is_some_and(|(a, _)| a == r);
        if open.is_some() || armed {
            states.push(State::Active);
        }
        ItemSnapshot {
            id: it.id.clone(),
            index: it.index,
            rect: it.rect,
            visible: it.placed
                && res.is_some_and(|r| r.item.visible != Some(false) && r.item.empty != Some(true)),
            states,
            dwell_progress: progress,
            action_set: entry.map(|(set, _)| set),
            armed,
            latch,
            safe_item: s.safe.then(|| SAFE_ITEMS.get(it.index).copied()).flatten(),
        }
    }
}

/// The item's entry in the topmost active set that has one (04 "Resolution").
/// `action:` is the start stage of the `default` set.
fn resolve_set(item: &Item, sets: &SetStack) -> Option<(String, ActionSetEntry)> {
    let shorthand = item.action.as_ref().map(|a| ActionSetEntry {
        start: Some(a.clone()),
        ..ActionSetEntry::default()
    });
    for set in sets.resolution_order() {
        if let Some(actions) = &item.actions {
            if let Some((_, e)) = actions.0.iter().find(|(id, _)| id.0 == set) {
                return Some((set, e.clone()));
            }
        }
        if set == DEFAULT_SET {
            if let Some(e) = &shorthand {
                return Some((set, e.clone()));
            }
        }
    }
    None
}

/// The modifier an item latches, for its `latched` state (05).
fn latch_key(item: &Item) -> Option<Key> {
    let seqs = item.action.iter().chain(
        item.actions
            .iter()
            .flat_map(|a| a.0.iter().filter_map(|(_, e)| e.start.as_ref())),
    );
    seqs.flat_map(|s| s.0.iter()).find_map(|a| match a {
        Action::KeyLatch(k) => Some(*k),
        _ => None,
    })
}
