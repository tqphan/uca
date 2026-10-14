// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine and executor threads (01 "Threads").

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use uca_actions::{Executor, SoftwareOutput};
use uca_app::engine_core::{CoreInput, EngineCore, Queries};
use uca_app::recording::{config_hash, Entry, Header, Recorder, FORMAT};
use uca_config::store::{ConfigUpdate, LoadedConfig};
use uca_engine::{
    Control, Engine, EngineControl, EngineSnapshot, Intent, KeyChars, KeyLayout, SpeechIntent,
    VirtualBoard,
};
use uca_input::{SignalSender, SourceStats, StatsCollector};
use uca_platform::sources::ConsumeSet;
use uca_platform::{Modifiers, Platform};
use uca_types::{Clock, Key, MonitorInfo, PhysPoint, Signal, SourceId, SystemClock};
use uca_voice::{Voice, VoiceSettings};

use crate::notices::SpeechNotice;

/// Everything that reaches the engine thread (01: one channel).
#[derive(Debug)]
pub enum Msg {
    Signal(Signal),
    Config(ConfigUpdate),
    Monitors(Vec<MonitorInfo>),
    Control(EngineControl),
    /// A `uca-ui` screen's widgets by board id (Settings, notices), or
    /// `None` when it closes (05, 11).
    Virtual(String, Option<VirtualBoard>),
    /// Start recording into the file, or stop with `None` (11).
    Record(Option<std::path::PathBuf>),
    /// The foreground keyboard layout changed (07): rebuild the compose table (13).
    KeyLayoutChanged,
    Quit,
}

/// What the UI reads: the latest snapshot and config.
#[derive(Default)]
pub struct Shared {
    pub snapshot: Option<EngineSnapshot>,
    pub config: Option<Arc<LoadedConfig>>,
    pub debug: DebugInfo,
}

/// What the debug overlay shows from the input side (11).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DebugInfo {
    /// The pointer source's last raw sample, before the PointPipeline.
    pub raw: Option<PhysPoint>,
    /// The last one-second window's per-source stats.
    pub sources: Vec<SourceStats>,
}

/// Wakes the UI thread when the snapshot changes or the app must act.
pub trait UiWaker: Send + 'static {
    fn snapshot(&self);
    fn control(&self, c: Control);
    /// A virtual board item was activated (05 "Settings window").
    fn virtual_item(&self, board: String, item: String);
    /// Nothing was spoken, or speech fell back (13).
    fn speech_notice(&self, n: SpeechNotice);
}

/// What the engine thread talks to.
pub struct EngineLinks {
    pub platform: Arc<dyn Platform>,
    pub rx: Receiver<Msg>,
    pub intents: Sender<Intent>,
    pub shared: Arc<Mutex<Shared>>,
    pub consume: ConsumeSet,
    /// Each source's sender, to read its dropped samples (01, 11).
    pub senders: Vec<(SourceId, SignalSender)>,
    /// The voice worker (13).
    pub voice: Arc<Voice>,
}

/// The engine loop (01): signals, config, controls and timers in one place.
pub fn run_engine(
    config: Arc<LoadedConfig>,
    monitors: Vec<MonitorInfo>,
    links: EngineLinks,
    waker: impl UiWaker,
) {
    let EngineLinks {
        platform,
        rx,
        intents,
        shared,
        consume,
        senders,
        voice,
    } = links;
    let clock = SystemClock;
    let mut current = config.clone();
    let mut core = EngineCore::new(config.clone(), monitors);
    set_consume(&consume, &config);
    let mut stats = StatsCollector::default();
    let mut debug = DebugInfo::default();
    let mut recorder: Option<Recorder> = None;
    core.handle(
        CoreInput::KeyLayout(key_layout(platform.as_ref())),
        &clock,
        &mut Live {
            platform: platform.as_ref(),
            recorder: &mut recorder,
            now: clock.now(),
        },
    );
    let mut frame = publish(
        core.engine(),
        &shared,
        Some(config),
        &clock,
        &waker,
        core.monitors(),
        &debug,
    );
    loop {
        let deadline = [core.next_deadline(), frame, stats.next_deadline()]
            .into_iter()
            .flatten()
            .min();
        let msg = match deadline {
            Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let mut new_config = None;
        let input = match msg {
            Ok(Msg::Signal(s)) => {
                if let Signal::Point(p) = &s {
                    stats.on_point(p, clock.now());
                    if core.pointer_source() == Some(&p.source) {
                        debug.raw = Some(p.pos);
                    }
                }
                Some((CoreInput::Signal(s.clone()), Entry::Signal(s)))
            }
            Ok(Msg::Config(ConfigUpdate::Applied(c))) => {
                set_consume(&consume, &c);
                voice.set_settings(VoiceSettings::from_profile(&c.profile.profile.voice));
                current = c.clone();
                new_config = Some(c.clone());
                let hash = if recorder.is_some() {
                    config_hash(&c)
                } else {
                    String::new()
                };
                Some((CoreInput::Config(c), Entry::Config { hash }))
            }
            Ok(Msg::Config(ConfigUpdate::Rejected(issues))) => {
                // Last-known-good stays live (02); the store already logged each issue.
                tracing::warn!(target: "uca::app", count = issues.len(), "config change rejected");
                None
            }
            Ok(Msg::Monitors(m)) => Some((CoreInput::Monitors(m.clone()), Entry::Monitors(m))),
            Ok(Msg::Control(c)) => Some((CoreInput::Control(c), Entry::Control(c))),
            Ok(Msg::Virtual(id, board)) => Some((
                CoreInput::Virtual(id.clone(), board.clone()),
                Entry::Virtual { id, board },
            )),
            Ok(Msg::Record(target)) => {
                recorder = target.and_then(|path| start_recording(&path, &core, &current));
                match &mut recorder {
                    // Compose characters depend on it, so a replay needs it first (13).
                    Some(r) => r.record(clock.now(), Entry::KeyLayout(core.key_layout().clone())),
                    None => tracing::info!(target: "uca::app", "recording stopped"),
                }
                None
            }
            Ok(Msg::KeyLayoutChanged) => {
                let l = key_layout(platform.as_ref());
                Some((CoreInput::KeyLayout(l.clone()), Entry::KeyLayout(l)))
            }
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => Some((CoreInput::Tick, Entry::Tick)),
        };
        if let Some((input, entry)) = input {
            let now = clock.now();
            if let Some(r) = &mut recorder {
                r.record(now, entry);
            }
            let mut q = Live {
                platform: platform.as_ref(),
                recorder: &mut recorder,
                now,
            };
            core.handle(input, &clock, &mut q);
        }
        for intent in core.drain_intents() {
            match intent {
                Intent::Control(Control::RefreshKeyLayout) => {
                    let l = key_layout(platform.as_ref());
                    let now = clock.now();
                    if let Some(r) = &mut recorder {
                        r.record(now, Entry::KeyLayout(l.clone()));
                    }
                    let mut q = Live {
                        platform: platform.as_ref(),
                        recorder: &mut recorder,
                        now,
                    };
                    core.handle(CoreInput::KeyLayout(l), &clock, &mut q);
                }
                Intent::Control(c) => waker.control(c),
                Intent::Virtual { board, item } => waker.virtual_item(board, item),
                Intent::Speech(s) => match s {
                    SpeechIntent::Say(text) => voice.say(&text),
                    SpeechIntent::Stop => voice.stop(),
                    SpeechIntent::Voice(id) => voice.set_voice(&id),
                    SpeechIntent::TurnedOff => waker.speech_notice(SpeechNotice::TurnedOff),
                    SpeechIntent::TooLong { max } => {
                        waker.speech_notice(SpeechNotice::TooLong { max })
                    }
                },
                intent => {
                    if intents.send(intent).is_err() {
                        return;
                    }
                }
            }
        }
        source_stats(&mut stats, &senders, &mut debug, clock.now());
        frame = publish(
            core.engine(),
            &shared,
            new_config,
            &clock,
            &waker,
            core.monitors(),
            &debug,
        );
    }
    // SAFETY-08 on exit: the executor releases everything when this sender drops.
    let _ = intents.send(Intent::ReleaseAll);
}

/// Asks the platform and records each answer while recording (11).
struct Live<'a> {
    platform: &'a dyn Platform,
    recorder: &'a mut Option<Recorder>,
    now: Instant,
}

impl Queries for Live<'_> {
    fn covered(&mut self, p: PhysPoint) -> bool {
        let covered = self.platform.overlay_covered_at(p);
        if let Some(r) = self.recorder.as_mut() {
            r.record(self.now, Entry::Covered(covered));
        }
        covered
    }

    fn caps_lock(&mut self) -> bool {
        let on = self.platform.caps_lock();
        if let Some(r) = self.recorder.as_mut() {
            r.record(self.now, Entry::CapsLock(on));
        }
        on
    }
}

/// What each typing key types under the foreground layout (13 "Layout").
pub fn key_layout(platform: &dyn Platform) -> KeyLayout {
    let label = |k: Key, shift: bool, caps_lock: bool| {
        platform.key_label(k, Modifiers { shift, caps_lock })
    };
    let keys = Key::ALL
        .iter()
        .filter(|k| k.types_text())
        .map(|&k| {
            let chars = KeyChars {
                plain: label(k, false, false),
                shift: label(k, true, false),
                caps: label(k, false, true),
                shift_caps: label(k, true, true),
            };
            (k, chars)
        })
        .filter(|(_, c)| *c != KeyChars::default())
        .collect();
    KeyLayout { keys }
}

/// Opens `path` and starts a recorder whose lines a writer thread writes, so
/// the engine thread never waits on the disk (11).
fn start_recording(
    path: &std::path::Path,
    core: &EngineCore,
    config: &LoadedConfig,
) -> Option<Recorder> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = match std::fs::File::create(path) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(target: "uca::app", file = %path.display(), error = %e, "cannot start a recording");
            return None;
        }
    };
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let spawned = std::thread::Builder::new()
        .name("recorder".into())
        .spawn(move || {
            // Flushed whenever it has caught up, so a crash or a kill keeps
            // what was recorded up to then.
            let mut out = std::io::BufWriter::new(file);
            while let Ok(line) = rx.recv() {
                let mut ok = writeln!(out, "{line}").is_ok();
                while let (true, Ok(more)) = (ok, rx.try_recv()) {
                    ok = writeln!(out, "{more}").is_ok();
                }
                if !ok || out.flush().is_err() {
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(target: "uca::app", error = %e, "cannot start a recording");
        return None;
    }
    let header = Header {
        format: FORMAT,
        uca_version: env!("CARGO_PKG_VERSION").to_owned(),
        started: uca_log::crash::now_rfc3339(),
        profile_id: profile_id(config),
        config_hash: config_hash(config),
        monitors: core.monitors().to_vec(),
    };
    tracing::info!(target: "uca::app", file = %path.display(), "recording started");
    Some(Recorder::start(
        Instant::now(),
        &header,
        Box::new(move |l| {
            let _ = tx.send(l);
        }),
    ))
}

/// The profile's id: the name of the folder `profile.yaml` is in (02).
fn profile_id(config: &LoadedConfig) -> String {
    config
        .profile_file
        .parent()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn set_consume(consume: &ConsumeSet, config: &LoadedConfig) {
    let set = uca_mapping::consumed_triggers(config);
    if let Ok(mut c) = consume.write() {
        *c = set;
    }
}

/// Collects dropped samples and, once a second, the per-source stats: logged
/// at `debug`, drops at `warn` (01, 11), and kept for the debug overlay.
fn source_stats(
    stats: &mut StatsCollector,
    senders: &[(SourceId, SignalSender)],
    debug: &mut DebugInfo,
    now: Instant,
) {
    for (id, sender) in senders {
        stats.add_dropped(id, sender.take_dropped(), now);
    }
    let Some(window) = stats.on_tick(now) else {
        return;
    };
    for s in &window {
        tracing::debug!(
            target: "uca::input",
            source = %s.source.0,
            rate_hz = s.rate_hz,
            dropped = s.dropped,
            mean_confidence = s.mean_confidence,
            presence_ratio = s.presence_ratio,
            "source stats"
        );
        if s.dropped > 0 {
            tracing::warn!(target: "uca::input", source = %s.source.0, dropped = s.dropped, "point samples dropped: the engine channel was full");
        }
    }
    debug.sources = window;
}

fn publish(
    engine: &Engine,
    shared: &Mutex<Shared>,
    config: Option<Arc<LoadedConfig>>,
    clock: &dyn Clock,
    waker: &impl UiWaker,
    monitors: &[MonitorInfo],
    debug: &DebugInfo,
) -> Option<Instant> {
    let snap = engine.snapshot(clock);
    let next_frame = frame_deadline(&snap, monitors, clock.now());
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    let changed = s.snapshot.as_ref() != Some(&snap) || config.is_some() || s.debug != *debug;
    if s.debug != *debug {
        s.debug = debug.clone();
    }
    if config.is_some() {
        s.config = config;
    }
    if changed {
        s.snapshot = Some(snap);
        drop(s);
        waker.snapshot();
    }
    next_frame
}

/// While a dwell indicator moves, the snapshot is republished once per frame of
/// the monitor it is on (05 "Frame scheduling").
fn frame_deadline(
    snap: &EngineSnapshot,
    monitors: &[MonitorInfo],
    now: Instant,
) -> Option<Instant> {
    let on_board = snap.board.as_ref().and_then(|b| {
        b.layers
            .iter()
            .flat_map(|l| &l.items)
            .any(|i| i.dwell_progress.is_some())
            .then_some(b.monitor)
    });
    let on_virtual = snap.virtual_boards.iter().find_map(|v| {
        v.items
            .iter()
            .any(|i| i.dwell_progress.is_some())
            .then_some(v.monitor)
    });
    let monitor = on_board.or(on_virtual)?;
    let hz = monitors.iter().find(|m| m.id == monitor)?.refresh_hz;
    (hz > 0.0).then(|| now + Duration::from_secs_f32(1.0 / hz))
}

/// The executor loop: device actions in order, releases on exit (SAFETY-08).
/// `debug.*` actions go to `app_action` (11).
pub fn run_executor(
    platform: Arc<dyn Platform>,
    rx: Receiver<Intent>,
    app_action: impl Fn(uca_config::action::Action),
) {
    let mut executor = Executor::new(Box::new(SoftwareOutput::new(platform.clone())), platform);
    for intent in rx {
        for a in executor.handle(&intent) {
            app_action(a);
        }
    }
    executor.release_all();
}

/// The latest consume set, shared with the hooks.
pub fn new_consume_set() -> ConsumeSet {
    Arc::new(RwLock::new(Default::default()))
}
