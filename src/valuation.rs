//! MobWorth: Pure faction valuation engine in native Rust.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValuationBreakdown {
    pub respect_value: i64,
    pub faction_cash_value: i64,
    pub armory_value: i64,
    pub total_value: i64,
}

fn coerce_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Parse faction respect from Torn v2 API payload.
pub fn parse_faction_respect(data: &Value) -> Option<i64> {
    let raw = data
        .get("basic")
        .and_then(|b| b.get("respect"))
        .or_else(|| data.get("respect"));

    let num = raw.and_then(coerce_number)?;
    if num < 0.0 {
        return None;
    }
    Some(num.round() as i64)
}

/// Parse faction's OWN spendable vault cash.
/// Torn balance endpoint reports  as total held in vault,
/// and  as member deposits.
/// Spendable faction cash is vault total minus member deposits.
pub fn parse_faction_cash(data: &Value) -> Option<i64> {
    let balance = data.get("balance")?;
    let faction_money = balance
        .get("faction")
        .and_then(|f| f.get("money"))
        .and_then(coerce_number)?;

    let mut member_deposits = 0.0;
    if let Some(members) = balance.get("members").and_then(|m| m.as_array()) {
        for m in members {
            if let Some(owed) = m.get("money").and_then(coerce_number) {
                member_deposits += owed;
            }
        }
    }

    let net = (faction_money - member_deposits).max(0.0);
    Some(net.round() as i64)
}

/// Computes overall faction valuation.
/// Formula: Respect * Rate + Armory + Faction Net Cash
pub fn compute_valuation(
    respect: f64,
    respect_rate: f64,
    armory_value: f64,
    faction_cash: f64,
) -> ValuationBreakdown {
    let r = respect.max(0.0);
    let rate = respect_rate.max(0.0);
    let armory = armory_value.max(0.0).round() as i64;
    let cash = faction_cash.max(0.0).round() as i64;
    let respect_val = (r * rate).round() as i64;

    ValuationBreakdown {
        respect_value: respect_val,
        faction_cash_value: cash,
        armory_value: armory,
        total_value: respect_val + armory + cash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_compute_valuation() {
        let v = compute_valuation(10000.0, 180284.0, 50000000.0, 25000000.0);
        assert_eq!(v.respect_value, 1802840000);
        assert_eq!(v.armory_value, 50000000);
        assert_eq!(v.faction_cash_value, 25000000);
        assert_eq!(v.total_value, 1802840000 + 50000000 + 25000000);
    }

    #[test]
    fn test_parse_faction_cash() {
        let json_data = json!({
            "balance": {
                "faction": { "money": 1000000 },
                "members": [
                    { "id": 1, "money": 300000 },
                    { "id": 2, "money": 200000 }
                ]
            }
        });
        let cash = parse_faction_cash(&json_data).unwrap();
        assert_eq!(cash, 500000);
    }
}
