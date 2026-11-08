//! A small labelled set for meaning-based recall (ledger E-M26), run through the real recall path.
//!
//! `meaning_set_is_honest` always runs: it keeps the set itself trustworthy (every label points at a real note, a "paraphrase"
//! question really shares no word with its note, an "unrelated" question shares none with any note). `meaning_set_live` needs a
//! running Ollama with `nomic-embed-text` and is run on purpose:
//!
//!     cargo test -p kk-cli --bin kaku meaning_set_live -- --ignored --nocapture

use crate::runner::OllamaEmbedder;
use kk_memory::{Embedder, Memories, NewMemory, Remembered, Source};
use kk_route::OllamaRoute;
use kk_types::label::{Label, Level};
use kk_vault::{Vault, VaultKey};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

const SET: &str = include_str!("../../../fixtures/memory-meaning-set.json");

#[derive(Deserialize)]
struct Set {
    notes: Vec<NoteDef>,
    questions: Vec<QuestionDef>,
}

#[derive(Deserialize)]
struct NoteDef {
    id: String,
    text: String,
}

#[derive(Deserialize)]
struct QuestionDef {
    kind: String,
    question: String,
    expect: Option<String>,
}

fn set() -> Set {
    serde_json::from_str(SET).expect("the labelled set parses")
}

/// Words of four or more letters, lower-cased, without the little words every question has.
fn words(text: &str) -> HashSet<String> {
    const COMMON: &[&str] = &[
        "what", "when", "where", "which", "does", "that", "this", "with", "from", "have", "your",
        "they", "there", "their", "about", "would", "could", "should", "will", "must", "each",
        "every", "than", "then", "into", "over", "only", "also",
    ];
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 4 && !COMMON.contains(&w.as_str()))
        .collect()
}

#[test]
fn meaning_set_is_honest() {
    let s = set();
    assert!(s.notes.len() >= 30 && s.questions.len() >= 25);
    let ids: HashSet<&str> = s.notes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids.len(), s.notes.len(), "note ids are unique");
    let note_words: Vec<(&str, HashSet<String>)> = s
        .notes
        .iter()
        .map(|n| (n.id.as_str(), words(&n.text)))
        .collect();
    let (mut paraphrase, mut keyword, mut unrelated) = (0, 0, 0);
    let mut problems: Vec<String> = Vec::new();
    for q in &s.questions {
        let qw = words(&q.question);
        let sharing: Vec<(&str, Vec<&String>)> = note_words
            .iter()
            .filter_map(|(id, nw)| {
                let shared: Vec<&String> = qw.intersection(nw).collect();
                (!shared.is_empty()).then_some((*id, shared))
            })
            .collect();
        match (q.kind.as_str(), &q.expect) {
            ("paraphrase", Some(want)) => {
                paraphrase += 1;
                if !ids.contains(want.as_str()) {
                    problems.push(format!("'{}' expects a note that is not there", q.question));
                }
                // a paraphrase shares no word with any note: the right note and every other one
                for (id, shared) in &sharing {
                    problems.push(format!(
                        "paraphrase '{}' shares {shared:?} with {id}",
                        q.question
                    ));
                }
            }
            ("keyword", Some(want)) => {
                keyword += 1;
                if !sharing.iter().any(|(id, _)| id == want) {
                    problems.push(format!(
                        "keyword question '{}' shares no word with {want}",
                        q.question
                    ));
                }
            }
            ("unrelated", None) => {
                unrelated += 1;
                for (id, shared) in &sharing {
                    problems.push(format!(
                        "unrelated '{}' shares {shared:?} with {id}",
                        q.question
                    ));
                }
            }
            other => problems.push(format!(
                "a question with a kind or label this set does not use: {other:?}"
            )),
        }
    }
    assert!(
        problems.is_empty(),
        "the labelled set is not honest:\n{}",
        problems.join("\n")
    );
    assert!(
        paraphrase >= 15 && keyword >= 4 && unrelated >= 4,
        "{paraphrase} {keyword} {unrelated}"
    );
}

/// The production embedder with the closeness cut-off set by the test, so the same set can be scored at several cut-offs.
struct WithCutoff {
    inner: Arc<dyn Embedder>,
    cutoff: f32,
}

impl Embedder for WithCutoff {
    fn model(&self) -> String {
        self.inner.model()
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.inner.embed(texts)
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.inner.embed_query(text)
    }
    fn min_similarity(&self) -> f32 {
        self.cutoff
    }
}

struct Outcome {
    cutoff: f32,
    n_par: usize,
    at1: usize,
    at3: usize,
    at5: usize,
    n_kw: usize,
    kw_at1: usize,
    n_un: usize,
    answered: usize,
    misses: Vec<String>,
    noise: Vec<String>,
}

fn score(s: &Set, inner: &Arc<dyn Embedder>, cutoff: f32) -> Outcome {
    let dir = std::env::temp_dir().join(format!(
        "kk-meaning-set-{}-{}",
        std::process::id(),
        (cutoff * 1000.0) as i32
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut mem = Memories::open(
        Vault::create(&dir.join("m.db"), &VaultKey::from_bytes([7; 32])).unwrap(),
        [9; 32],
    )
    .unwrap();
    mem.set_embedder(Arc::new(WithCutoff {
        inner: inner.clone(),
        cutoff,
    }));
    let label = Label::new(Level::Internal);
    let mut note_of: BTreeMap<String, String> = BTreeMap::new();
    for n in &s.notes {
        let id = match mem
            .remember(
                1,
                NewMemory {
                    text: n.text.clone(),
                    label: label.clone(),
                    subject: None,
                    project: None,
                    reason: "a note of the labelled set".into(),
                    source: Source::User,
                    valid_from: None,
                    valid_to: None,
                },
            )
            .unwrap()
        {
            Remembered::Added(i) | Remembered::AlreadyKnown(i) => i,
        };
        note_of.insert(id, n.id.clone());
    }
    let mut o = Outcome {
        cutoff,
        n_par: 0,
        at1: 0,
        at3: 0,
        at5: 0,
        n_kw: 0,
        kw_at1: 0,
        n_un: 0,
        answered: 0,
        misses: vec![],
        noise: vec![],
    };
    for q in &s.questions {
        let got = mem.recall(&q.question, &label, 2, 5).unwrap();
        let ids: Vec<&str> = got.iter().map(|r| note_of[&r.memory.id].as_str()).collect();
        match q.kind.as_str() {
            "paraphrase" => {
                o.n_par += 1;
                let want = q.expect.as_deref().unwrap();
                let rank = ids.iter().position(|i| *i == want);
                o.at1 += usize::from(rank == Some(0));
                o.at3 += usize::from(rank.is_some_and(|r| r < 3));
                o.at5 += usize::from(rank.is_some());
                if rank.is_none() {
                    o.misses
                        .push(format!("{} -> wanted {want}, got {ids:?}", q.question));
                }
            }
            "keyword" => {
                o.n_kw += 1;
                o.kw_at1 += usize::from(ids.first().copied() == q.expect.as_deref());
            }
            _ => {
                o.n_un += 1;
                if !ids.is_empty() {
                    o.answered += 1;
                    o.noise
                        .push(format!("{} -> {} notes", q.question, ids.len()));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    o
}

#[test]
#[ignore]
fn meaning_set_live() {
    let s = set();
    let route = OllamaRoute::new("http://127.0.0.1:11434", "nomic-embed-text").expect("route");
    let inner: Arc<dyn Embedder> = Arc::new(OllamaEmbedder::new(route, "nomic-embed-text".into()));
    let shipped = inner.min_similarity();
    let mut cutoffs = vec![shipped, 0.52, 0.55, 0.58, 0.6, 0.65];
    cutoffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cutoffs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    for c in cutoffs {
        let o = score(&s, &inner, c);
        println!(
            "MEANING-SET cutoff={:.2}{} paraphrase n={} found_first={} in_top3={} in_top5={}; keyword n={} first={}; unrelated n={} answered_with_notes={}",
            o.cutoff, if (o.cutoff - shipped).abs() < 1e-6 { " (shipped)" } else { "" }, o.n_par, o.at1, o.at3, o.at5, o.n_kw, o.kw_at1, o.n_un, o.answered
        );
        for m in &o.misses {
            println!("MEANING-SET-MISS at {:.2}: {m}", o.cutoff);
        }
        for m in &o.noise {
            println!("MEANING-SET-NOISE at {:.2}: {m}", o.cutoff);
        }
        // the guard: the shipped cut-off must keep what it was chosen for (floors from the measurement, with one question of room)
        if (o.cutoff - shipped).abs() < 1e-6 {
            assert!(
                o.at3 >= 19,
                "paraphrase questions found in the top 3: {} of {}",
                o.at3,
                o.n_par
            );
            assert_eq!(o.kw_at1, o.n_kw, "keyword questions found first");
            assert!(
                o.answered <= 1,
                "unrelated questions that drew notes: {} of {}: {:?}",
                o.answered,
                o.n_un,
                o.noise
            );
        }
    }
}
