//! Memory recall benchmark on LongMemEval (MIT, Di Wu et al.), run through the real recall path.
//!
//! Not part of the normal test run: it needs the sample built by `scripts/recall-bench-prepare.py`, a running Ollama with the
//! embedding model, and several minutes.
//!
//!     python3 scripts/recall-bench-prepare.py
//!     cargo test -p kk-cli --bin kaku recall_benchmark -- --ignored --nocapture
//!
//! Protocol: the reference one (xiaowu0162/LongMemEval, src/retrieval). Each question gets a fresh store holding the user turns
//! of its history sessions as notes; the question is the query; the ground truth is the turns marked `has_answer`. Session
//! level scores come from the turn ranking by the reference's own turn-to-session rule. The settings (how many notes meaning
//! may bring in, how close a note must be) are chosen on the development questions and reported on the test questions.

use crate::runner::OllamaEmbedder;
use kk_memory::{Embedder, Memories, NewMemory, Remembered, Source};
use kk_route::OllamaRoute;
use kk_types::label::{Label, Level};
use kk_vault::{Vault, VaultKey};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Deserialize)]
struct Sample {
    questions: Vec<Question>,
}

#[derive(Deserialize)]
struct Question {
    #[serde(rename = "type")]
    qtype: String,
    question: String,
    split: String,
    sessions: Vec<Session>,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    turns: Vec<Turn>,
}

#[derive(Deserialize)]
struct Turn {
    i: u32,
    text: String,
    answer: bool,
}

/// The production embedder behind a cache (the same text is embedded once however many settings are tried), with the
/// closeness threshold adjustable between runs.
struct Cached {
    inner: Arc<dyn Embedder>,
    docs: Mutex<HashMap<String, Vec<f32>>>,
    queries: Mutex<HashMap<String, Vec<f32>>>,
    min: AtomicU32,
    embedded: AtomicUsize,
}

impl Cached {
    fn set_min(&self, v: f32) {
        self.min.store(v.to_bits(), Ordering::SeqCst);
    }
}

impl Embedder for Cached {
    fn model(&self) -> String {
        self.inner.model()
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let misses: Vec<String> = {
            let docs = self.docs.lock().unwrap();
            let mut seen = HashSet::new();
            texts
                .iter()
                .filter(|t| !docs.contains_key(*t) && seen.insert((*t).clone()))
                .cloned()
                .collect()
        };
        for chunk in misses.chunks(32) {
            let vecs = self.inner.embed(chunk)?;
            if vecs.len() != chunk.len() {
                return Err("the embedder returned the wrong number of vectors".into());
            }
            self.embedded.fetch_add(chunk.len(), Ordering::SeqCst);
            let mut docs = self.docs.lock().unwrap();
            for (t, v) in chunk.iter().zip(vecs) {
                docs.insert(t.clone(), v);
            }
        }
        let docs = self.docs.lock().unwrap();
        Ok(texts.iter().map(|t| docs[t].clone()).collect())
    }
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        if let Some(v) = self.queries.lock().unwrap().get(text) {
            return Ok(v.clone());
        }
        let v = self.inner.embed_query(text)?;
        self.queries
            .lock()
            .unwrap()
            .insert(text.to_string(), v.clone());
        Ok(v)
    }
    fn min_similarity(&self) -> f32 {
        f32::from_bits(self.min.load(Ordering::SeqCst))
    }
}

// ---- the reference metrics (src/retrieval/eval_utils.py), ported exactly ----

fn dcg(rels: &[f64], k: usize) -> f64 {
    rels.iter()
        .take(k)
        .enumerate()
        .map(|(i, r)| {
            if i == 0 {
                *r
            } else {
                r / ((i + 1) as f64).log2()
            }
        })
        .sum()
}

fn ndcg(rankings: &[usize], correct: &HashSet<String>, ids: &[String], k: usize) -> f64 {
    let rels: Vec<f64> = ids.iter().map(|d| f64::from(correct.contains(d))).collect();
    let got: Vec<f64> = rankings.iter().take(k).map(|&i| rels[i]).collect();
    let mut ideal = rels.clone();
    ideal.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let best = dcg(&ideal, k);
    if best == 0.0 {
        0.0
    } else {
        dcg(&got, k) / best
    }
}

/// (recall_any, recall_all, ndcg) at k over the given ids.
fn evaluate(
    rankings: &[usize],
    correct: &HashSet<String>,
    ids: &[String],
    k: usize,
) -> (f64, f64, f64) {
    let got: HashSet<&String> = rankings.iter().take(k).map(|&i| &ids[i]).collect();
    let any = f64::from(correct.iter().any(|d| got.contains(d)));
    let all = f64::from(correct.iter().all(|d| got.contains(d)));
    (any, all, ndcg(rankings, correct, ids, k))
}

fn strip_turn(id: &str) -> String {
    id.rsplit_once('_')
        .map(|(s, _)| s)
        .unwrap_or(id)
        .to_string()
}

/// The reference turn-to-session rule: widen k until k distinct sessions are in the list (here: or the list is used up, since
/// a recall that returns fewer notes does not rank the rest).
fn evaluate_sessions(
    rankings: &[usize],
    correct_turns: &HashSet<String>,
    turn_ids: &[String],
    k: usize,
) -> (f64, f64, f64) {
    let correct: HashSet<String> = correct_turns.iter().map(|d| strip_turn(d)).collect();
    let ids: Vec<String> = turn_ids.iter().map(|d| strip_turn(d)).collect();
    let mut eff = k;
    let distinct = |upto: usize| {
        rankings
            .iter()
            .take(upto)
            .map(|&i| &ids[i])
            .collect::<HashSet<_>>()
            .len()
    };
    while eff < rankings.len() && distinct(eff) < k {
        eff += 1;
    }
    evaluate(rankings, &correct, &ids, eff)
}

#[derive(Default, Clone)]
struct Scores {
    n: usize,
    sum: BTreeMap<&'static str, f64>,
}

impl Scores {
    fn add(&mut self, rankings: &[usize], correct: &HashSet<String>, ids: &[String]) {
        self.n += 1;
        let mut put = |name: &'static str, v: f64| *self.sum.entry(name).or_default() += v;
        for k in [5, 10] {
            let (any, all, nd) = evaluate_sessions(rankings, correct, ids, k);
            put(if k == 5 { "s_any@5" } else { "s_any@10" }, any);
            put(if k == 5 { "s_all@5" } else { "s_all@10" }, all);
            if k == 10 {
                put("s_ndcg@10", nd);
            }
        }
        let (_, all10, _) = evaluate(rankings, correct, ids, 10);
        let (_, all50, _) = evaluate(rankings, correct, ids, 50);
        put("t_all@10", all10);
        put("t_all@50", all50);
    }
    fn mean(&self, name: &str) -> f64 {
        self.sum.get(name).copied().unwrap_or(0.0) / self.n.max(1) as f64
    }
    fn line(&self) -> String {
        format!(
            "n={:<3} session any@5 {:.3}  all@5 {:.3}  any@10 {:.3}  all@10 {:.3}  ndcg@10 {:.3} | turn all@10 {:.3}  all@50 {:.3}",
            self.n,
            self.mean("s_any@5"),
            self.mean("s_all@5"),
            self.mean("s_any@10"),
            self.mean("s_all@10"),
            self.mean("s_ndcg@10"),
            self.mean("t_all@10"),
            self.mean("t_all@50"),
        )
    }
}

struct Built {
    mem: Memories,
    /// Corpus turn ids in order, and for each note id the corpus positions it stands for.
    turn_ids: Vec<String>,
    positions: HashMap<String, Vec<usize>>,
    correct: HashSet<String>,
    refused: usize,
    refused_gold: usize,
    dir: std::path::PathBuf,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn build(q: &Question, embedder: Arc<Cached>) -> Built {
    let dir = std::env::temp_dir().join(format!(
        "kk-recall-bench-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut mem = Memories::open(
        Vault::create(&dir.join("m.db"), &VaultKey::from_bytes([7; 32])).unwrap(),
        [9; 32],
    )
    .unwrap();
    mem.set_embedder(embedder);
    let (mut turn_ids, mut positions, mut correct) = (
        Vec::new(),
        HashMap::<String, Vec<usize>>::new(),
        HashSet::new(),
    );
    let (mut refused, mut refused_gold) = (0, 0);
    for s in &q.sessions {
        for t in &s.turns {
            let tid = format!("{}_{}", s.id, t.i);
            let pos = turn_ids.len();
            turn_ids.push(tid.clone());
            if t.answer {
                correct.insert(tid);
            }
            let text: String = t.text.chars().take(4096).collect();
            let m = NewMemory {
                text,
                label: Label::new(Level::Internal),
                subject: None,
                project: None,
                reason: "benchmark".into(),
                source: Source::User,
                valid_from: None,
                valid_to: None,
            };
            match mem.remember(1, m) {
                Ok(Remembered::Added(id)) | Ok(Remembered::AlreadyKnown(id)) => {
                    positions.entry(id).or_default().push(pos)
                }
                Err(_) => {
                    refused += 1;
                    if t.answer {
                        refused_gold += 1;
                    }
                }
            }
        }
    }
    // make every note's vector now, so the measured recall sees the same vectors a long-used store would have
    let notes = positions.len();
    for _ in 0..(notes / 20 + 50) {
        if mem.vector_count() >= notes {
            break;
        }
        let _ = mem.recall("warm up the vectors", &Label::new(Level::Internal), 2, 1);
    }
    Built {
        mem,
        turn_ids,
        positions,
        correct,
        refused,
        refused_gold,
        dir,
    }
}

fn rank(b: &Built, query: &str, limit: usize) -> Vec<usize> {
    let got = b
        .mem
        .recall(query, &Label::new(Level::Internal), 2, limit)
        .unwrap();
    let mut out = Vec::new();
    for r in got {
        if let Some(p) = b.positions.get(&r.memory.id) {
            out.extend(p.iter().copied());
        }
    }
    out
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Plain cosine ranking of every turn: the baseline a dense retriever gives, with no threshold and no keywords.
fn dense_rank(b: &Built, emb: &Cached, q: &Question) -> Vec<usize> {
    let texts: Vec<String> = q
        .sessions
        .iter()
        .flat_map(|s| {
            s.turns
                .iter()
                .map(|t| t.text.chars().take(4096).collect::<String>())
        })
        .collect();
    let docs = emb.embed(&texts).unwrap();
    let qv = emb.embed_query(&q.question).unwrap();
    let mut scored: Vec<(f32, usize)> = docs
        .iter()
        .enumerate()
        .map(|(i, v)| (cosine(&qv, v), i))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
    let _ = b;
    scored.into_iter().take(50).map(|(_, i)| i).collect()
}

/// One setting of the recall: how many notes meaning may bring in, how close a note must be, and how much meaning counts.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Setting {
    cap: usize,
    min: f32,
    weight: f64,
}

const SHIPPED: Setting = Setting {
    cap: 5,
    min: 0.5,
    weight: 1.0,
};
const KEYWORDS_ONLY: Setting = Setting {
    cap: 0,
    min: 0.5,
    weight: 1.0,
};

type Table = BTreeMap<(usize, String), Scores>;

fn settings() -> Vec<Setting> {
    let mut out = vec![KEYWORDS_ONLY];
    for cap in [5usize, 10, 20, 50] {
        for min in [0.35f32, 0.45, 0.5, 0.55, 0.6] {
            for weight in [1.0f64, 2.0, 4.0] {
                out.push(Setting { cap, min, weight });
            }
        }
    }
    out
}

#[test]
#[ignore]
fn recall_benchmark() {
    let path = std::env::var("KK_RECALL_SAMPLE").unwrap_or_else(|_| {
        format!(
            "{}/../../target/recall-bench/sample.json",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let sample: Sample = serde_json::from_slice(
        &std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{path}: {e}; run scripts/recall-bench-prepare.py")),
    )
    .unwrap();
    let route = OllamaRoute::new("http://127.0.0.1:11434", "nomic-embed-text").expect("route");
    let emb = Arc::new(Cached {
        inner: Arc::new(OllamaEmbedder::new(route, "nomic-embed-text".into())),
        docs: Default::default(),
        queries: Default::default(),
        min: AtomicU32::new(0.5f32.to_bits()),
        embedded: AtomicUsize::new(0),
    });
    let all = settings();
    // scores[(setting index, "split:ALL" | "split:type")]; the dense baseline has its own table
    let mut table: Table = BTreeMap::new();
    let mut dense: BTreeMap<String, Scores> = BTreeMap::new();
    let (mut refused, mut refused_gold, mut notes) = (0usize, 0usize, 0usize);
    let started = std::time::Instant::now();
    for (n, q) in sample.questions.iter().enumerate() {
        let mut b = build(q, emb.clone());
        refused += b.refused;
        refused_gold += b.refused_gold;
        notes += b.positions.len();
        let keys = |split: &str| {
            [
                format!("{split}:ALL"),
                format!("{split}:{}", q.qtype),
                "all:ALL".to_string(),
                format!("all:{}", q.qtype),
            ]
        };
        for (si, st) in all.iter().enumerate() {
            emb.set_min(st.min);
            b.mem.set_meaning_cap(st.cap);
            b.mem.set_meaning_weight(st.weight);
            let r = rank(&b, &q.question, 50);
            for key in keys(&q.split) {
                table
                    .entry((si, key))
                    .or_default()
                    .add(&r, &b.correct, &b.turn_ids);
            }
        }
        let r = dense_rank(&b, &emb, q);
        for key in keys(&q.split) {
            dense
                .entry(key)
                .or_default()
                .add(&r, &b.correct, &b.turn_ids);
        }
        // the store is not needed again; free its folder now
        drop(b.mem);
        let _ = std::fs::remove_dir_all(&b.dir);
        if (n + 1) % 10 == 0 || n + 1 == sample.questions.len() {
            println!(
                "done {}/{} ({} notes, {} embedded, {:.0}s)",
                n + 1,
                sample.questions.len(),
                notes,
                emb.embedded.load(Ordering::SeqCst),
                started.elapsed().as_secs_f64()
            );
        }
    }
    println!("notes the store refused: {refused} (of which evidence notes: {refused_gold})");
    let at = |s: Setting, key: &str| -> Scores {
        let i = all.iter().position(|x| *x == s).expect("setting");
        table
            .get(&(i, key.to_string()))
            .cloned()
            .unwrap_or_default()
    };
    let types: Vec<String> = {
        let mut t: Vec<String> = sample.questions.iter().map(|q| q.qtype.clone()).collect();
        t.sort();
        t.dedup();
        t
    };

    println!(
        "\n== all {} questions: keywords only, as shipped, dense only (no merge, no threshold)",
        sample.questions.len()
    );
    for key in std::iter::once("ALL".to_string()).chain(types.iter().cloned()) {
        let k = format!("all:{key}");
        println!(
            "{key}\n  keywords+wording   {}\n  fused (shipped)    {}\n  dense only         {}",
            at(KEYWORDS_ONLY, &k).line(),
            at(SHIPPED, &k).line(),
            dense.get(&k).cloned().unwrap_or_default().line()
        );
    }

    println!("\n== development questions: every setting");
    let rank_key = |s: &Scores| (s.mean("s_all@10"), s.mean("s_any@5"), s.mean("s_ndcg@10"));
    let mut dev: Vec<(Setting, Scores)> = all.iter().map(|s| (*s, at(*s, "dev:ALL"))).collect();
    for (s, sc) in &dev {
        println!(
            "  cap {:<2} min {:.2} weight {:.0}  {}",
            s.cap,
            s.min,
            s.weight,
            sc.line()
        );
    }
    // rank by session all@10, then any@5, then ndcg@10; the shipped setting is kept unless another beats it on development
    // by a real margin (0.01), so a tie or noise never changes a default
    dev.sort_by(|a, b| rank_key(&b.1).partial_cmp(&rank_key(&a.1)).unwrap());
    let shipped_dev = rank_key(&at(SHIPPED, "dev:ALL"));
    let top = (dev[0].0, rank_key(&dev[0].1));
    let better = top.1 .0 - shipped_dev.0 >= 0.01
        || (top.1 .0 == shipped_dev.0 && top.1 .1 - shipped_dev.1 >= 0.01)
        || (top.1 .0 == shipped_dev.0
            && top.1 .1 == shipped_dev.1
            && top.1 .2 - shipped_dev.2 >= 0.01);
    let chosen = if better { top.0 } else { SHIPPED };
    println!("\nchosen on development: {chosen:?} (the shipped setting is kept unless another is better by a margin of 0.01)");

    println!(
        "\n== held-out test questions ({} questions)",
        sample
            .questions
            .iter()
            .filter(|q| q.split == "test")
            .count()
    );
    for key in std::iter::once("ALL".to_string()).chain(types.iter().cloned()) {
        let k = format!("test:{key}");
        println!("{key}\n  keywords+wording   {}\n  fused (shipped)    {}\n  fused (chosen)     {}\n  dense only         {}", at(KEYWORDS_ONLY, &k).line(), at(SHIPPED, &k).line(), at(chosen, &k).line(), dense.get(&k).cloned().unwrap_or_default().line());
    }
}
