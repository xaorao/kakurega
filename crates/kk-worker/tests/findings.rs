use kk_worker::{build_finding, finding_tool};
use serde_json::json;

#[test]
fn w01_the_finding_tool_offers_exactly_the_three_languages_and_does_not_require_one() {
    let t = finding_tool();
    let props = &t.parameters["properties"];
    assert_eq!(
        props["language"]["enum"],
        json!(["python", "javascript", "typescript"])
    );
    let required: Vec<&str> = t.parameters["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(!required.contains(&"language"), "{required:?}");
    assert!(
        t.description.to_lowercase().contains("language"),
        "{}",
        t.description
    );
}

#[test]
fn w02_a_finding_carries_its_language_only_when_the_model_named_a_known_one() {
    let base = json!({"title": "t", "file": "src/a.js", "line": 3, "explanation": "e", "reproducer": "console.log(1)"});
    let plain = build_finding(&base).unwrap();
    assert_eq!(plain["reproducer"], "console.log(1)");
    assert!(
        plain.get("language").is_none(),
        "no language named, none added: {plain}"
    );
    for lang in ["python", "javascript", "typescript"] {
        let mut a = base.clone();
        a["language"] = json!(lang);
        assert_eq!(build_finding(&a).unwrap()["language"], lang);
    }
    // an unknown or oddly written language is sent back to the model to correct, never passed on or guessed
    for bad in [
        json!("rust"),
        json!("TypeScript"),
        json!("ts"),
        json!("JavaScript"),
        json!("js"),
        json!(""),
        json!(" python"),
        json!(3),
        json!(null),
        json!(["python"]),
        json!("python\n"),
    ] {
        let mut a = base.clone();
        a["language"] = bad.clone();
        let err = build_finding(&a).expect_err(&format!("{bad} must be refused"));
        assert!(
            err.contains("python") && err.contains("javascript") && err.contains("typescript"),
            "{err}"
        );
    }
}

#[test]
fn w03_a_finding_without_its_required_parts_is_refused_with_a_message_the_model_can_act_on() {
    for missing in ["title", "file", "line", "explanation"] {
        let mut a = json!({"title": "t", "file": "f", "line": 1, "explanation": "e"});
        a.as_object_mut().unwrap().remove(missing);
        let err = build_finding(&a).expect_err(missing);
        assert!(err.contains("title, file, line and explanation"), "{err}");
    }
    let wrong_type = json!({"title": "t", "file": "f", "line": "3", "explanation": "e"});
    assert!(build_finding(&wrong_type).is_err());
    // the reproducer is optional and defaults to empty, as before
    let f =
        build_finding(&json!({"title": "t", "file": "f", "line": 1, "explanation": "e"})).unwrap();
    assert_eq!(f["reproducer"], "");
}
