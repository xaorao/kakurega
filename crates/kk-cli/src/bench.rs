//! `kaku bench sign` and `kaku bench verify`: the leak benchmark's results as a signed file anyone can check.
//!
//! A signature says the results were not changed after the holder of the key signed them. It says nothing about whether
//! the run was honest; for that the verifier re-reads every case and recomputes each verdict and every count from the
//! recorded observations, and the benchmark itself can be run again (`benchmarks/leak/reverify --rerun`).

use kk_evidence::{approval, b64, canon, dsse, keys};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const RESULTS_TYPE: &str = "application/vnd.kakurega.leak-benchmark.v1+json";
const VERDICTS: [&str; 6] = [
    "held",
    "refused",
    "sent_by_owner",
    "allowed_on_this_computer",
    "LEAK",
    "not_run",
];

fn fail(msg: &str) -> u8 {
    eprintln!("{msg}");
    2
}

/// The verdict a case's own observations give. The doors where the owner hands the value over (typed into the chat box)
/// or labelled it for the local model (a restricted note) are counted apart, as the benchmark's method says.
pub fn verdict_of(case: &Value) -> &'static str {
    let reached = case["reached_model"].as_bool().unwrap_or(false);
    let on_disk = case["on_disk_plain"]
        .as_array()
        .is_some_and(|a| !a.is_empty());
    let asked = case["model_was_asked"].as_bool().unwrap_or(false);
    let status = case["http_status"].as_u64().unwrap_or(0);
    let door = case["door"].as_str().unwrap_or("");
    if on_disk || (reached && door != "typed" && door != "note") {
        "LEAK"
    } else if reached && door == "typed" {
        "sent_by_owner"
    } else if reached {
        "allowed_on_this_computer"
    } else if matches!(status, 200 | 201) && asked {
        "held"
    } else if matches!(status, 409 | 422) && !asked {
        "refused"
    } else {
        "not_run"
    }
}

/// What a set of results says, recomputed from its cases, and everything in it that does not add up.
pub fn summarise(doc: &Value) -> (Value, Vec<String>) {
    let mut problems = Vec::new();
    if doc["type"] != "kakurega-leak-benchmark" || doc["version"] != 1 {
        problems.push("this is not a version 1 leak benchmark result".to_string());
    }
    let empty = Vec::new();
    let cases = doc["cases"].as_array().unwrap_or(&empty);
    if cases.is_empty() {
        problems.push("there are no cases".to_string());
    }
    let mut groups: BTreeMap<String, BTreeMap<&'static str, u64>> = BTreeMap::new();
    let mut leaks = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for c in cases {
        let name = c["case"].as_str().unwrap_or("?").to_string();
        if !seen.insert(name.clone()) {
            problems.push(format!("{name}: listed twice"));
        }
        let v = verdict_of(c);
        if c["verdict"].as_str() != Some(v) {
            problems.push(format!(
                "{name}: recorded as {} but its observations give {v}",
                c["verdict"].as_str().unwrap_or("nothing")
            ));
        }
        if c["plain_arm_leaked"] != true {
            problems.push(format!(
                "{name}: the plain arm did not leak, so the control that shows a leak would be seen failed"
            ));
        }
        if v == "not_run" {
            problems.push(format!("{name}: was not run, which shows nothing"));
        }
        if v == "LEAK" {
            leaks.push(name);
        }
        let group = c["group"].as_str().unwrap_or("?").to_string();
        *groups.entry(group).or_default().entry(v).or_insert(0) += 1;
    }
    let by_group: BTreeMap<String, Value> = groups
        .iter()
        .map(|(g, t)| {
            let total: u64 = t.values().sum();
            let mut o = json!({"cases": total});
            for v in VERDICTS {
                o[v] = json!(t.get(v).copied().unwrap_or(0));
            }
            (g.clone(), o)
        })
        .collect();
    (
        json!({"cases": cases.len(), "groups": by_group, "leaks": leaks, "commit": doc["commit"], "tree_had_uncommitted_changes": doc["tree_had_uncommitted_changes"], "model": doc["model"]}),
        problems,
    )
}

pub fn run(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("sign") => sign(&args[1..]),
        Some("verify") => verify(&args[1..]),
        _ => fail("usage: kaku bench sign RESULTS.json --key KEYFILE [--new-key] [--out FILE]\n       kaku bench verify SIGNED.json --pin PUBLIC-KEY [--json]"),
    }
}

fn sign(args: &[String]) -> u8 {
    let Some(file) = args.first().filter(|a| !a.starts_with("--")) else {
        return fail("usage: kaku bench sign RESULTS.json --key KEYFILE [--new-key] [--out FILE]");
    };
    let Some(key_path) = crate::flag(args, "--key") else {
        return fail("give the signing key file with --key (it is made with --new-key and must stay outside the repository)");
    };
    let text = match std::fs::read(file) {
        Ok(t) => t,
        Err(e) => return fail(&format!("cannot read {file}: {e}")),
    };
    let canonical = match canon::canonicalize(&text) {
        Ok(c) => c,
        Err(_) => return fail(
            "the results are not JSON this can sign (whole numbers, text and true or false only)",
        ),
    };
    let doc: Value = serde_json::from_str(&canonical).unwrap_or(Value::Null);
    let (_, problems) = summarise(&doc);
    if !problems.is_empty() {
        for p in problems.iter().take(20) {
            eprintln!("  {p}");
        }
        return fail("these results do not add up, so they are not signed");
    }
    let signer = match approval::load_key(Path::new(&key_path)) {
        Ok(Some(s)) => s,
        Ok(None) if args.iter().any(|a| a == "--new-key") => {
            match approval::create_key(Path::new(&key_path)) {
                Ok(s) => s,
                Err(e) => return fail(&format!("cannot make the key file {key_path}: {e:?}")),
            }
        }
        Ok(None) => {
            return fail(&format!(
                "there is no key at {key_path}; add --new-key to make one"
            ))
        }
        Err(e) => {
            return fail(&format!(
                "cannot use the key file {key_path}: {e:?} (it must be a file only you can read)"
            ))
        }
    };
    let env = dsse::sign(RESULTS_TYPE, canonical.as_bytes(), &signer);
    let out = crate::flag(args, "--out").unwrap_or_else(|| file.replace(".json", ".signed.json"));
    if let Err(e) = std::fs::write(&out, dsse::encode(&env) + "\n") {
        return fail(&format!("cannot write {out}: {e}"));
    }
    println!("Wrote {out}");
    println!(
        "Signed with the key whose public part is: {}",
        keys::public_key_b64(&signer.public())
    );
    0
}

fn verify(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let Some(file) = args.first().filter(|a| !a.starts_with("--")) else {
        return fail("usage: kaku bench verify SIGNED.json --pin PUBLIC-KEY [--json]");
    };
    let Some(pin) = crate::flag(args, "--pin") else {
        return fail(
            "give the public key you expect with --pin; without it a signature shows nothing",
        );
    };
    let public: [u8; 32] = match b64::decode(pin.trim()).ok().and_then(|b| b.try_into().ok()) {
        Some(p) => p,
        None => return fail("--pin is not a public key (32 bytes in base64)"),
    };
    let mut trusted = keys::TrustedKeys::new();
    if trusted.add(&public).is_none() {
        return fail("--pin is not a usable public key");
    }
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => return fail(&format!("cannot read {file}: {e}")),
    };
    let env = match dsse::decode(bytes.trim_ascii()) {
        Ok(e) => e,
        Err(e) => return fail(&format!("{file} is not a signed results file ({e:?})")),
    };
    if let Err(e) = dsse::verify(&env, &trusted, &[RESULTS_TYPE]) {
        return fail(&format!(
            "NOT VERIFIED: the signature does not check against that key ({e:?})"
        ));
    }
    let doc: Value = serde_json::from_slice(&env.payload).unwrap_or(Value::Null);
    let (summary, problems) = summarise(&doc);
    if json_mode {
        println!(
            "{}",
            json!({"signature": "ok", "summary": summary, "problems": problems})
        );
    } else {
        println!("Signature: checks against the pinned key.");
        println!(
            "Run on commit {}{}, model: {}",
            summary["commit"].as_str().unwrap_or("?"),
            if summary["tree_had_uncommitted_changes"] == true {
                " with uncommitted changes"
            } else {
                ""
            },
            summary["model"].as_str().unwrap_or("?")
        );
        if let Some(groups) = summary["groups"].as_object() {
            for (g, t) in groups {
                println!(
                    "{g}: {} cases. Held {}, refused before any model {}, sent by the owner's own hand {}, allowed on this computer by label {}, leaked {}.",
                    t["cases"], t["held"], t["refused"], t["sent_by_owner"], t["allowed_on_this_computer"], t["LEAK"]
                );
            }
        }
        for l in summary["leaks"].as_array().into_iter().flatten() {
            println!("  leaked: {}", l.as_str().unwrap_or("?"));
        }
        println!("Recomputed from the {} recorded cases; the plain arm leaked in every one, which is the control.", summary["cases"]);
        println!("A signature shows the file was not changed after signing. It does not show the run was honest: run it again with benchmarks/leak/reverify --rerun.");
    }
    if problems.is_empty() {
        0
    } else {
        for p in problems.iter().take(20) {
            eprintln!("  {p}");
        }
        fail("NOT VERIFIED: the results do not add up")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(door: &str, status: u64, asked: bool, reached: bool, disk: bool) -> Value {
        let mut c = json!({"case": format!("{door}/x/{status}/{asked}/{reached}/{disk}"), "door": door, "group": "known shapes", "http_status": status, "model_was_asked": asked, "reached_model": reached, "on_disk_plain": if disk { json!(["chat.db"]) } else { json!([]) }, "plain_arm_leaked": true});
        c["verdict"] = json!(verdict_of(&c));
        c
    }

    #[test]
    fn bn01_a_verdict_follows_from_what_was_observed_and_nothing_else() {
        assert_eq!(verdict_of(&case("attach", 200, true, false, false)), "held");
        assert_eq!(verdict_of(&case("attach", 200, true, true, false)), "LEAK");
        assert_eq!(verdict_of(&case("attach", 200, true, false, true)), "LEAK");
        assert_eq!(
            verdict_of(&case("typed", 200, true, true, false)),
            "sent_by_owner"
        );
        assert_eq!(verdict_of(&case("typed", 200, true, true, true)), "LEAK");
        assert_eq!(
            verdict_of(&case("note", 200, true, true, false)),
            "allowed_on_this_computer"
        );
        assert_eq!(
            verdict_of(&case("note", 422, false, false, false)),
            "refused"
        );
        // an error is not protection
        assert_eq!(
            verdict_of(&case("attach", 500, false, false, false)),
            "not_run"
        );
        assert_eq!(
            verdict_of(&case("attach", 200, false, false, false)),
            "not_run"
        );
    }

    #[test]
    fn bn02_results_that_were_edited_to_look_better_do_not_add_up() {
        let good = json!({"type": "kakurega-leak-benchmark", "version": 1, "commit": "abc", "cases": [case("attach", 200, true, false, false), case("ask", 200, true, true, false)]});
        let (s, problems) = summarise(&good);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(s["groups"]["known shapes"]["LEAK"], json!(1));
        assert_eq!(s["leaks"], json!(["ask/x/200/true/true/false"]));
        // the leak relabelled as held
        let mut edited = good.clone();
        edited["cases"][1]["verdict"] = json!("held");
        assert!(summarise(&edited)
            .1
            .iter()
            .any(|p| p.contains("observations give LEAK")));
        // the control quietly failing, a case run twice, a case that never ran, no cases, another kind of file
        let mut no_control = good.clone();
        no_control["cases"][0]["plain_arm_leaked"] = json!(false);
        assert!(summarise(&no_control)
            .1
            .iter()
            .any(|p| p.contains("control")));
        let mut twice = good.clone();
        twice["cases"][1] = twice["cases"][0].clone();
        assert!(summarise(&twice)
            .1
            .iter()
            .any(|p| p.contains("listed twice")));
        let never = json!({"type": "kakurega-leak-benchmark", "version": 1, "cases": [case("attach", 500, false, false, false)]});
        assert!(summarise(&never)
            .1
            .iter()
            .any(|p| p.contains("was not run")));
        assert!(
            !summarise(&json!({"type": "kakurega-leak-benchmark", "version": 1, "cases": []}))
                .1
                .is_empty()
        );
        assert!(!summarise(&json!({"type": "something-else", "version": 1, "cases": [case("attach", 200, true, false, false)]})).1.is_empty());
    }
}
