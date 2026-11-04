use kk_policy::{
    Decision, DenyReason, Engine, Op, PolicyError, Resource, StepFacts, WorkerFacts, DEFAULT_SCHEMA,
};
use kk_types::label::{Label, Level};

fn worker(kind: &str, clearance: Label, tools: &[&str]) -> WorkerFacts {
    WorkerFacts {
        id: "w1".into(),
        kind: kind.into(),
        clearance,
        allowed_tools: tools.iter().map(|t| t.to_string()).collect(),
    }
}

fn step(taint: Label, approved: bool) -> StepFacts {
    StepFacts {
        taint,
        human_approved: approved,
    }
}

fn artifact(label: Label) -> Resource {
    Resource::Artifact {
        id: "a1".into(),
        label,
    }
}

fn route(class: &str, verified: bool) -> Resource {
    Resource::Route {
        id: "r1".into(),
        class: class.into(),
        tee_verified: verified,
    }
}

fn allowed(d: Decision) -> bool {
    d.is_allow()
}

#[test]
fn reading_follows_level_compartments_and_unknown_sensitivity() {
    let e = Engine::default_policy();
    let w = worker(
        "AGENT",
        Label::new(Level::Confidential).with_compartment("alpha"),
        &[],
    );
    let s = step(Label::bottom(), false);
    let read = |l: Label| e.decide(&w, Op::ReadArtifact, &artifact(l), &s);
    assert!(allowed(read(Label::new(Level::Public))));
    assert!(allowed(read(
        Label::new(Level::Confidential).with_compartment("alpha")
    )));
    assert!(!allowed(read(Label::new(Level::Restricted))));
    assert!(!allowed(read(
        Label::new(Level::Internal).with_compartment("beta")
    )));
    assert!(!allowed(read(
        Label::new(Level::Public).unknown_sensitivity()
    )));
}

#[test]
fn writing_and_proposing_depend_on_the_worker_kind() {
    let e = Engine::default_policy();
    let s = step(Label::bottom(), false);
    let res = artifact(Label::bottom());
    for (kind, write_ok, propose_ok) in [
        ("AGENT", true, true),
        ("PARSER", true, false),
        ("CHECKER", false, false),
        ("MCP_SERVER", false, false),
    ] {
        let w = worker(kind, Label::new(Level::Public), &[]);
        assert_eq!(
            allowed(e.decide(&w, Op::WriteArtifact, &res, &s)),
            write_ok,
            "write {kind}"
        );
        let rcpt = Resource::Recipient {
            id: "acme".into(),
            clearance_level: 2,
        };
        assert_eq!(
            allowed(e.decide(&w, Op::ReleasePropose, &rcpt, &s)),
            propose_ok,
            "propose {kind}"
        );
    }
}

#[test]
fn model_routes_follow_the_taint_and_never_fall_back() {
    let e = Engine::default_policy();
    let w = worker("AGENT", Label::new(Level::Restricted), &[]);
    let restricted = step(Label::new(Level::Restricted), false);
    let clean = step(Label::bottom(), false);
    assert!(allowed(e.decide(
        &w,
        Op::ModelComplete,
        &route("device_only", false),
        &restricted
    )));
    assert!(allowed(e.decide(
        &w,
        Op::ModelComplete,
        &route("verified_tee", true),
        &restricted
    )));
    let unverified = e.decide(
        &w,
        Op::ModelComplete,
        &route("verified_tee", false),
        &restricted,
    );
    assert!(
        matches!(unverified, Decision::Deny(DenyReason::Forbidden(_))),
        "{unverified:?}"
    );
    let unverified_clean = e.decide(&w, Op::ModelComplete, &route("verified_tee", false), &clean);
    assert!(
        matches!(
            unverified_clean,
            Decision::Deny(DenyReason::NoMatchingPermit)
        ),
        "a route that is not verified is never permitted: {unverified_clean:?}"
    );
    let public_restricted = e.decide(&w, Op::ModelComplete, &route("public", false), &restricted);
    assert!(
        matches!(public_restricted, Decision::Deny(DenyReason::Forbidden(_))),
        "{public_restricted:?}"
    );
    let public_clean = e.decide(&w, Op::ModelComplete, &route("public", false), &clean);
    assert!(
        matches!(public_clean, Decision::Deny(DenyReason::NoMatchingPermit)),
        "public is never permitted: {public_clean:?}"
    );
    let unknown = step(Label::new(Level::Public).unknown_sensitivity(), false);
    assert!(!allowed(e.decide(
        &w,
        Op::ModelComplete,
        &route("public", false),
        &unknown
    )));
}

#[test]
fn tools_need_a_grant_and_effects_need_a_human_when_untrusted_content_is_in_play() {
    let e = Engine::default_policy();
    let w = worker(
        "AGENT",
        Label::new(Level::Internal),
        &["grep", "send_email"],
    );
    let reader = Resource::Tool {
        id: "grep".into(),
        readonly: true,
    };
    let sender = Resource::Tool {
        id: "send_email".into(),
        readonly: false,
    };
    let ungranted = Resource::Tool {
        id: "delete_all".into(),
        readonly: true,
    };
    let trusted = step(Label::bottom(), false);
    let hostile = step(Label::bottom().untrusted(), false);
    let approved_hostile = step(Label::bottom().untrusted(), true);
    assert!(allowed(e.decide(&w, Op::ToolCall, &reader, &trusted)));
    assert!(allowed(e.decide(&w, Op::ToolCall, &reader, &hostile)));
    assert!(!allowed(e.decide(&w, Op::ToolCall, &ungranted, &trusted)));
    assert!(
        !allowed(e.decide(&w, Op::ToolCall, &sender, &trusted)),
        "an effect needs a human decision"
    );
    assert!(!allowed(e.decide(&w, Op::ToolCall, &sender, &hostile)));
    assert!(allowed(e.decide(
        &w,
        Op::ToolCall,
        &sender,
        &approved_hostile
    )));
}

#[test]
fn an_empty_policy_set_denies_everything() {
    let e = Engine::new("", DEFAULT_SCHEMA).unwrap();
    let w = worker("AGENT", Label::new(Level::Restricted), &["grep"]);
    let s = step(Label::bottom(), true);
    for (op, r) in [
        (Op::ReadArtifact, artifact(Label::bottom())),
        (Op::WriteArtifact, artifact(Label::bottom())),
        (Op::ModelComplete, route("device_only", false)),
        (
            Op::ToolCall,
            Resource::Tool {
                id: "grep".into(),
                readonly: true,
            },
        ),
    ] {
        assert_eq!(
            e.decide(&w, op, &r, &s),
            Decision::Deny(DenyReason::NoMatchingPermit),
            "{op:?}"
        );
    }
}

#[test]
fn an_evaluation_error_is_a_deny_even_when_a_permit_matches() {
    let policy = format!(
        "{}\nforbid (principal, action == Action::\"read.artifact\", resource) when {{ resource.level * 9223372036854775807 > 0 }};\n",
        kk_policy::DEFAULT_POLICY
    );
    let e = Engine::new(&policy, DEFAULT_SCHEMA)
        .expect("validates; the overflow only happens at run time");
    let w = worker("AGENT", Label::new(Level::Restricted), &[]);
    let s = step(Label::bottom(), false);
    let high = e.decide(
        &w,
        Op::ReadArtifact,
        &artifact(Label::new(Level::Restricted)),
        &s,
    );
    assert!(
        matches!(high, Decision::Deny(DenyReason::EvaluationError(_))),
        "{high:?}"
    );
    let low = e.decide(
        &w,
        Op::ReadArtifact,
        &artifact(Label::new(Level::Public)),
        &s,
    );
    assert!(
        low.is_allow(),
        "level 0 does not overflow and the forbid does not apply: {low:?}"
    );
}

#[test]
fn policies_that_do_not_fit_the_schema_are_refused_at_load() {
    let bad_attr =
        r#"permit (principal, action, resource) when { resource.has_untrusted_origin };"#;
    assert!(matches!(
        Engine::new(bad_attr, DEFAULT_SCHEMA),
        Err(PolicyError::Validation(_))
    ));
    let syntax = "permit (principal, action, resource";
    assert!(matches!(
        Engine::new(syntax, DEFAULT_SCHEMA),
        Err(PolicyError::Policy(_))
    ));
    let optional_style = r#"forbid (principal, action == Action::"read.artifact", resource) when { resource.nonexistent == 1 };"#;
    assert!(Engine::new(optional_style, DEFAULT_SCHEMA).is_err());
    assert!(matches!(
        Engine::new("", "entity ;"),
        Err(PolicyError::Schema(_))
    ));
}

#[test]
fn identifiers_are_data_not_policy_text() {
    let e = Engine::default_policy();
    let mut w = worker("AGENT", Label::new(Level::Public), &[]);
    w.id = "x\" || true || \"".into();
    let s = step(Label::bottom(), false);
    let r = Resource::Artifact {
        id: "a\\\" ) ; permit (principal, action, resource) ; //".into(),
        label: Label::new(Level::Restricted),
    };
    assert!(!allowed(e.decide(&w, Op::ReadArtifact, &r, &s)));
    let uni = Resource::Artifact {
        id: "résumé\u{202e}.txt".into(),
        label: Label::new(Level::Public),
    };
    assert!(allowed(e.decide(&w, Op::ReadArtifact, &uni, &s)));
}
