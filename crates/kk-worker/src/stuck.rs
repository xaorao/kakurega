//! Noticing that an agent is going round in circles, from the conversation it has already built.
//!
//! The reading is the whole saved conversation, so it survives a restart of the worker. A "response" is one assistant turn
//! with the tool calls it made and the answers it got. Two shapes are looked for: the same response again and again (the same
//! calls, the same answers), and two responses taking turns. Different answers are progress, however alike the calls: polling a
//! status that changes is not a loop. At a first length the agent is told once, in a notice that does not count as the owner's
//! words; if it carries on to a second length the run ends with a reason, and the state is saved so nothing is lost.

use kk_types::chat::ChatMessage;

/// Responses in a row that are the same before the agent is told.
pub const REPEAT_NUDGE: usize = 3;
/// ...and before the run ends.
pub const REPEAT_STOP: usize = 5;
/// Responses taking turns (A, B, A, B, ...) before the agent is told.
pub const ALTERNATE_NUDGE: usize = 6;
/// ...and before the run ends.
pub const ALTERNATE_STOP: usize = 8;
/// A notice starts with this, so it is told apart from the owner's task and never mistaken for one.
pub const NOTICE_PREFIX: &str = "[notice from the program] ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Fine,
    /// Say this to the agent once and let it try again.
    Nudge(String),
    /// End the run for this reason.
    Stop(String),
}

/// One assistant turn with calls, as what it asked and what it got.
#[derive(PartialEq, Eq, Clone, Debug)]
struct Response {
    calls: Vec<(String, serde_json::Value)>,
    answers: Vec<String>,
}

fn is_notice(m: &ChatMessage) -> bool {
    m.role == "user" && m.content.starts_with(NOTICE_PREFIX)
}

/// The responses in order, and for each one whether a notice came after it. Only assistant turns that made calls count.
fn responses(messages: &[ChatMessage]) -> Vec<(Response, bool)> {
    let mut out: Vec<(Response, bool)> = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let m = &messages[i];
        if m.role == "assistant" && !m.tool_calls.is_empty() {
            let calls = m
                .tool_calls
                .iter()
                .map(|c| (c.name.clone(), c.arguments.clone()))
                .collect();
            let mut answers = Vec::new();
            let mut j = i + 1;
            let mut noticed = false;
            while j < messages.len() && messages[j].role != "assistant" {
                if messages[j].role == "tool" {
                    answers.push(messages[j].content.clone());
                } else if is_notice(&messages[j]) {
                    noticed = true;
                } else if messages[j].role == "user" {
                    // the owner said something new: whatever came before is a different conversation
                    out.clear();
                    noticed = false;
                    answers.clear();
                    break;
                }
                j += 1;
            }
            if !(answers.is_empty() && out.is_empty() && noticed) {
                out.push((Response { calls, answers }, noticed));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// How many of the newest responses are the same as the very last one.
fn repeat_run(rs: &[(Response, bool)]) -> usize {
    let Some((last, _)) = rs.last() else {
        return 0;
    };
    rs.iter().rev().take_while(|(r, _)| r == last).count()
}

/// How many of the newest responses alternate between two different ones.
fn alternate_run(rs: &[(Response, bool)]) -> usize {
    let n = rs.len();
    if n < 2 || rs[n - 1].0 == rs[n - 2].0 {
        return 0;
    }
    let (a, b) = (&rs[n - 2].0, &rs[n - 1].0);
    rs.iter()
        .rev()
        .enumerate()
        .take_while(|(k, (r, _))| r == if k % 2 == 0 { b } else { a })
        .count()
}

fn noticed_within(rs: &[(Response, bool)], len: usize) -> bool {
    rs.iter().rev().take(len).any(|(_, n)| *n)
}

/// Looks at the conversation before the next model call.
pub fn check(messages: &[ChatMessage]) -> Verdict {
    let rs = responses(messages);
    let rep = repeat_run(&rs);
    let alt = alternate_run(&rs);
    if rep >= REPEAT_STOP || alt >= ALTERNATE_STOP {
        return Verdict::Stop(if rep >= REPEAT_STOP {
            format!("the agent made the same call with the same answer {rep} times in a row and did not change course after being told")
        } else {
            format!("the agent went back and forth between two calls with the same answers {alt} times and did not change course after being told")
        });
    }
    if rep >= REPEAT_NUDGE && !noticed_within(&rs, rep) {
        return Verdict::Nudge(format!(
            "{NOTICE_PREFIX}You have made the same call {rep} times and got the same answer each time. Repeating it will not change the answer. Use what you have, try a different call, or finish."
        ));
    }
    if alt >= ALTERNATE_NUDGE && !noticed_within(&rs, alt) {
        return Verdict::Nudge(format!(
            "{NOTICE_PREFIX}You are going back and forth between the same two calls and getting the same answers. Use what you have, try something different, or finish."
        ));
    }
    Verdict::Fine
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_types::chat::ToolCall;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ChatMessage {
        let mut m = ChatMessage::new("assistant", "");
        m.tool_calls = vec![ToolCall {
            id: "c".into(),
            name: name.into(),
            arguments: args,
        }];
        m
    }

    fn answer(text: &str) -> ChatMessage {
        let mut m = ChatMessage::new("tool", text);
        m.tool_call_id = Some("c".into());
        m
    }

    fn convo(pairs: &[(&str, serde_json::Value, &str)]) -> Vec<ChatMessage> {
        let mut v = vec![
            ChatMessage::new("system", "s"),
            ChatMessage::new("user", "task"),
        ];
        for (n, a, r) in pairs {
            v.push(call(n, a.clone()));
            v.push(answer(r));
        }
        v
    }

    fn same(n: usize) -> Vec<ChatMessage> {
        convo(
            &(0..n)
                .map(|_| ("list_files", json!({}), "[]"))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn st01_the_same_call_with_the_same_answer_is_told_at_three_and_stopped_at_five() {
        assert_eq!(check(&same(2)), Verdict::Fine);
        assert!(
            matches!(check(&same(3)), Verdict::Nudge(t) if t.starts_with(NOTICE_PREFIX) && t.contains("3 times"))
        );
        assert!(
            matches!(check(&same(4)), Verdict::Nudge(_)),
            "still not stopped at four while no notice is in"
        );
        assert!(matches!(check(&same(5)), Verdict::Stop(r) if r.contains("5 times")));
        assert!(matches!(check(&same(9)), Verdict::Stop(_)));
    }

    #[test]
    fn st02_after_a_notice_the_agent_is_not_told_again_but_is_stopped_if_it_carries_on() {
        let mut m = same(3);
        let Verdict::Nudge(t) = check(&m) else {
            panic!()
        };
        m.push(ChatMessage::new("user", &t));
        assert_eq!(
            check(&m),
            Verdict::Fine,
            "told once, now it gets its chance"
        );
        m.push(call("list_files", json!({})));
        m.push(answer("[]"));
        assert_eq!(check(&m), Verdict::Fine, "a fourth is allowed");
        m.push(call("list_files", json!({})));
        m.push(answer("[]"));
        assert!(
            matches!(check(&m), Verdict::Stop(_)),
            "the fifth ends the run"
        );
    }

    #[test]
    fn st03_a_different_answer_or_different_arguments_is_progress() {
        let polling = convo(&[
            ("status", json!({}), "running"),
            ("status", json!({}), "running"),
            ("status", json!({}), "done"),
            ("status", json!({}), "done"),
        ]);
        assert_eq!(
            check(&polling),
            Verdict::Fine,
            "the answer changed in between"
        );
        let args = convo(
            &(0..6)
                .map(|i| ("read_file", json!({"path": format!("f{i}")}), "x"))
                .collect::<Vec<_>>(),
        );
        assert_eq!(check(&args), Verdict::Fine, "different arguments each time");
        let mut order = convo(&[
            ("a", json!({"x": 1, "y": 2}), "r"),
            ("a", json!({"y": 2, "x": 1}), "r"),
            ("a", json!({"x": 1, "y": 2}), "r"),
        ]);
        assert!(
            matches!(check(&order), Verdict::Nudge(_)),
            "key order does not make a call different"
        );
        order.truncate(2);
        assert_eq!(check(&order), Verdict::Fine);
    }

    #[test]
    fn st04_two_calls_taking_turns_with_the_same_answers_are_a_loop_too() {
        let pairs = |n: usize| {
            convo(
                &(0..n)
                    .map(|i| {
                        if i % 2 == 0 {
                            ("a", json!({}), "ra")
                        } else {
                            ("b", json!({}), "rb")
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(check(&pairs(5)), Verdict::Fine);
        assert!(matches!(check(&pairs(6)), Verdict::Nudge(t) if t.contains("back and forth")));
        assert!(matches!(check(&pairs(7)), Verdict::Nudge(_)));
        assert!(matches!(check(&pairs(8)), Verdict::Stop(r) if r.contains("back and forth")));
        // told once: not told again while it goes on, stopped at the longer length
        let mut told = pairs(6);
        let Verdict::Nudge(t) = check(&told) else {
            panic!()
        };
        told.push(ChatMessage::new("user", &t));
        assert_eq!(check(&told), Verdict::Fine);
        for i in 6..7 {
            let (n, r) = if i % 2 == 0 { ("a", "ra") } else { ("b", "rb") };
            told.push(call(n, json!({})));
            told.push(answer(r));
        }
        assert_eq!(check(&told), Verdict::Fine, "seven, already told once");
        told.push(call("b", json!({})));
        told.push(answer("rb"));
        assert!(matches!(check(&told), Verdict::Stop(_)), "eight ends it");
        // a third kind of call in the middle breaks it
        let mut broken = pairs(7);
        broken.push(call("c", json!({})));
        broken.push(answer("rc"));
        assert_eq!(check(&broken), Verdict::Fine);
    }

    #[test]
    fn st05_errors_that_repeat_count_like_any_answer() {
        let errs = convo(
            &(0..3)
                .map(|_| ("apply", json!({"p": 1}), "{\"error\":\"NO_SUCH_FILE\"}"))
                .collect::<Vec<_>>(),
        );
        assert!(matches!(check(&errs), Verdict::Nudge(_)));
    }

    #[test]
    fn st06_a_new_message_from_the_owner_starts_over_and_other_kinds_of_turns_do_not_count() {
        let mut m = same(4);
        m.push(ChatMessage::new(
            "user",
            "actually, look at main.rs instead",
        ));
        m.push(call("list_files", json!({})));
        m.push(answer("[]"));
        assert_eq!(
            check(&m),
            Verdict::Fine,
            "after the owner speaks the count restarts"
        );
        assert_eq!(check(&[]), Verdict::Fine);
        assert_eq!(
            check(&[
                ChatMessage::new("system", "s"),
                ChatMessage::new("user", "t"),
                ChatMessage::new("assistant", "thinking")
            ]),
            Verdict::Fine,
            "turns without calls are not responses"
        );
        // a parallel pair of calls is one response and compares as a whole
        let mut two = ChatMessage::new("assistant", "");
        two.tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "a".into(),
                arguments: json!({}),
            },
            ToolCall {
                id: "2".into(),
                name: "b".into(),
                arguments: json!({}),
            },
        ];
        let mut v = vec![ChatMessage::new("user", "t")];
        for _ in 0..3 {
            v.push(two.clone());
            v.push(answer("ra"));
            v.push(answer("rb"));
        }
        assert!(matches!(check(&v), Verdict::Nudge(_)), "{:?}", check(&v));
    }
}
