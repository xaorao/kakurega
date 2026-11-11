//! Methods that ship with the program. They are ordinary methods: text and trigger words, nothing else, and like every
//! method they reach a conversation only after the owner has added one and approved its exact text.

use crate::{Draft, Method};
use serde_json::Value;

const SOURCE: &str = include_str!("bundled.json");

#[derive(Debug, Clone)]
pub struct Bundled {
    /// The heading it is listed under.
    pub group: String,
    pub method: Method,
}

/// Every bundled method, in the order of the file. An entry that does not read as a valid method is left out; the tests
/// make sure there is none.
pub fn all() -> Vec<Bundled> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(SOURCE) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|mut item| {
            let group = item.as_object_mut()?.remove("group")?.as_str()?.to_string();
            let method = Method::new(Draft::from_json(&item).ok()?).ok()?;
            Some(Bundled { group, method })
        })
        .collect()
}

pub fn find(name: &str) -> Option<Bundled> {
    all().into_iter().find(|b| b.method.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{select, Approval, Approved, Limits};

    #[test]
    fn bd01_every_bundled_method_is_a_valid_method_with_its_own_name_and_group() {
        let raw: Value = serde_json::from_str(SOURCE).unwrap();
        let listed = raw.as_array().unwrap().len();
        let all = all();
        assert_eq!(
            all.len(),
            listed,
            "an entry in the file does not read as a valid method"
        );
        assert!(all.len() >= 15);
        let mut names: Vec<&str> = all.iter().map(|b| b.method.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len(), "two bundled methods share a name");
        for b in &all {
            let m = &b.method;
            assert!(
                ["Engineering", "Product", "Business", "Writing", "Analysis"]
                    .contains(&b.group.as_str()),
                "{}",
                m.name
            );
            assert!(m.intact(), "{}", m.name);
            assert!(
                m.description.len() >= 30 && m.body.len() >= 300,
                "{} is too thin to be useful",
                m.name
            );
            assert!(
                m.body.len() <= 2500,
                "{} would take too much of a small model's window",
                m.name
            );
            assert!(!m.triggers.is_empty(), "{}", m.name);
            // a method is guidance: it names no tool and gives no order to ignore the product's rules
            let text = format!("{} {}", m.description, m.body).to_lowercase();
            for phrase in [
                "ignore previous",
                "ignore all",
                "system prompt",
                "run this command",
                "curl ",
                "http://",
                "https://",
            ] {
                assert!(!text.contains(phrase), "{} holds {phrase:?}", m.name);
            }
        }
        assert!(find("code-review").is_some() && find("no-such-method").is_none());
    }

    #[test]
    fn bd02_a_bundled_method_applies_to_the_request_it_is_for_and_not_to_an_unrelated_one() {
        let approved: Vec<Approved> = all()
            .into_iter()
            .map(|b| Approved {
                approval: Approval {
                    name: b.method.name.clone(),
                    hash: b.method.hash.clone(),
                },
                method: b.method,
            })
            .collect();
        let chosen = |message: &str| -> Vec<String> {
            select(message, &approved, Limits::default())
                .chosen
                .iter()
                .map(|a| a.method.name.clone())
                .collect()
        };
        for (message, want) in [
            (
                "Please do a code review of this pull request",
                "code-review",
            ),
            (
                "Write a threat model for the upload feature",
                "threat-model",
            ),
            ("Help me file a bug with steps to reproduce", "bug-report"),
            ("Draft a commit message for this change", "commit-message"),
            ("Write the release notes for version 2", "release-notes"),
            (
                "Write a postmortem for last night's outage",
                "incident-review",
            ),
            (
                "Turn this transcript into meeting notes with action items",
                "meeting-notes",
            ),
            (
                "Read this contract and tell me about the termination clause",
                "contract-reading",
            ),
            (
                "Does this feature collect personal data? Do a privacy review",
                "privacy-check",
            ),
            ("Rewrite this in plain language", "plain-language"),
            ("Is this SQL migration safe?", "sql-review"),
            ("Review the API design of these endpoints", "api-design"),
            (
                "Write up this decision as a decision record",
                "decision-record",
            ),
            (
                "Are these survey results and the sample size enough?",
                "numbers-check",
            ),
            ("Draft a reply to this email", "email-reply"),
            ("Do a dependency audit of this lockfile", "dependency-check"),
            (
                "What does the evidence say? Summarise these papers",
                "research-brief",
            ),
            ("What should we test? Give me a test plan", "test-plan"),
        ] {
            assert!(
                chosen(message).iter().any(|n| n == want),
                "{message:?} chose {:?}, not {want}",
                chosen(message)
            );
        }
        assert!(chosen("What is the capital of Peru?").is_empty());
        assert!(chosen("Good morning, how are you today?").is_empty());
        // a smart contract is code, not a legal contract
        assert!(!chosen("Audit this smart contract for reentrancy")
            .iter()
            .any(|n| n == "contract-reading"));
        // naming one with a slash applies it whatever the words
        assert_eq!(
            chosen("/plain-language tell me about Peru"),
            ["plain-language"]
        );
    }
}
