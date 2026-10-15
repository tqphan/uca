// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prediction worker tests (16 "Acceptance tests": prediction).

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::*;

fn settings(learning: bool, store: Option<PathBuf>) -> PredictSettings {
    PredictSettings {
        language: Some("en-US".into()),
        learning,
        weights: [0.6, 0.3, 0.1],
        store,
    }
}

/// A fresh directory for one test's user store.
fn store_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("uca-predict-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Spawns a predictor whose answers arrive on the returned channel.
fn spawn(s: PredictSettings) -> (Predictor, mpsc::Receiver<(u64, Vec<String>)>) {
    let (tx, rx) = mpsc::channel();
    let p = Predictor::spawn(
        s,
        Box::new(move |rev, words| {
            let _ = tx.send((rev, words));
        }),
    );
    (p, rx)
}

fn ask(p: &Predictor, rx: &mpsc::Receiver<(u64, Vec<String>)>, before: &str) -> Vec<String> {
    p.predict(7, before, 6);
    let (rev, words) = rx.recv_timeout(Duration::from_secs(10)).expect("answer");
    assert_eq!(rev, 7);
    words
}

fn lower(words: &[String]) -> Vec<String> {
    words.iter().map(|w| w.to_lowercase()).collect()
}

#[test]
fn completes_words_from_the_embedded_model() {
    let (p, rx) = spawn(settings(false, None));
    let words = ask(&p, &rx, "I need hel");
    assert!(lower(&words).contains(&"help".to_owned()), "{words:?}");
    assert!(
        words.iter().all(|w| w.to_lowercase().starts_with("hel")),
        "{words:?}"
    );
    assert_eq!(p.status(), PredictStatus::Ready { pack: "en-US" });
    // A sentence start is capitalized (augur's casing).
    let words = ask(&p, &rx, "hel");
    assert!(!words.is_empty());
    assert!(words.iter().all(|w| w.starts_with("Hel")), "{words:?}");
    // The en-US model knows word pairs (16 "Next-word pairs").
    let words = ask(&p, &rx, "He was born in Lon");
    assert_eq!(
        words.first().map(String::as_str),
        Some("London"),
        "{words:?}"
    );
}

/// Only the newest of several queued asks is answered.
#[test]
fn a_burst_of_asks_gets_one_answer() {
    let (tx, rx) = mpsc::channel();
    let (answers_tx, answers) = mpsc::channel();
    let worker = Worker {
        settings: settings(false, None),
        engine: None,
        status: Arc::new(Mutex::new(PredictStatus::Starting)),
        answer: Box::new(move |rev, words| {
            let _ = answers_tx.send((rev, words));
        }),
    };
    for (rev, before) in [(1, "h"), (2, "he"), (3, "hel")] {
        let _ = tx.send(Command::Predict {
            rev,
            before: before.into(),
            max: 3,
        });
    }
    let _ = tx.send(Command::Quit);
    worker.run(rx);
    let got: Vec<u64> = answers.try_iter().map(|(rev, _)| rev).collect();
    assert_eq!(got, [3]);
}

#[test]
fn learns_only_while_learning_is_on() {
    let (p, rx) = spawn(settings(false, None));
    p.learn("We met at Zubrowkaville yesterday. ");
    assert!(!lower(&ask(&p, &rx, "zubr")).contains(&"zubrowkaville".to_owned()));

    p.set_settings(settings(true, None));
    p.learn("We met at Zubrowkaville yesterday. ");
    let words = ask(&p, &rx, "zubr");
    assert!(
        lower(&words).contains(&"zubrowkaville".to_owned()),
        "{words:?}"
    );
    // The user's own capitals are kept.
    assert!(words.iter().any(|w| w == "Zubrowkaville"), "{words:?}");
}

#[test]
fn forget_and_reset() {
    let (p, rx) = spawn(settings(true, None));
    p.learn("Zubrowkaville and Qorvexland. ");
    p.forget("Zubrowkaville");
    assert!(!lower(&ask(&p, &rx, "zubr")).contains(&"zubrowkaville".to_owned()));
    assert!(lower(&ask(&p, &rx, "qorv")).contains(&"qorvexland".to_owned()));
    p.reset();
    assert!(!lower(&ask(&p, &rx, "qorv")).contains(&"qorvexland".to_owned()));
}

/// 16: learned words are kept in the profile's store and survive a restart.
#[test]
fn learned_words_are_saved_in_the_store() {
    let dir = store_dir("store");
    let store = dir.join("prediction").join("user.redb");
    {
        let (p, _rx) = spawn(settings(true, Some(store.clone())));
        p.learn("Zubrowkaville is far. ");
        p.end_text();
    }
    assert!(store.exists());
    let (p, rx) = spawn(settings(true, Some(store.clone())));
    assert!(lower(&ask(&p, &rx, "zubr")).contains(&"zubrowkaville".to_owned()));
    drop(p);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store that cannot be opened degrades to learning in memory (rule 5).
#[test]
fn an_unusable_store_learns_in_memory() {
    let dir = store_dir("bad");
    std::fs::create_dir_all(&dir).expect("dir");
    // A directory where the store file should be.
    let store = dir.join("user.redb");
    std::fs::create_dir_all(&store).expect("dir");
    let (p, rx) = spawn(settings(true, Some(store)));
    p.learn("Zubrowkaville. ");
    assert!(lower(&ask(&p, &rx, "zubr")).contains(&"zubrowkaville".to_owned()));
    assert_eq!(p.status(), PredictStatus::StoreFailed { pack: "en-US" });
    drop(p);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 16 "Performance": suggestions within 30 ms on the 50,000-word model.
/// Debug builds are slower, so the limit is checked in release builds only.
#[test]
fn predictions_are_fast() {
    let (p, rx) = spawn(settings(true, None));
    p.learn("I want to go to the shop and then I want to go home. ");
    // The worker loads the model when it starts, before the user types:
    // wait for that, so only predictions are timed.
    let loading = Instant::now();
    let _ = ask(&p, &rx, "x");
    eprintln!(
        "first answer (includes loading the model): {:?}",
        loading.elapsed()
    );
    let texts = [
        "",
        "I",
        "I w",
        "I want to ",
        "the sho",
        "and then I want to go h",
        "s",
    ];
    let started = Instant::now();
    let mut slowest = Duration::ZERO;
    for t in texts {
        let one = Instant::now();
        let _ = ask(&p, &rx, t);
        eprintln!("{t:?}: {:?}", one.elapsed());
        slowest = slowest.max(one.elapsed());
    }
    eprintln!(
        "slowest prediction: {slowest:?} (all: {:?})",
        started.elapsed()
    );
    if !cfg!(debug_assertions) {
        assert!(slowest < Duration::from_millis(30), "{slowest:?}");
    }
}
