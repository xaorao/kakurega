mod support;

use kk_near::route::NearRoute;
use kk_route::{ChatMessage, CompletionRequest, Route, RouteClass, RouteError, RouteSet, ToolSpec};
use kk_types::label::{Label, Level};
use serde_json::json;
use support::{address, Faults, Gateway, GATEWAY_SEED, MODEL, MODEL_SEED};

struct Rig {
    _rt: tokio::runtime::Runtime,
    gw: Gateway,
    route: NearRoute,
}

fn rig(faults: Faults) -> Rig {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let gw = rt.block_on(Gateway::start(faults));
    let route = NearRoute::with_options(gw.options(), MODEL).unwrap();
    Rig { _rt: rt, gw, route }
}

fn ask(text: &str) -> CompletionRequest {
    CompletionRequest {
        messages: vec![
            ChatMessage::new("system", "Be brief."),
            ChatMessage::new("user", text),
        ],
        tools: vec![],
        max_tokens: 64,
        temperature: 0.0,
    }
}

#[test]
fn nr01_an_answer_comes_back_decrypted_with_the_models_signature_and_a_receipt_a_recipient_can_read(
) {
    let r = rig(Faults::default());
    assert!(
        r.route.evidence().receipt.is_none(),
        "no receipt before any answer"
    );
    assert!(!r.route.evidence().tee_verified);
    let c = r
        .route
        .complete(&ask("What is the secret plan of Zorblax?"))
        .unwrap();
    assert_eq!(c.message.content, "A private answer.");
    assert_eq!((c.prompt_tokens, c.completion_tokens), (11, 5));
    // the enclave saw the plaintext (it decrypted it) and the wire never held it
    assert_eq!(
        *r.gw.seen.lock().unwrap(),
        ["Be brief.", "What is the secret plan of Zorblax?"]
    );
    let wire = r.gw.wire.lock().unwrap().join("\n");
    assert!(
        !wire.contains("Zorblax") && !wire.contains("Be brief"),
        "plaintext on the wire: {wire}"
    );
    let ev = r.route.evidence();
    assert!(ev.tee_verified && !ev.simulated);
    assert_eq!(
        (ev.class.as_str(), ev.model.as_str()),
        ("verified_tee", MODEL)
    );
    let rec = ev.receipt.unwrap();
    assert_eq!(rec["signature_kind"], "provider_tee");
    assert_eq!(rec["signer"], address(MODEL_SEED));
    assert_eq!(rec["e2ee"], true);
    assert_eq!(rec["reply_key_bound"], true);
    assert!(
        rec["signed_text"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{MODEL}:")),
        "{rec}"
    );
    assert_eq!(rec["completion_id"], "chat-0");
    assert_eq!(rec["deployment"]["gateway"]["tcb_status"], "UpToDate");
    assert_eq!(
        rec["deployment"]["gateway"]["signer"],
        address(GATEWAY_SEED)
    );
    assert_eq!(
        rec["deployment"]["models"][0]["signer"],
        address(MODEL_SEED)
    );
}

#[test]
fn nr02_an_answer_signed_by_the_gateway_and_not_the_model_is_held_back() {
    let r = rig(Faults {
        gateway_signs: true,
        ..Faults::default()
    });
    let e = r.route.complete(&ask("hello")).unwrap_err();
    assert!(
        matches!(&e, RouteError::NotVerified(m) if m.contains("signed by the gateway")),
        "{e}"
    );
    assert!(
        r.route.evidence().receipt.is_none(),
        "a refused answer leaves no receipt"
    );
}

#[test]
fn nr03_an_answer_changed_after_it_was_signed_or_signed_by_another_key_is_held_back() {
    for faults in [
        Faults {
            tamper: true,
            ..Faults::default()
        },
        Faults {
            wrong_signer: true,
            ..Faults::default()
        },
    ] {
        let r = rig(faults);
        let e = r.route.complete(&ask("hello")).unwrap_err();
        assert!(matches!(e, RouteError::NotVerified(_)), "{e}");
        assert!(r.route.evidence().receipt.is_none());
    }
}

#[test]
fn nr04_a_deployment_whose_proof_does_not_verify_gets_no_request_at_all() {
    let r = rig(Faults {
        bad_model_report: true,
        ..Faults::default()
    });
    assert!(!r.route.tee_verified());
    let e = r.route.complete(&ask("the private question")).unwrap_err();
    assert!(matches!(e, RouteError::NotVerified(_)), "{e}");
    assert_eq!(
        r.gw.chats.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a chat request was sent to an unverified model"
    );
    assert!(r.gw.wire.lock().unwrap().is_empty());
}

#[test]
fn nr05_a_service_error_is_named_as_what_it_is_and_nothing_is_retried_or_given() {
    for (status, ok) in [
        (401, "Unauthorized"),
        (403, "Unauthorized"),
        (402, "OutOfCredit"),
        (404, "NotFound"),
        (413, "ContextExceeded"),
        (429, "Busy"),
        (503, "Busy"),
    ] {
        let r = rig(Faults {
            chat_status: Some(status),
            ..Faults::default()
        });
        let e = r.route.complete(&ask("hello")).unwrap_err();
        let name = match &e {
            RouteError::Unauthorized(_) => "Unauthorized",
            RouteError::OutOfCredit => "OutOfCredit",
            RouteError::NotFound => "NotFound",
            RouteError::ContextExceeded => "ContextExceeded",
            RouteError::Busy(_) => "Busy",
            other => panic!("{status}: {other}"),
        };
        assert_eq!(name, ok, "{status}");
        assert_eq!(
            r.gw.chats.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "{status}: asked once"
        );
    }
}

#[test]
fn nr06_a_tool_call_comes_back_decrypted_with_its_arguments_read_as_data() {
    let r = rig(Faults {
        tool_call: true,
        ..Faults::default()
    });
    let mut req = ask("read it");
    req.tools = vec![ToolSpec {
        name: "read_file".into(),
        description: "read".into(),
        parameters: json!({"type": "object"}),
    }];
    let c = r.route.complete(&req).unwrap();
    assert_eq!(c.message.content, "");
    assert_eq!(c.message.tool_calls.len(), 1);
    assert_eq!(c.message.tool_calls[0].name, "read_file");
    assert_eq!(c.message.tool_calls[0].arguments, json!({"path": "a.py"}));
}

#[test]
fn nr07_the_kernel_offers_restricted_data_to_this_route_only_while_it_verifies() {
    let good = rig(Faults::default());
    let restricted = Label::new(Level::Restricted).with_compartment("secrets");
    assert_eq!(good.route.class(), RouteClass::VerifiedTee);
    assert!(good.route.reply_key_bound());
    let routes: Vec<Box<dyn Route>> = vec![Box::new(good.route)];
    assert_eq!(
        RouteSet::new(routes)
            .select(&restricted)
            .map(|r| r.id().to_string())
            .unwrap(),
        format!("near:{MODEL}")
    );
    let bad = rig(Faults {
        bad_model_report: true,
        ..Faults::default()
    });
    let routes: Vec<Box<dyn Route>> = vec![Box::new(bad.route)];
    assert!(matches!(
        RouteSet::new(routes).select(&restricted),
        Err(RouteError::NoEligibleRoute)
    ));
}

#[test]
fn nr08_a_route_without_end_to_end_encryption_or_with_a_bad_model_name_cannot_be_made() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let gw = rt.block_on(Gateway::start(Faults::default()));
    let mut o = gw.options();
    o.e2ee = false;
    assert!(matches!(
        NearRoute::with_options(o, MODEL),
        Err(RouteError::NotVerified(_))
    ));
    for name in ["", "a\nb", &"x".repeat(300)] {
        assert!(
            NearRoute::with_options(gw.options(), name).is_err(),
            "{name:?}"
        );
    }
}
