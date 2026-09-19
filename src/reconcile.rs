//! Torn faction money log and war payout reconciliation engine.
//! Pure, deterministic verification matching in-game disbursements to register rows.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static GIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)increased\s+<a[^>]*XID=(\d+)[^>]*>([^<]+)</a>(?:'s|')\s+(?:<a[^>]*>[^<]*</a>\s+)?money balance by \$([0-9,]+)")
        .expect("Failed to compile GIVE_RE regex")
});

static OC_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)cut for their role").expect("Failed to compile OC_MARKER regex")
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDisbursement {
    pub news_id: String,
    pub recipient_torn_id: i64,
    pub recipient_name: Option<String>,
    pub amount: i64,
    pub occurred_at: Option<String>,
    pub raw_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRowForMatch {
    pub id: i64,
    pub torn_id: i64,
    pub amount_owed: i64,
    pub paid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiveForMatch {
    pub news_id: String,
    pub recipient_torn_id: i64,
    pub amount: i64,
    pub occurred_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchDecision {
    pub news_id: String,
    pub recipient_torn_id: i64,
    pub amount: i64,
    pub matched_register_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileSummary {
    pub matched: usize,
    pub unmatched: usize,
    pub scanned: usize,
    pub decisions: Vec<MatchDecision>,
}

fn to_rfc3339(unix: i64) -> Option<String> {
    if unix <= 0 {
        return None;
    }
    chrono::DateTime::from_timestamp(unix, 0).map(|dt| dt.to_rfc3339())
}

/// Parse faction money-log news entries into payout disbursements.
/// Excludes Organized Crime (OC) cuts and non-give money events.
pub fn parse_disbursements(raw_news: &serde_json::Value) -> Vec<ParsedDisbursement> {
    let mut out = Vec::new();

    let items: Vec<&serde_json::Value> = if let Some(arr) = raw_news.as_array() {
        arr.iter().collect()
    } else if let Some(news_arr) = raw_news.get("news").and_then(|n| n.as_array()) {
        news_arr.iter().collect()
    } else if let Some(news_obj) = raw_news.get("news").and_then(|n| n.as_object()) {
        news_obj.values().collect()
    } else if let Some(obj) = raw_news.as_object() {
        obj.values().collect()
    } else {
        Vec::new()
    };

    for item in items {
        let text = item
            .get("text")
            .and_then(|t| t.as_str())
            .or_else(|| item.get("news").and_then(|t| t.as_str()))
            .unwrap_or("");

        if text.is_empty() || OC_MARKER.is_match(text) {
            continue;
        }

        if let Some(caps) = GIVE_RE.captures(text) {
            let recipient_torn_id: i64 = match caps.get(1).map(|m| m.as_str().parse()) {
                Some(Ok(id)) => id,
                _ => continue,
            };

            let recipient_name = caps.get(2).map(|m| m.as_str().to_string());
            let amount_str = caps.get(3).map(|m| m.as_str().replace(',', "")).unwrap_or_default();
            let amount: i64 = match amount_str.parse() {
                Ok(a) => a,
                Err(_) => continue,
            };

            let news_id = item
                .get("id")
                .and_then(|id| {
                    if let Some(s) = id.as_str() {
                        Some(s.to_string())
                    } else {
                        id.as_i64().map(|n| n.to_string())
                    }
                })
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let timestamp = item.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0);
            let occurred_at = to_rfc3339(timestamp);

            out.push(ParsedDisbursement {
                news_id,
                recipient_torn_id,
                recipient_name,
                amount,
                occurred_at,
                raw_text: text.to_string(),
            });
        }
    }

    out
}

/// Match money-log disbursements to unpaid register rows.
/// Real-money safety rules:
/// 1. Never match an already-paid register row.
/// 2. One register row can be claimed by at most one give.
/// 3. Pass 1: exact amount match.
/// 4. Pass 2: within tolerance percentage.
pub fn match_disbursements(
    gives: &[GiveForMatch],
    register: &[RegisterRowForMatch],
    tolerance_pct: f64,
) -> ReconcileSummary {
    let mut by_torn: HashMap<i64, &RegisterRowForMatch> = HashMap::new();
    for r in register {
        by_torn.insert(r.torn_id, r);
    }

    let mut claimed_register_ids: HashSet<i64> = HashSet::new();
    for r in register {
        if r.paid {
            claimed_register_ids.insert(r.id);
        }
    }

    let mut decisions: HashMap<String, MatchDecision> = HashMap::new();
    for g in gives {
        decisions.insert(
            g.news_id.clone(),
            MatchDecision {
                news_id: g.news_id.clone(),
                recipient_torn_id: g.recipient_torn_id,
                amount: g.amount,
                matched_register_id: None,
            },
        );
    }

    let mut pass = |accept: &dyn Fn(&GiveForMatch, &RegisterRowForMatch) -> bool| {
        for g in gives {
            let d = match decisions.get_mut(&g.news_id) {
                Some(entry) => entry,
                None => continue,
            };
            if d.matched_register_id.is_some() {
                continue;
            }

            let r = match by_torn.get(&g.recipient_torn_id) {
                Some(&row) => row,
                None => continue,
            };

            if claimed_register_ids.contains(&r.id) {
                continue;
            }

            if accept(g, r) {
                d.matched_register_id = Some(r.id);
                claimed_register_ids.insert(r.id);
            }
        }
    };

    // Pass 1: exact amount match
    pass(&|g, r| r.amount_owed > 0 && g.amount > 0 && g.amount == r.amount_owed);

    // Pass 2: within tolerance
    pass(&|g, r| {
        if r.amount_owed <= 0 || g.amount <= 0 {
            return false;
        }
        let max_diff = ((r.amount_owed as f64) * tolerance_pct).round() as i64;
        let allowed_diff = max_diff.max(1);
        (g.amount - r.amount_owed).abs() <= allowed_diff
    });

    let mut matched = 0;
    let mut unmatched = 0;
    let mut ordered_decisions = Vec::new();

    for g in gives {
        if let Some(d) = decisions.remove(&g.news_id) {
            if d.matched_register_id.is_some() {
                matched += 1;
            } else {
                unmatched += 1;
            }
            ordered_decisions.push(d);
        }
    }

    ReconcileSummary {
        matched,
        unmatched,
        scanned: gives.len(),
        decisions: ordered_decisions,
    }
}

/// Verify if a give timestamp falls strictly inside the war payout window.
pub fn is_in_war_window(
    occurred_at_ms: Option<i64>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> bool {
    match occurred_at_ms {
        None => to_ms.is_none(),
        Some(ts) => {
            if let Some(from) = from_ms {
                if ts < from {
                    return false;
                }
            }
            if let Some(to) = to_ms {
                if ts >= to {
                    return false;
                }
            }
            true
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayoutGivePick {
    One { give_id: i64, amount: i64 },
    None,
    Ambiguous,
}

/// Pick an exact give for historical closeout with tolerance bounds.
pub fn pick_payout_give(owed: i64, gives: &[(i64, i64)], tolerance: f64) -> PayoutGivePick {
    if owed <= 0 {
        return PayoutGivePick::None;
    }

    let lo = ((owed as f64) * (1.0 - tolerance)).round() as i64;
    let hi = ((owed as f64) * (1.0 + tolerance)).round() as i64;

    let in_band: Vec<&(i64, i64)> = gives
        .iter()
        .filter(|(_, amt)| *amt > 0 && *amt >= lo && *amt <= hi)
        .collect();

    match in_band.len() {
        0 => PayoutGivePick::None,
        1 => PayoutGivePick::One {
            give_id: in_band[0].0,
            amount: in_band[0].1,
        },
        _ => PayoutGivePick::Ambiguous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_disbursements_with_oc_exclusion() {
        let raw_news = json!({
            "news": {
                "1": {
                    "id": "1001",
                    "timestamp": 1720000000,
                    "text": "<a href=\"profiles.php?XID=100\">Officer</a> increased <a href=\"profiles.php?XID=4027928\">TheLastPrimate</a>'s money balance by $50,973,248 from $0 to $50,973,248."
                },
                "2": {
                    "id": "1002",
                    "timestamp": 1720000010,
                    "text": "<a href=\"profiles.php?XID=100\">Officer</a> increased <a href=\"profiles.php?XID=4027928\">TheLastPrimate</a>'s money balance by $1,250,000 as their cut for their role as Planner."
                },
                "3": {
                    "id": "1003",
                    "timestamp": 1720000020,
                    "text": "<a href=\"profiles.php?XID=2532285\">MrRazorlove</a> deposited $10,000,000 to faction vault."
                }
            }
        });

        let parsed = parse_disbursements(&raw_news);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].news_id, "1001");
        assert_eq!(parsed[0].recipient_torn_id, 4027928);
        assert_eq!(parsed[0].recipient_name.as_deref(), Some("TheLastPrimate"));
        assert_eq!(parsed[0].amount, 50973248);
    }

    #[test]
    fn test_match_disbursements_exact_and_tolerance() {
        let gives = vec![
            GiveForMatch {
                news_id: "g1".to_string(),
                recipient_torn_id: 1,
                amount: 1000000,
                occurred_at: None,
            },
            GiveForMatch {
                news_id: "g2".to_string(),
                recipient_torn_id: 2,
                amount: 501000,
                occurred_at: None,
            },
            GiveForMatch {
                news_id: "g3".to_string(),
                recipient_torn_id: 3,
                amount: 250000,
                occurred_at: None,
            },
        ];

        let register = vec![
            RegisterRowForMatch {
                id: 101,
                torn_id: 1,
                amount_owed: 1000000,
                paid: false,
            },
            RegisterRowForMatch {
                id: 102,
                torn_id: 2,
                amount_owed: 500000,
                paid: false,
            },
            RegisterRowForMatch {
                id: 103,
                torn_id: 3,
                amount_owed: 250000,
                paid: true,
            },
        ];

        let res = match_disbursements(&gives, &register, 0.005);
        assert_eq!(res.matched, 2);
        assert_eq!(res.unmatched, 1);

        let g1_match = res.decisions.iter().find(|d| d.news_id == "g1").unwrap();
        assert_eq!(g1_match.matched_register_id, Some(101));

        let g2_match = res.decisions.iter().find(|d| d.news_id == "g2").unwrap();
        assert_eq!(g2_match.matched_register_id, Some(102));

        let g3_match = res.decisions.iter().find(|d| d.news_id == "g3").unwrap();
        assert_eq!(g3_match.matched_register_id, None);
    }

    #[test]
    fn test_is_in_war_window() {
        assert!(is_in_war_window(Some(150), Some(100), Some(200)));
        assert!(!is_in_war_window(Some(90), Some(100), Some(200)));
        assert!(!is_in_war_window(Some(200), Some(100), Some(200)));
        assert!(is_in_war_window(Some(250), Some(100), None));
        assert!(!is_in_war_window(None, Some(100), Some(200)));
        assert!(is_in_war_window(None, Some(100), None));
    }

    #[test]
    fn test_pick_payout_give() {
        let gives = vec![(1, 1000000), (2, 2000000)];
        let pick1 = pick_payout_give(1020000, &gives, 0.1);
        assert_eq!(pick1, PayoutGivePick::One { give_id: 1, amount: 1000000 });

        let pick_ambig = pick_payout_give(1500000, &gives, 0.5);
        assert_eq!(pick_ambig, PayoutGivePick::Ambiguous);

        let pick_none = pick_payout_give(5000000, &gives, 0.1);
        assert_eq!(pick_none, PayoutGivePick::None);
    }
}
