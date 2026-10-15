// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Built-in word prediction (16): the prediction worker around augur.
//!
//! The engine asks; this thread answers (01 "Threads"). When several asks
//! are queued only the newest is answered, so a burst of keystrokes costs
//! one prediction. Learning, forgetting and reset run in order.
//!
//! Everything sent here is typed text: it is logged only as `Sensitive` (11).

mod model;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use augur::{
    Context, EngineBuilder, EnglishPack, LinearCombiner, Model, NgramPredictor, RecencyConfig,
    RecencyPredictor, UserConfig, UserPredictor,
};
use uca_config::profile::Profile;
use uca_log::Sensitive;

/// What the worker needs from the profile (16).
#[derive(Debug, Clone, PartialEq)]
pub struct PredictSettings {
    /// The profile's communication language (BCP 47); `None`: `en-US`.
    pub language: Option<String>,
    pub learning: bool,
    /// `[model, user, recent]`.
    pub weights: [f32; 3],
    /// `profiles/<id>/prediction/user.redb`; `None`: learn in memory only.
    pub store: Option<PathBuf>,
}

impl PredictSettings {
    pub fn from_profile(profile: &Profile, store: Option<PathBuf>) -> Self {
        let p = &profile.prediction;
        Self {
            language: profile.language.clone(),
            learning: p.learning,
            weights: p.weights.resolved().map(|w| w as f32),
            store,
        }
    }

    fn engine_differs(&self, other: &Self) -> bool {
        self.language != other.language
            || self.weights != other.weights
            || self.store != other.store
    }
}

/// What Settings shows about prediction (16 "Model lookup").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredictStatus {
    Starting,
    /// Predicting with the model of this language pack.
    Ready {
        pack: &'static str,
    },
    /// The learned words could not be opened: learning stays in memory and is
    /// lost when UCA closes.
    StoreFailed {
        pack: &'static str,
    },
    /// No usable model: the slots stay blank.
    NoModel,
}

/// The worker's answer to `predict`: the revision and the words, best first.
pub type Answer = Box<dyn FnMut(u64, Vec<String>) + Send>;

enum Command {
    Predict { rev: u64, before: String, max: u32 },
    Learn(String),
    EndText,
    Forget(String),
    Reset,
    Settings(PredictSettings),
    Quit,
}

/// The handle the app keeps. Dropping it saves what was learned and ends the
/// thread.
pub struct Predictor {
    tx: Sender<Command>,
    status: Arc<Mutex<PredictStatus>>,
    thread: Option<JoinHandle<()>>,
}

impl Predictor {
    pub fn spawn(settings: PredictSettings, answer: Answer) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(PredictStatus::Starting));
        let worker = Worker {
            settings,
            engine: None,
            status: status.clone(),
            answer,
        };
        let thread = match std::thread::Builder::new()
            .name("prediction".into())
            .spawn(move || worker.run(rx))
        {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::error!(target: "uca::predict", error = %e, "prediction thread failed to start");
                *lock(&status) = PredictStatus::NoModel;
                None
            }
        };
        Self { tx, status, thread }
    }

    /// Up to `max` suggestions for the compose text `before`.
    pub fn predict(&self, rev: u64, before: &str, max: u32) {
        self.send(Command::Predict {
            rev,
            before: before.to_owned(),
            max,
        });
    }

    /// Learns `text`, continuing the text learned before.
    pub fn learn(&self, text: &str) {
        self.send(Command::Learn(text.to_owned()));
    }

    /// The text ended; what is learned next starts a new one.
    pub fn end_text(&self) {
        self.send(Command::EndText);
    }

    /// Removes a word from suggestions, saved data included (16).
    pub fn forget(&self, word: &str) {
        self.send(Command::Forget(word.to_owned()));
    }

    /// Settings "Reset learning": forgets everything learned (16).
    pub fn reset(&self) {
        self.send(Command::Reset);
    }

    pub fn set_settings(&self, settings: PredictSettings) {
        self.send(Command::Settings(settings));
    }

    pub fn status(&self) -> PredictStatus {
        lock(&self.status).clone()
    }

    fn send(&self, c: Command) {
        if self.tx.send(c).is_err() {
            tracing::error!(target: "uca::predict", "prediction thread is gone");
        }
    }
}

impl Drop for Predictor {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Worker {
    settings: PredictSettings,
    engine: Option<augur::Engine>,
    status: Arc<Mutex<PredictStatus>>,
    answer: Answer,
}

impl Worker {
    fn run(mut self, rx: Receiver<Command>) {
        self.build();
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            batch.extend(rx.try_iter());
            // Only the newest ask is answered; the engine drops older revisions anyway.
            let newest = batch
                .iter()
                .rposition(|c| matches!(c, Command::Predict { .. }));
            for (i, c) in batch.into_iter().enumerate() {
                match c {
                    Command::Predict { rev, before, max } if Some(i) == newest => {
                        self.predict(rev, &before, max);
                    }
                    Command::Predict { .. } => {}
                    Command::Learn(text) => {
                        if let Some(e) = &self.engine {
                            tracing::trace!(target: "uca::predict", text = %Sensitive(&text), "learn");
                            e.learn(&text);
                        }
                    }
                    Command::EndText => {
                        if let Some(e) = &self.engine {
                            e.reset_learning_context();
                            if let Err(err) = e.flush() {
                                tracing::warn!(target: "uca::predict", error = %err, "learned words not saved");
                            }
                        }
                    }
                    Command::Forget(word) => {
                        if let Some(e) = &self.engine {
                            tracing::debug!(target: "uca::predict", word = %Sensitive(&word), "forget");
                            if let Err(err) = e.forget(&word) {
                                tracing::warn!(target: "uca::predict", error = %err, "forgotten word not saved");
                            }
                        }
                    }
                    Command::Reset => {
                        if let Some(e) = &self.engine {
                            tracing::debug!(target: "uca::predict", "reset learning");
                            if let Err(err) = e.clear() {
                                tracing::warn!(target: "uca::predict", error = %err, "reset not saved");
                            }
                        }
                    }
                    Command::Settings(s) => {
                        let rebuild = s.engine_differs(&self.settings);
                        self.settings = s;
                        if rebuild {
                            self.build();
                        } else if let Some(e) = &self.engine {
                            e.set_learning(self.settings.learning);
                        }
                    }
                    Command::Quit => return,
                }
            }
        }
    }

    fn predict(&mut self, rev: u64, before: &str, max: u32) {
        let words = match &self.engine {
            Some(e) => {
                let started = std::time::Instant::now();
                let max = usize::try_from(max).unwrap_or(usize::MAX);
                let words: Vec<String> = e
                    .predict(Context::past(before), max)
                    .into_iter()
                    .map(|s| s.text)
                    .collect();
                tracing::trace!(
                    target: "uca::predict",
                    rev,
                    words = words.len(),
                    micros = started.elapsed().as_micros() as u64,
                    "predicted"
                );
                words
            }
            None => Vec::new(),
        };
        (self.answer)(rev, words);
    }

    /// (Re)builds the engine. The old one is dropped first: it saves what it
    /// learned and frees the store, which only one engine may hold.
    fn build(&mut self) {
        if let Some(old) = self.engine.take() {
            if let Err(e) = old.flush() {
                tracing::warn!(target: "uca::predict", error = %e, "learned words not saved");
            }
        }
        let (engine, status) = build_engine(&self.settings);
        tracing::debug!(target: "uca::predict", status = ?status, "prediction engine");
        self.engine = engine;
        *lock(&self.status) = status;
    }
}

/// augur's default engine (base model, user model, recent words) with the
/// embedded model and UCA's settings.
fn build_engine(s: &PredictSettings) -> (Option<augur::Engine>, PredictStatus) {
    let Some((pack, bytes)) = model::model_for(s.language.as_deref()) else {
        return (None, PredictStatus::NoModel);
    };
    let model = match Model::from_bytes(bytes.to_vec()) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(target: "uca::predict", pack, error = %e, "invalid prediction model");
            return (None, PredictStatus::NoModel);
        }
    };
    // augur's user store is tied to the language it learned.
    let language = pack.split('-').next().unwrap_or(pack).to_ascii_lowercase();
    if language != "en" {
        // augur's only language pack so far is English.
        tracing::error!(target: "uca::predict", pack, "augur has no language pack for this model");
        return (None, PredictStatus::NoModel);
    }
    let mut status = PredictStatus::Ready { pack };
    let user = match &s.store {
        Some(path) => {
            let opened = match path.parent() {
                Some(dir) => std::fs::create_dir_all(dir).map_err(|e| e.to_string()),
                None => Ok(()),
            }
            .and_then(|()| {
                UserPredictor::open(path, &language, UserConfig::default())
                    .map_err(|e| e.to_string())
            });
            match opened {
                Ok(u) => u,
                Err(e) => {
                    tracing::error!(target: "uca::predict", error = %e, "learned words cannot be opened; learning in memory only");
                    status = PredictStatus::StoreFailed { pack };
                    UserPredictor::in_memory(UserConfig::default())
                }
            }
        }
        None => UserPredictor::in_memory(UserConfig::default()),
    };
    let [model_w, user_w, recent_w] = s.weights;
    // Predictor order matches the combiner weights.
    let built = EngineBuilder::default()
        .language(EnglishPack::new())
        .predictor(user)
        .predictor(NgramPredictor::new(model))
        .predictor(RecencyPredictor::new(RecencyConfig::default()))
        .combiner(LinearCombiner::with_weights([user_w, model_w, recent_w]))
        .build();
    match built {
        Ok(engine) => {
            engine.set_learning(s.learning);
            (Some(engine), status)
        }
        Err(e) => {
            tracing::error!(target: "uca::predict", error = %e, "prediction engine failed");
            (None, PredictStatus::NoModel)
        }
    }
}

#[cfg(test)]
mod tests;
