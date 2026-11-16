//! `workyard status` and `workyard wait`: a one-screen board summary and a blocking check
//! for scripts and agents. Both read the board and orchestrator views; neither writes.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use workyard_core::{Error, Result, Store};

/// What `wait` is waiting for.
#[derive(Debug, PartialEq)]
pub enum Until {
    Attention,
    Done,
    /// This many more cards in a done lane than when the wait began.
    MoreDone(usize),
}

pub fn parse_until(text: &str) -> Result<Until> {
    match text {
        "attention" => Ok(Until::Attention),
        "done" => Ok(Until::Done),
        other => other
            .strip_suffix("-done")
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .map(Until::MoreDone)
            .ok_or_else(|| Error::new("validation_error", format!("--until must be attention, done or <n>-done, got '{other}'"))),
    }
}

/// `90`, `90s`, `30m`, `2h`.
pub fn parse_duration(text: &str) -> Result<Duration> {
    let (digits, unit) = match text.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => text.split_at(i),
        None => (text, "s"),
    };
    let n: u64 = digits
        .parse()
        .map_err(|_| Error::new("validation_error", format!("bad duration '{text}'; use 90s, 30m or 2h")))?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return Err(Error::new("validation_error", format!("bad duration '{text}'; use 90s, 30m or 2h"))),
    };
    Ok(Duration::from_secs(n * mult))
}

fn cards<'a>(board: &'a Value) -> impl Iterator<Item = (&'a Value, &'a Value)> {
    board["lanes"].as_array().into_iter().flatten().flat_map(|lane| {
        lane["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["cancelled_at"].is_null())
            .map(move |c| (lane, c))
    })
}

/// Everything `status` and `wait` need, in one value.
pub fn snapshot(store: &Store, board: &str) -> Result<Value> {
    let view = store.view_board_filtered(board, &Default::default())?;
    let orch = store.orchestrator_status(board).unwrap_or(Value::Null);
    let questions = store.list_questions_filtered(Some(board), Some("open"), None)?;
    let mut lanes = vec![];
    for lane in view["lanes"].as_array().into_iter().flatten() {
        if lane["archived"] == true {
            continue;
        }
        let mut out = vec![];
        for card in lane["cards"].as_array().into_iter().flatten().filter(|c| c["cancelled_at"].is_null()) {
            let session = &card["session"];
            let state = if lane["kind"] != "active" || card["owner_actor"].is_null() {
                Value::Null
            } else if session.is_null() {
                json!("detached")
            } else if session["paused"] == true {
                json!("paused")
            } else {
                json!("running")
            };
            out.push(json!({
                "key": card["key"], "title": card["title"], "progress": card["progress"],
                "owner": card["owner_actor"], "state": state, "stale": card["stale"],
                "last_activity_at": session["activity_at"].as_str().map(Value::from).unwrap_or_else(|| card["updated_at"].clone()),
                "activity": session["activity"], "open_questions": card["open_question_count"],
                "blocked": card["blocked"],
            }));
        }
        lanes.push(json!({"name": lane["name"], "kind": lane["kind"], "cards": out}));
    }
    let open: Vec<Value> = questions
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| json!({"key": q["key"], "title": q["title"], "owner": q["owner"]}))
        .collect();
    let backing_off = orch["backing_off"] == true;
    let mut snap = json!({
        "board": view["key"], "name": view["name"], "lanes": lanes, "open_questions": open,
        "orchestrator": {
            "present": !orch.is_null(),
            "state": orch["desired_state"], "connected": orch["connected"], "pool_size": orch["pool_size"],
            "backing_off": backing_off, "backoff_until": orch["backoff_until"], "backoff_reason": orch["backoff_reason"],
        },
    });
    snap["attention"] = json!(attention(&snap, &view, &orch));
    Ok(snap)
}

/// Reasons a human or supervising agent should look now.
fn attention(snap: &Value, view: &Value, orch: &Value) -> Vec<String> {
    let mut why = vec![];
    for q in snap["open_questions"].as_array().into_iter().flatten() {
        why.push(format!("open question {}: {}", q["key"].as_str().unwrap_or("?"), q["title"].as_str().unwrap_or("")));
    }
    if orch["backing_off"] == true {
        why.push(format!(
            "orchestrator backing off until {}: {}",
            orch["backoff_until"].as_str().unwrap_or("?"),
            orch["backoff_reason"].as_str().unwrap_or("no reason given")
        ));
    }
    // A board that never had an orchestrator has no holder; one whose lease lapsed does.
    if orch["holder"].is_string() && orch["desired_state"] == "running" && orch["connected"] == false {
        why.push(format!("orchestrator {} stopped: its lease expired", orch["holder"].as_str().unwrap_or("?")));
    }
    for lane in snap["lanes"].as_array().into_iter().flatten() {
        for card in lane["cards"].as_array().into_iter().flatten() {
            let key = card["key"].as_str().unwrap_or("?");
            if card["state"] == "detached" {
                why.push(format!("{key} is detached: claimed but has no session"));
            } else if card["stale"] == true {
                why.push(format!("{key} is stale: its owner has been silent"));
            }
        }
    }
    for (_, card) in cards(view) {
        if card["session"]["waiting_review"] == true {
            why.push(format!("{} is waiting for review", card["key"].as_str().unwrap_or("?")));
        }
    }
    why
}

fn done_count(snap: &Value) -> (usize, usize) {
    let (mut done, mut all) = (0, 0);
    for lane in snap["lanes"].as_array().into_iter().flatten() {
        let n = lane["cards"].as_array().map_or(0, |c| c.len());
        all += n;
        if lane["kind"] == "done" {
            done += n;
        }
    }
    (done, all)
}

/// Check the condition against a snapshot; `Some(reason)` when it holds.
pub fn check(snap: &Value, until: &Until, baseline_done: usize) -> Option<String> {
    let attention = snap["attention"].as_array().cloned().unwrap_or_default();
    let (done, all) = done_count(snap);
    match until {
        // Attention also ends the other waits: a stuck run will not finish by itself.
        Until::Attention => attention.first().map(|_| attention.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")),
        Until::Done | Until::MoreDone(_) if !attention.is_empty() => {
            Some(format!("attention needed: {}", attention.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")))
        }
        Until::Done if all > 0 && done == all => Some(format!("all {all} cards are done")),
        Until::MoreDone(n) if done >= baseline_done + n => Some(format!("{} more cards done ({done} of {all})", done - baseline_done)),
        _ => None,
    }
}

/// Block until the condition holds. `Ok(Some(reason))` when it does, `Ok(None)` on timeout.
pub fn wait(store: &Store, board: &str, until: &Until, timeout: Option<Duration>) -> Result<Option<String>> {
    let start = Instant::now();
    let baseline = done_count(&snapshot(store, board)?).0;
    loop {
        let marker = store.tick_marker();
        let snap = snapshot(store, board)?;
        if let Some(reason) = check(&snap, until, baseline) {
            return Ok(Some(reason));
        }
        let left = match timeout {
            Some(t) => match t.checked_sub(start.elapsed()) {
                Some(left) => left,
                None => return Ok(None),
            },
            None => Duration::from_secs(2),
        };
        // Wake on any commit, and at least every few seconds for the clock-based checks.
        store.wait_for_tick(&marker, left.min(Duration::from_secs(5)));
    }
}

pub fn render(snap: &Value) -> String {
    let mut out = format!("{} {}\n", snap["board"].as_str().unwrap_or(""), snap["name"].as_str().unwrap_or(""));
    let o = &snap["orchestrator"];
    if o["present"] == true {
        let mut line = format!(
            "orchestrator: {}{} pool {}",
            o["state"].as_str().unwrap_or("?"),
            if o["connected"] == true { "" } else { " (not connected)" },
            o["pool_size"]
        );
        if o["backing_off"] == true {
            line += &format!(
                ", backing off until {}: {}",
                o["backoff_until"].as_str().unwrap_or("?"),
                o["backoff_reason"].as_str().unwrap_or("")
            );
        }
        out += &line;
    } else {
        out += "orchestrator: none";
    }
    out += "\n";
    for lane in snap["lanes"].as_array().into_iter().flatten() {
        let cards = lane["cards"].as_array().cloned().unwrap_or_default();
        out += &format!("\n{} ({})\n", lane["name"].as_str().unwrap_or(""), cards.len());
        // Finished work is a count, not a list, so the screen stays one page.
        if lane["kind"] == "done" {
            continue;
        }
        for c in cards {
            let mut line = format!(
                "  {:<5} {:>3}%  {:<8} {}",
                c["key"].as_str().unwrap_or(""),
                c["progress"].as_i64().unwrap_or(0),
                c["state"].as_str().unwrap_or(if c["blocked"] == true { "blocked" } else { "" }),
                c["title"].as_str().unwrap_or("")
            );
            if c["stale"] == true {
                line += " [stale]";
            }
            if c["open_questions"].as_i64().unwrap_or(0) > 0 {
                line += &format!(" [{} open question(s)]", c["open_questions"]);
            }
            if let Some(at) = c["last_activity_at"].as_str() {
                line += &format!("  @{}", at.get(..19).unwrap_or(at));
            }
            out += &line;
            out += "\n";
        }
    }
    let why: Vec<&str> = snap["attention"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if why.is_empty() {
        out += "\nNothing needs attention.\n";
    } else {
        out += "\nNeeds attention:\n";
        for w in why {
            out += &format!("  - {w}\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conditions_and_durations() {
        assert_eq!(parse_until("attention").unwrap(), Until::Attention);
        assert_eq!(parse_until("3-done").unwrap(), Until::MoreDone(3));
        assert!(parse_until("0-done").is_err());
        assert!(parse_until("soon").is_err());
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert!(parse_duration("x").is_err());
    }

    #[test]
    fn check_reports_why() {
        let snap = json!({
            "lanes": [{"kind": "active", "cards": [{}]}, {"kind": "done", "cards": [{}, {}]}],
            "attention": [],
        });
        assert_eq!(check(&snap, &Until::Attention, 0), None);
        assert!(check(&snap, &Until::MoreDone(2), 0).unwrap().contains("2 more"));
        assert_eq!(check(&snap, &Until::MoreDone(2), 1), None);
        assert_eq!(check(&snap, &Until::Done, 0), None);
        let all = json!({"lanes": [{"kind": "done", "cards": [{}]}], "attention": []});
        assert!(check(&all, &Until::Done, 0).unwrap().contains("all 1"));
        let stuck = json!({"lanes": [], "attention": ["open question Q-1: which db"]});
        assert!(check(&stuck, &Until::Attention, 0).unwrap().contains("Q-1"));
        assert!(check(&stuck, &Until::Done, 0).unwrap().contains("attention"));
    }
}
