// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Binding resolution (04 "Bindings"): matches input patterns against trigger,
//! gesture and dwell events, picks the bindings that fire, and tracks binding
//! activations until their input lets go. The engine runs the stages (01).
//!
//! Pure and deterministic: time comes only from event timestamps and the `now`
//! passed to [`Mapping::on_tick`].

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use uca_config::action::Mode;
use uca_config::board::Binding;
use uca_config::pattern::{Input, Modifier, Pattern, Term};
use uca_config::profile::Profile;
use uca_config::store::LoadedConfig;
use uca_types::{GestureEvent, Key, Phase, TriggerEvent, TriggerId};

/// Where a binding is written, from highest to lowest priority (04).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    Item {
        board: String,
        layer: String,
        item: String,
    },
    Layer {
        board: String,
        layer: String,
    },
    Board {
        board: String,
    },
    Profile,
}

impl Scope {
    fn rank(&self) -> u8 {
        match self {
            Scope::Item { .. } => 0,
            Scope::Layer { .. } => 1,
            Scope::Board { .. } => 2,
            Scope::Profile => 3,
        }
    }
}

/// Identifies one binding: its scope and its index in that scope's `bindings`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingKey {
    pub scope: Scope,
    pub index: usize,
}

/// The engine state that context filters and scopes look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchContext {
    pub mode: Mode,
    pub top_board: String,
    /// Visible layers of the top board.
    pub visible_layers: Vec<String>,
    /// `(board, layer, item)` of the hovered item.
    pub hovered: Option<(String, String, String)>,
    pub latched: Vec<Key>,
    pub app: Option<String>,
    /// Active action sets, bottom (`default`) first.
    pub action_sets: Vec<String>,
}

/// What ends a binding activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ends {
    /// Patterns with no ongoing hold (release, double, sequence, gesture, dwell).
    Instant,
    /// When any of these triggers is released.
    OnRelease(Vec<TriggerId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingEvent {
    Start {
        binding: BindingKey,
        at: Instant,
        ends: Ends,
    },
    End {
        binding: BindingKey,
        at: Instant,
    },
}

/// A term with aliases resolved and `x` meaning `x press`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum NInput {
    Trigger(TriggerId),
    Gesture(String),
    Dwell,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum NTerm {
    Input(NInput, NModifier),
    Sequence(Vec<NInput>, Option<u32>),
    Chord(Vec<NInput>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum NModifier {
    Press,
    Release,
    Hold(u32),
    Double(u32),
}

fn normalize(term: &Term, profile: &Profile) -> Option<NTerm> {
    let input = |i: &Input| -> Option<NInput> {
        Some(match i {
            Input::Alias(name) => NInput::Trigger(
                profile
                    .inputs
                    .switches
                    .iter()
                    .find(|(alias, _)| alias.0 == *name)
                    .map(|(_, t)| t.clone())?,
            ),
            Input::Trigger(t) => NInput::Trigger(t.clone()),
            Input::Gesture(g) => NInput::Gesture(g.clone()),
            Input::Dwell => NInput::Dwell,
        })
    };
    Some(match term {
        Term::Input { input: i, modifier } => NTerm::Input(
            input(i)?,
            match modifier.unwrap_or(Modifier::Press) {
                Modifier::Press => NModifier::Press,
                Modifier::Release => NModifier::Release,
                Modifier::Hold(ms) => NModifier::Hold(ms),
                Modifier::Double(ms) => NModifier::Double(ms),
            },
        ),
        Term::Sequence { inputs, within_ms } => {
            NTerm::Sequence(inputs.iter().map(input).collect::<Option<_>>()?, *within_ms)
        }
        Term::Chord(inputs) => NTerm::Chord(inputs.iter().map(input).collect::<Option<_>>()?),
    })
}

#[derive(Debug, Clone)]
struct Compiled {
    key: BindingKey,
    binding: Binding,
    terms: Vec<NTerm>,
}

#[derive(Debug, Clone, Default)]
struct TermState {
    /// Hold: the trigger and when it went down.
    hold_since: Option<(TriggerId, Instant)>,
    /// Double: last Down time.
    last_down: Option<Instant>,
    /// Sequence: next index and start time.
    seq: Option<(usize, Instant)>,
}

/// Matches bindings. Build one per config; [`Mapping::single`] for the escape.
#[derive(Debug, Clone)]
pub struct Mapping {
    compiled: Vec<Compiled>,
    states: BTreeMap<(usize, usize), TermState>,
    pressed: BTreeSet<TriggerId>,
    open: Vec<(BindingKey, Vec<TriggerId>)>,
}

fn ms(n: u32) -> Duration {
    Duration::from_millis(u64::from(n))
}

impl NInput {
    fn matches_trigger(&self, t: &TriggerId) -> bool {
        matches!(self, NInput::Trigger(p) if p.matches(t))
    }
}

impl Mapping {
    /// Every binding of the profile and its boards (items, layers, boards, profile).
    pub fn new(config: &LoadedConfig) -> Self {
        let profile = &config.profile.profile;
        let mut all: Vec<(BindingKey, &Binding)> = Vec::new();
        for file in config.boards.values() {
            let board = file.board.id.0.clone();
            for (i, b) in file.bindings.iter().enumerate() {
                all.push((
                    BindingKey {
                        scope: Scope::Board {
                            board: board.clone(),
                        },
                        index: i,
                    },
                    b,
                ));
            }
            for layer in &file.layers {
                let lid = layer.id.0.clone();
                for (i, b) in layer.bindings.iter().enumerate() {
                    all.push((
                        BindingKey {
                            scope: Scope::Layer {
                                board: board.clone(),
                                layer: lid.clone(),
                            },
                            index: i,
                        },
                        b,
                    ));
                }
                for (n, item) in layer.items.iter().enumerate() {
                    let iid = item
                        .id
                        .as_ref()
                        .map_or_else(|| format!("item-{n}"), |id| id.0.clone());
                    for (i, b) in item.bindings.iter().enumerate() {
                        let scope = Scope::Item {
                            board: board.clone(),
                            layer: lid.clone(),
                            item: iid.clone(),
                        };
                        all.push((BindingKey { scope, index: i }, b));
                    }
                }
            }
        }
        for (i, b) in profile.bindings.iter().enumerate() {
            all.push((
                BindingKey {
                    scope: Scope::Profile,
                    index: i,
                },
                b,
            ));
        }
        Self::from_bindings(all.into_iter().map(|(k, b)| (k, b.clone())), profile)
    }

    /// A mapping for one pattern, outside any context (the safety escape, SAFETY-01/02).
    pub fn single(pattern: &str, profile: &Profile) -> Option<Self> {
        let binding = Binding {
            when: pattern.to_owned(),
            context: None,
            do_: None,
            start: None,
            repeat: None,
            stop: None,
            behavior: None,
            passthrough: None,
        };
        let m = Self::from_bindings(
            [(
                BindingKey {
                    scope: Scope::Profile,
                    index: 0,
                },
                binding,
            )],
            profile,
        );
        (!m.compiled.is_empty()).then_some(m)
    }

    fn from_bindings(
        bindings: impl IntoIterator<Item = (BindingKey, Binding)>,
        profile: &Profile,
    ) -> Self {
        let compiled = bindings
            .into_iter()
            .filter_map(|(key, binding)| {
                let pattern = Pattern::parse(&binding.when).ok()?;
                let terms = pattern
                    .terms
                    .iter()
                    .map(|t| normalize(t, profile))
                    .collect::<Option<Vec<_>>>()?;
                Some(Compiled {
                    key,
                    binding,
                    terms,
                })
            })
            .collect();
        Self {
            compiled,
            states: BTreeMap::new(),
            pressed: BTreeSet::new(),
            open: Vec::new(),
        }
    }

    pub fn binding(&self, key: &BindingKey) -> Option<&Binding> {
        self.compiled
            .iter()
            .find(|c| c.key == *key)
            .map(|c| &c.binding)
    }

    fn applies(c: &Compiled, ctx: Option<&MatchContext>) -> bool {
        let Some(ctx) = ctx else { return true };
        let scoped = match &c.key.scope {
            Scope::Profile => true,
            Scope::Board { board } => *board == ctx.top_board,
            Scope::Layer { board, layer } => {
                *board == ctx.top_board && ctx.visible_layers.contains(layer)
            }
            Scope::Item { board, layer, item } => {
                ctx.hovered.as_ref() == Some(&(board.clone(), layer.clone(), item.clone()))
            }
        };
        let Some(f) = &c.binding.context else {
            return scoped;
        };
        scoped
            && f.mode.is_none_or(|m| m == ctx.mode)
            && f.board.as_ref().is_none_or(|b| b.0 == ctx.top_board)
            && f.layer
                .as_ref()
                .is_none_or(|l| ctx.visible_layers.contains(&l.0))
            && f.latched.is_none_or(|k| ctx.latched.contains(&k))
            && f.app
                .as_ref()
                .is_none_or(|a| ctx.app.as_deref() == Some(a.as_str()))
            && f.action_set
                .as_ref()
                .is_none_or(|s| ctx.action_sets.contains(&s.0))
    }

    /// Priority key: scope, then position of the `action_set` filter in the stack (higher first), then order.
    fn priority(c: &Compiled, ctx: Option<&MatchContext>) -> (u8, usize, usize) {
        let set_rank = c
            .binding
            .context
            .as_ref()
            .and_then(|f| f.action_set.as_ref())
            .and_then(|s| ctx.and_then(|ctx| ctx.action_sets.iter().position(|x| *x == s.0)))
            .map_or(usize::MAX, |pos| usize::MAX - 1 - pos);
        (c.key.scope.rank(), set_rank, c.key.index)
    }

    /// Picks, per pattern meaning, the highest-priority binding (04).
    fn fire(
        &mut self,
        fired: Vec<(usize, usize, Ends)>,
        at: Instant,
        ctx: Option<&MatchContext>,
    ) -> Vec<MappingEvent> {
        let mut best: BTreeMap<NTerm, (usize, Ends)> = BTreeMap::new();
        for (ci, ti, ends) in fired {
            let term = self.compiled[ci].terms[ti].clone();
            let better = match best.get(&term) {
                None => true,
                Some((other, _)) => {
                    Self::priority(&self.compiled[ci], ctx)
                        < Self::priority(&self.compiled[*other], ctx)
                }
            };
            if better {
                best.insert(term, (ci, ends));
            }
        }
        let mut chosen: Vec<(usize, Ends)> = best.into_values().collect();
        chosen.sort_by_key(|(ci, _)| Self::priority(&self.compiled[*ci], ctx));
        chosen
            .into_iter()
            .map(|(ci, ends)| {
                let binding = self.compiled[ci].key.clone();
                if let Ends::OnRelease(triggers) = &ends {
                    self.open.push((binding.clone(), triggers.clone()));
                }
                MappingEvent::Start { binding, at, ends }
            })
            .collect()
    }

    pub fn on_trigger(
        &mut self,
        ev: &TriggerEvent,
        ctx: Option<&MatchContext>,
    ) -> Vec<MappingEvent> {
        let mut out = self.on_tick(ev.t, ctx);
        let mut fired = Vec::new();
        match ev.phase {
            Phase::Down => {
                self.pressed.insert(ev.trigger.clone());
            }
            Phase::Up => {
                self.pressed.remove(&ev.trigger);
                let (ended, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.open)
                    .into_iter()
                    .partition(|(_, ts)| ts.contains(&ev.trigger));
                self.open = kept;
                out.extend(
                    ended
                        .into_iter()
                        .map(|(binding, _)| MappingEvent::End { binding, at: ev.t }),
                );
            }
        }
        for (ci, c) in self.compiled.iter().enumerate() {
            let applies = Self::applies(c, ctx);
            for (ti, term) in c.terms.iter().enumerate() {
                let st = self.states.entry((ci, ti)).or_default();
                if let Some(ends) = step_trigger(term, st, ev, &self.pressed) {
                    if applies {
                        fired.push((ci, ti, ends));
                    }
                }
            }
        }
        out.extend(self.fire(fired, ev.t, ctx));
        out
    }

    pub fn on_gesture(
        &mut self,
        ev: &GestureEvent,
        ctx: Option<&MatchContext>,
    ) -> Vec<MappingEvent> {
        self.on_named(&NInput::Gesture(ev.gesture.to_string()), ev.t, ctx)
    }

    /// A dwell completed on some item (04 `dwell` input).
    pub fn on_dwell(&mut self, at: Instant, ctx: Option<&MatchContext>) -> Vec<MappingEvent> {
        self.on_named(&NInput::Dwell, at, ctx)
    }

    fn on_named(
        &mut self,
        input: &NInput,
        at: Instant,
        ctx: Option<&MatchContext>,
    ) -> Vec<MappingEvent> {
        let mut out = self.on_tick(at, ctx);
        let mut fired = Vec::new();
        for (ci, c) in self.compiled.iter().enumerate() {
            let applies = Self::applies(c, ctx);
            for (ti, term) in c.terms.iter().enumerate() {
                let st = self.states.entry((ci, ti)).or_default();
                let hit = match term {
                    NTerm::Input(i, NModifier::Press) => i == input,
                    NTerm::Sequence(inputs, within) => {
                        advance_sequence(st, inputs, *within, at, |i| i == input)
                    }
                    _ => false,
                };
                if hit && applies {
                    fired.push((ci, ti, Ends::Instant));
                }
            }
        }
        out.extend(self.fire(fired, at, ctx));
        out
    }

    /// Fires holds whose time has come.
    pub fn on_tick(&mut self, now: Instant, ctx: Option<&MatchContext>) -> Vec<MappingEvent> {
        let mut fired = Vec::new();
        let mut at = now;
        for (ci, c) in self.compiled.iter().enumerate() {
            for (ti, term) in c.terms.iter().enumerate() {
                if let NTerm::Input(_, NModifier::Hold(hold)) = term {
                    let st = self.states.entry((ci, ti)).or_default();
                    if let Some((trigger, since)) = st.hold_since.clone() {
                        let due = since + ms(*hold);
                        if due <= now {
                            st.hold_since = None;
                            if Self::applies(c, ctx) {
                                at = at.min(due);
                                fired.push((ci, ti, Ends::OnRelease(vec![trigger])));
                            }
                        }
                    }
                }
            }
        }
        self.fire(fired, at, ctx)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.states
            .iter()
            .filter_map(
                |((ci, ti), st)| match (&self.compiled[*ci].terms[*ti], &st.hold_since) {
                    (NTerm::Input(_, NModifier::Hold(h)), Some((_, since))) => {
                        Some(*since + ms(*h))
                    }
                    _ => None,
                },
            )
            .min()
    }

    /// Forgets held inputs, pending holds and open activations (pause, Safe Board, SAFETY-08).
    pub fn reset(&mut self) {
        self.states.clear();
        self.pressed.clear();
        self.open.clear();
    }
}

/// Advances a sequence; true when it completes.
fn advance_sequence(
    st: &mut TermState,
    inputs: &[NInput],
    within: Option<u32>,
    at: Instant,
    is: impl Fn(&NInput) -> bool,
) -> bool {
    let expected = st.seq.map_or(0, |(i, _)| i);
    if inputs.get(expected).is_some_and(&is) && expected > 0 {
        let (_, start) = st.seq.unwrap_or((0, at));
        if within.is_some_and(|w| at.duration_since(start) > ms(w)) {
            st.seq = None;
        } else if expected + 1 == inputs.len() {
            st.seq = None;
            return true;
        } else {
            st.seq = Some((expected + 1, start));
            return false;
        }
    }
    // The first input (re)starts the sequence; anything else is ignored (04).
    if inputs.first().is_some_and(&is) {
        if inputs.len() == 1 {
            st.seq = None;
            return true;
        }
        st.seq = Some((1, at));
    }
    false
}

fn step_trigger(
    term: &NTerm,
    st: &mut TermState,
    ev: &TriggerEvent,
    pressed: &BTreeSet<TriggerId>,
) -> Option<Ends> {
    let t = &ev.trigger;
    match (term, ev.phase) {
        (NTerm::Input(i, NModifier::Press), Phase::Down) if i.matches_trigger(t) => {
            Some(Ends::OnRelease(vec![t.clone()]))
        }
        (NTerm::Input(i, NModifier::Release), Phase::Up) if i.matches_trigger(t) => {
            Some(Ends::Instant)
        }
        (NTerm::Input(i, NModifier::Hold(_)), phase) if i.matches_trigger(t) => {
            match phase {
                Phase::Down => st.hold_since = Some((t.clone(), ev.t)),
                Phase::Up => {
                    if st.hold_since.as_ref().is_some_and(|(h, _)| h == t) {
                        st.hold_since = None;
                    }
                }
            }
            None
        }
        (NTerm::Input(i, NModifier::Double(window)), Phase::Down) if i.matches_trigger(t) => {
            let double = st
                .last_down
                .is_some_and(|last| ev.t.duration_since(last) <= ms(*window));
            st.last_down = if double { None } else { Some(ev.t) };
            double.then_some(Ends::Instant)
        }
        (NTerm::Sequence(inputs, within), Phase::Down) => {
            advance_sequence(st, inputs, *within, ev.t, |i| i.matches_trigger(t))
                .then_some(Ends::Instant)
        }
        (NTerm::Chord(inputs), Phase::Down) => {
            let part = inputs.iter().any(|i| i.matches_trigger(t));
            let all_held = inputs
                .iter()
                .all(|i| pressed.iter().any(|p| i.matches_trigger(p)));
            (part && all_held).then(|| {
                let held = pressed
                    .iter()
                    .filter(|p| inputs.iter().any(|i| i.matches_trigger(p)))
                    .cloned()
                    .collect();
                Ends::OnRelease(held)
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
