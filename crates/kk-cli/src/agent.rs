use serde_json::{json, Value};

const DEFAULT_USES: i64 = 3;
const DEFAULT_MINUTES: i64 = 60;

/// `grant TOOL [--uses N] [--minutes M]`: the tool and the limits, with the defaults the refusal message suggests.
fn parse_grant(args: &[String]) -> Result<(String, i64, i64), String> {
    let tool = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or_else(usage_grant)?
        .clone();
    if !kk_service::LEASABLE_TOOLS.contains(&tool.as_str()) {
        return Err(format!(
            "a lease can cover {} only",
            kk_service::LEASABLE_TOOLS.join(" and ")
        ));
    }
    let number = |name: &str, default: i64| -> Result<i64, String> {
        match crate::flag(args, name) {
            None => Ok(default),
            Some(v) => v
                .parse::<i64>()
                .map_err(|_| format!("{name} needs a whole number, not `{v}`")),
        }
    };
    Ok((
        tool,
        number("--uses", DEFAULT_USES)?,
        number("--minutes", DEFAULT_MINUTES)?,
    ))
}

fn usage_grant() -> String {
    format!(
        "usage: kaku agent grant {} [--uses N] [--minutes M]",
        kk_service::LEASABLE_TOOLS.join("|")
    )
}

fn describe(lease: &Value, now: i64) -> String {
    let left = lease["expires_at"].as_i64().unwrap_or(now) - now;
    let minutes = (left + 59) / 60;
    format!(
        "  {}: {} use(s) left, {} minute(s) remaining",
        lease["tool"].as_str().unwrap_or("?"),
        lease["remaining"].as_i64().unwrap_or(0),
        minutes.max(0)
    )
}

pub fn run(args: &[String]) -> u8 {
    let rest = args.get(1..).unwrap_or(&[]);
    if !matches!(
        args.first().map(String::as_str),
        Some("grant" | "revoke" | "status")
    ) {
        eprintln!("usage: kaku agent grant TOOL [--uses N] [--minutes M] | revoke [TOOL] | status");
        return 2;
    }
    let grant = if args.first().map(String::as_str) == Some("grant") {
        match parse_grant(rest) {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    } else {
        None
    };
    let admin = match crate::approve::connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    match args.first().map(String::as_str) {
        Some("grant") => {
            let (tool, uses, minutes) = grant.expect("parsed above");
            match admin.post(
                "/admin/agent-access/grant",
                json!({"tool": tool, "uses": uses, "minutes": minutes}),
            ) {
                Ok((200, _)) => {
                    println!("An agent started with `kaku mcp --leased` may now use {tool} {uses} time(s) in the next {minutes} minute(s).");
                    println!("Revoke it at any time with: kaku agent revoke {tool}");
                    0
                }
                Ok((_, v)) => {
                    eprintln!(
                        "{}",
                        v["error"]["message"]
                            .as_str()
                            .unwrap_or("the service refused")
                    );
                    1
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Some("revoke") => {
            let tool = rest.first().filter(|a| !a.starts_with("--")).cloned();
            if let Some(t) = &tool {
                if !kk_service::LEASABLE_TOOLS.contains(&t.as_str()) {
                    eprintln!(
                        "a lease can cover {} only",
                        kk_service::LEASABLE_TOOLS.join(" and ")
                    );
                    return 2;
                }
            }
            match admin.post("/admin/agent-access/revoke", json!({"tool": tool})) {
                Ok((200, v)) => {
                    let n = v["revoked"].as_i64().unwrap_or(0);
                    println!(
                        "{}",
                        if n > 0 {
                            format!("Revoked {n} lease(s). The agent's next call is refused.")
                        } else {
                            "There was no lease to revoke.".to_string()
                        }
                    );
                    0
                }
                other => {
                    eprintln!("could not revoke: {other:?}");
                    1
                }
            }
        }
        _ => match admin.get("/admin/agent-access") {
            Ok((200, v)) => {
                let now = v["now"].as_i64().unwrap_or(0);
                let leases = v["leases"].as_array().cloned().unwrap_or_default();
                if leases.is_empty() {
                    println!("No lease is active. An agent started with `kaku mcp --leased` cannot start reviews or ask for approvals.");
                } else {
                    println!("Active leases for an agent started with `kaku mcp --leased`:");
                    for l in &leases {
                        println!("{}", describe(l, now));
                    }
                }
                0
            }
            other => {
                eprintln!("could not read the leases: {other:?}");
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ag01_a_grant_names_a_tool_that_acts_and_takes_defaults_or_numbers() {
        assert_eq!(
            parse_grant(&a(&["start_review"])).unwrap(),
            ("start_review".into(), 3, 60)
        );
        assert_eq!(
            parse_grant(&a(&["request_approval", "--uses", "10", "--minutes", "5"])).unwrap(),
            ("request_approval".into(), 10, 5)
        );
        for bad in [
            a(&[]),
            a(&["--uses", "3"]),
            a(&["overview"]),
            a(&["start_review", "--uses", "many"]),
            a(&["start_review", "--minutes", "1.5"]),
        ] {
            assert!(parse_grant(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ag02_the_time_left_rounds_up_and_never_goes_negative() {
        let l = json!({"tool": "start_review", "remaining": 2, "expires_at": 1000});
        assert!(describe(&l, 1000 - 61).contains("2 minute(s)"));
        assert!(describe(&l, 1000 - 60).contains("1 minute(s)"));
        assert!(describe(&l, 1000 - 1).contains("1 minute(s)"));
        assert!(describe(&l, 5000).contains("0 minute(s)"));
        assert!(describe(&l, 0).contains("2 use(s) left"));
    }
}
