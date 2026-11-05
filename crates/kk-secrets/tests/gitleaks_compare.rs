use kk_secrets::scan;
use serde_json::Value;

/// Recall and false-positive comparison against gitleaks' own per-rule samples.
/// The samples are fake secrets and must not live in the repository, so the file is read from KK_GITLEAKS_SAMPLES.
#[test]
#[ignore]
fn gl01_gitleaks_samples_recall_and_false_positives() {
    let path = std::env::var("KK_GITLEAKS_SAMPLES").expect("KK_GITLEAKS_SAMPLES");
    let text = std::fs::read_to_string(path).unwrap();
    let (mut tp_total, mut tp_hit, mut fp_total, mut fp_hit) = (0, 0, 0, 0);
    let mut missed: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut fp_rules: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for line in text.lines() {
        let v: Value = serde_json::from_str(line).unwrap();
        let rule = v["rule"].as_str().unwrap().to_string();
        for s in v["tps"].as_array().into_iter().flatten() {
            let s = s.as_str().unwrap();
            tp_total += 1;
            if scan(s).is_empty() {
                if std::env::var("KK_SHOW")
                    .map(|w| w == "all" || w.split(',').any(|x| x == rule))
                    .unwrap_or(false)
                {
                    println!("SHOW {rule}: {}", s.replace('\n', "\\n"));
                }
                let e = missed.entry(rule.clone()).or_default();
                e.0 += 1;
            } else {
                tp_hit += 1;
            }
            missed.entry(rule.clone()).or_default().1 += 1;
        }
        for s in v["fps"].as_array().into_iter().flatten() {
            let s = s.as_str().unwrap();
            fp_total += 1;
            if !scan(s).is_empty() {
                fp_hit += 1;
                if std::env::var("KK_SHOWFP").is_ok() {
                    let hits = scan(s);
                    let h = &hits[0];
                    println!(
                        "SHOWFP {rule}: [{}] {}",
                        h.rule,
                        s.chars().take(110).collect::<String>().replace('\n', "\\n")
                    );
                }
                fp_rules
                    .entry(rule.clone())
                    .or_default()
                    .push(s.chars().take(60).collect());
            }
        }
    }
    println!("TP {tp_hit}/{tp_total} found; FP {fp_hit}/{fp_total} wrongly flagged");
    for (r, (m, n)) in &missed {
        if *m > 0 {
            println!("MISS {r}: {m}/{n}");
        }
    }
    for (r, v) in &fp_rules {
        println!("FP {r}: {}", v.len());
    }
}
