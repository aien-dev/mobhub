//! MobPay payout engine in native Rust.
//! Pure, deterministic, unit-tested against real Torn ranked war data.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayoutModel {
    FlatRate,
    PoolShare,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutPolicy {
    pub model: PayoutModel,
    pub rate_per_respect: f64,
    pub rate_per_outside_hit: f64,
    pub rate_per_assist: f64,
    pub weight_war_hit: f64,
    pub weight_assist: f64,
    pub weight_outside_hit: f64,
    pub split_members_pct: f64,
    pub split_faction_pct: f64,
    pub split_leadership_pct: f64,
    pub leadership_recipient_torn_id: Option<i64>,
    #[serde(default)]
    pub use_fixed_respect_rate: Option<bool>,
    #[serde(default)]
    pub leadership_also_gets_member_pay: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PayoutOptions {
    pub flat_bonus_per_member: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInput {
    pub torn_id: i64,
    pub name: Option<String>,
    pub flat_respect: f64,
    pub war_hits: i64,
    pub assists: i64,
    pub outside_hits: i64,
    pub is_leadership: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberPayout {
    pub torn_id: i64,
    pub name: Option<String>,
    pub kind: String,
    pub flat_respect: f64,
    pub war_hits: i64,
    pub assists: i64,
    pub outside_hits: i64,
    pub payable_hits: f64,
    pub share: f64,
    pub base_amount: i64,
    pub leadership_bonus: i64,
    pub flat_bonus: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutResult {
    pub model: PayoutModel,
    pub cache_value: i64,
    pub members_pool: i64,
    pub faction_fund: i64,
    pub leadership_fund: i64,
    pub derived_rate_per_respect: Option<f64>,
    pub total_flat_respect: f64,
    pub total_payable_hits: f64,
    pub bonus_total: i64,
    pub flat_bonus_per_member: i64,
    pub flat_bonus_total: i64,
    pub members: Vec<MemberPayout>,
    pub member_payout_total: i64,
    pub leadership_paid: i64,
    pub grand_total: i64,
    pub unallocated: i64,
    pub warnings: Vec<String>,
}

pub fn payable_hits_of(m: &MemberInput, p: &PayoutPolicy) -> f64 {
    (m.war_hits as f64) * p.weight_war_hit
        + (m.assists as f64) * p.weight_assist
        + (m.outside_hits as f64) * p.weight_outside_hit
}

fn round_to_i64(n: f64) -> i64 {
    n.round() as i64
}

/// Pure deterministic payout engine.
pub fn compute_payout(
    cache_value: i64,
    policy: &PayoutPolicy,
    members_input: &[MemberInput],
    options: &PayoutOptions,
) -> PayoutResult {
    let mut warnings = Vec::new();

    let raw_bonus = options.flat_bonus_per_member.unwrap_or(0);
    let flat_bonus_per_member = raw_bonus.max(0);

    let split_sum = policy.split_members_pct + policy.split_faction_pct + policy.split_leadership_pct;
    if (split_sum - 1.0).abs() > 1e-4 {
        warnings.push(format!(
            "Splits sum to {:.4}, not 1.0; check policy configuration.",
            split_sum
        ));
    }

    let faction_fund = round_to_i64((cache_value as f64) * policy.split_faction_pct);
    let leadership_fund = round_to_i64((cache_value as f64) * policy.split_leadership_pct);
    let members_pool = (cache_value as f64) * policy.split_members_pct;

    let recipient_id = if policy.split_leadership_pct > 0.0 {
        policy.leadership_recipient_torn_id
    } else {
        None
    };

    if policy.split_leadership_pct > 0.0 && recipient_id.is_none() {
        warnings.push("Leadership split > 0 but no leadership recipient is configured.".to_string());
    }

    let leader_shares_pool = recipient_id.is_some() && policy.leadership_also_gets_member_pay.unwrap_or(false);
    let is_bought_out = |tid: i64| -> bool {
        recipient_id == Some(tid) && !leader_shares_pool
    };

    let dist_members: Vec<&MemberInput> = members_input
        .iter()
        .filter(|m| !is_bought_out(m.torn_id))
        .collect();

    let leadership_member = recipient_id.and_then(|rid| members_input.iter().find(|m| m.torn_id == rid));

    let mut out: Vec<MemberPayout> = Vec::new();
    let total_payable_hits: f64 = dist_members.iter().map(|m| payable_hits_of(m, policy)).sum();
    let mut total_flat_respect: f64 = 0.0;
    let mut bonus_total: i64 = 0;
    let mut derived_rate_per_respect: Option<f64> = None;

    if policy.model == PayoutModel::FlatRate {
        let b_total_f64: f64 = dist_members
            .iter()
            .map(|m| {
                (m.outside_hits as f64) * policy.rate_per_outside_hit
                    + (m.assists as f64) * policy.rate_per_assist
            })
            .sum();
        bonus_total = round_to_i64(b_total_f64);
        total_flat_respect = dist_members.iter().map(|m| m.flat_respect).sum();

        let rate = if policy.use_fixed_respect_rate.unwrap_or(false) {
            policy.rate_per_respect
        } else {
            let respect_pool = members_pool - (bonus_total as f64);
            if respect_pool < 0.0 {
                warnings.push("Outside/assist bonuses exceed member pool; per-respect rate set to 0.".to_string());
                0.0
            } else if total_flat_respect > 0.0 {
                respect_pool / total_flat_respect
            } else {
                0.0
            }
        };
        derived_rate_per_respect = Some(rate);

        for m in &dist_members {
            let base = round_to_i64(
                m.flat_respect * rate
                    + (m.outside_hits as f64) * policy.rate_per_outside_hit
                    + (m.assists as f64) * policy.rate_per_assist,
            );
            out.push(MemberPayout {
                torn_id: m.torn_id,
                name: m.name.clone(),
                kind: "member".to_string(),
                flat_respect: m.flat_respect,
                war_hits: m.war_hits,
                assists: m.assists,
                outside_hits: m.outside_hits,
                payable_hits: payable_hits_of(m, policy),
                share: 0.0,
                base_amount: base,
                leadership_bonus: 0,
                flat_bonus: 0,
                amount: base,
            });
        }
    } else {
        for m in &dist_members {
            let ph = payable_hits_of(m, policy);
            let share = if total_payable_hits > 0.0 {
                ph / total_payable_hits
            } else {
                0.0
            };
            let base = round_to_i64(members_pool * share);
            out.push(MemberPayout {
                torn_id: m.torn_id,
                name: m.name.clone(),
                kind: "member".to_string(),
                flat_respect: m.flat_respect,
                war_hits: m.war_hits,
                assists: m.assists,
                outside_hits: m.outside_hits,
                payable_hits: ph,
                share,
                base_amount: base,
                leadership_bonus: 0,
                flat_bonus: 0,
                amount: base,
            });
        }
    }

    let mut leadership_paid = 0;
    if let Some(rid) = recipient_id {
        leadership_paid = leadership_fund;
        if leader_shares_pool {
            if let Some(existing) = out.iter_mut().find(|r| r.torn_id == rid) {
                existing.kind = "leadership".to_string();
                existing.leadership_bonus = leadership_fund;
                existing.amount += leadership_fund;
            } else {
                out.push(MemberPayout {
                    torn_id: rid,
                    name: leadership_member.and_then(|m| m.name.clone()),
                    kind: "leadership".to_string(),
                    flat_respect: leadership_member.map(|m| m.flat_respect).unwrap_or(0.0),
                    war_hits: leadership_member.map(|m| m.war_hits).unwrap_or(0),
                    assists: leadership_member.map(|m| m.assists).unwrap_or(0),
                    outside_hits: leadership_member.map(|m| m.outside_hits).unwrap_or(0),
                    payable_hits: 0.0,
                    share: 0.0,
                    base_amount: 0,
                    leadership_bonus: leadership_fund,
                    flat_bonus: 0,
                    amount: leadership_fund,
                });
            }
        } else {
            out.push(MemberPayout {
                torn_id: rid,
                name: leadership_member.and_then(|m| m.name.clone()),
                kind: "leadership".to_string(),
                flat_respect: leadership_member.map(|m| m.flat_respect).unwrap_or(0.0),
                war_hits: leadership_member.map(|m| m.war_hits).unwrap_or(0),
                assists: leadership_member.map(|m| m.assists).unwrap_or(0),
                outside_hits: leadership_member.map(|m| m.outside_hits).unwrap_or(0),
                payable_hits: 0.0,
                share: 0.0,
                base_amount: 0,
                leadership_bonus: leadership_fund,
                flat_bonus: 0,
                amount: leadership_fund,
            });
        }
    }

    let mut flat_bonus_total = 0;
    if flat_bonus_per_member > 0 {
        for r in &mut out {
            if r.war_hits <= 0 {
                continue;
            }
            r.flat_bonus = flat_bonus_per_member;
            r.amount += flat_bonus_per_member;
            flat_bonus_total += flat_bonus_per_member;
        }
    }

    let member_payout_total: i64 = out
        .iter()
        .filter(|m| m.kind == "member")
        .map(|m| m.amount)
        .sum();
    let pool_distributed: i64 = out.iter().map(|m| m.base_amount).sum();
    let grand_total: i64 = out.iter().map(|m| m.amount).sum::<i64>() + faction_fund;
    let unallocated = round_to_i64(members_pool) - pool_distributed;

    PayoutResult {
        model: policy.model,
        cache_value,
        members_pool: round_to_i64(members_pool),
        faction_fund,
        leadership_fund,
        derived_rate_per_respect,
        total_flat_respect,
        total_payable_hits,
        bonus_total,
        flat_bonus_per_member,
        flat_bonus_total,
        members: out,
        member_payout_total,
        leadership_paid,
        grand_total,
        unallocated,
        warnings,
    }
}

/// Generates direct link to Torn give-money control panel.
pub fn make_give_money_url(_faction_id: i64, recipient_id: i64, amount: i64) -> String {
    format!(
        "https://www.torn.com/factions.php?step=your#/tab=controls&giveMoneyTo={}&amount={}",
        recipient_id, amount
    )
}

/// Exports payout result to standard CSV format.
pub fn export_payout_csv(result: &PayoutResult, faction_id: i64) -> String {
    let mut wtr = csv::Writer::from_writer(vec![]);
    let _ = wtr.write_record(&[
        "Torn ID",
        "Name",
        "Role",
        "Flat Respect",
        "War Hits",
        "Assists",
        "Outside Hits",
        "Payable Hits",
        "Base Amount",
        "Leadership Bonus",
        "Flat Bonus",
        "Total Amount",
        "Give Money Link",
    ]);

    for m in &result.members {
        let name_str = m.name.clone().unwrap_or_default();
        let link = make_give_money_url(faction_id, m.torn_id, m.amount);
        let _ = wtr.write_record(&[
            m.torn_id.to_string(),
            name_str,
            m.kind.clone(),
            format!("{:.2}", m.flat_respect),
            m.war_hits.to_string(),
            m.assists.to_string(),
            m.outside_hits.to_string(),
            format!("{:.2}", m.payable_hits),
            m.base_amount.to_string(),
            m.leadership_bonus.to_string(),
            m.flat_bonus.to_string(),
            m.amount.to_string(),
            link,
        ]);
    }

    String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GanjaMember {
        torn_id: i64,
        name: &'static str,
        war_hits: i64,
        assists: i64,
        outside_hits: i64,
        flat_respect: f64,
        expected_payout: i64,
    }

    const GANJA: &[GanjaMember] = &[
        GanjaMember { torn_id: 2532285, name: "MrRazorlove", war_hits: 55, assists: 0, outside_hits: 1, flat_respect: 413.12, expected_payout: 74978973 },
        GanjaMember { torn_id: 4187899, name: "BabyBubbles", war_hits: 26, assists: 0, outside_hits: 0, flat_respect: 180.5, expected_payout: 32541282 },
        GanjaMember { torn_id: 3455459, name: "MrLoki", war_hits: 33, assists: 0, outside_hits: 0, flat_respect: 168.01, expected_payout: 30289534 },
        GanjaMember { torn_id: 4111834, name: "OrangeGutans", war_hits: 1, assists: 0, outside_hits: 18, flat_respect: 85.05, expected_payout: 24333164 },
        GanjaMember { torn_id: 3015468, name: "Booty_Warrior1", war_hits: 21, assists: 0, outside_hits: 5, flat_respect: 111.98, expected_payout: 22688215 },
        GanjaMember { torn_id: 1713646, name: "Dragonevotv", war_hits: 21, assists: 0, outside_hits: 2, flat_respect: 85.07, expected_payout: 16336769 },
        GanjaMember { torn_id: 4052511, name: "YaGirlKiKi04", war_hits: 18, assists: 0, outside_hits: 0, flat_respect: 85.65, expected_payout: 15441334 },
        GanjaMember { torn_id: 4115047, name: "Roffo", war_hits: 12, assists: 3, outside_hits: 1, flat_respect: 75.02, expected_payout: 14774914 },
        GanjaMember { torn_id: 4027928, name: "TheLastPrimate", war_hits: 10, assists: 0, outside_hits: 0, flat_respect: 77.0, expected_payout: 13881877 },
        GanjaMember { torn_id: 3955191, name: "Sibbett", war_hits: 11, assists: 0, outside_hits: 0, flat_respect: 54.49, expected_payout: 9823681 },
        GanjaMember { torn_id: 3556466, name: "babyYoda10", war_hits: 10, assists: 0, outside_hits: 0, flat_respect: 44.26, expected_payout: 7979375 },
        GanjaMember { torn_id: 4113799, name: "LTsuqmadiq", war_hits: 6, assists: 1, outside_hits: 0, flat_respect: 40.41, expected_payout: 7535281 },
        GanjaMember { torn_id: 4154259, name: "killibeats", war_hits: 5, assists: 0, outside_hits: 2, flat_respect: 35.34, expected_payout: 7371241 },
        GanjaMember { torn_id: 3955301, name: "helpmelive", war_hits: 2, assists: 0, outside_hits: 0, flat_respect: 9.01, expected_payout: 1624360 },
        GanjaMember { torn_id: 3706442, name: "MAJI_THE_VAGI", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 3456295, name: "ArthurShellby", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 3612877, name: "MrK0RG", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 4129627, name: "magnat_88", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 2811446, name: "Morphis0912", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 4117849, name: "Russ012", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 2990570, name: "BougarniN", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 3865277, name: "Vito", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
        GanjaMember { torn_id: 3997892, name: "VANSH01", war_hits: 0, assists: 0, outside_hits: 0, flat_respect: 0.0, expected_payout: 0 },
    ];

    fn default_flat_policy() -> PayoutPolicy {
        PayoutPolicy {
            model: PayoutModel::FlatRate,
            rate_per_respect: 180284.0,
            rate_per_outside_hit: 500000.0,
            rate_per_assist: 250000.0,
            weight_war_hit: 1.0,
            weight_assist: 0.5,
            weight_outside_hit: 0.5,
            split_members_pct: 0.8,
            split_faction_pct: 0.2,
            split_leadership_pct: 0.0,
            leadership_recipient_torn_id: None,
            use_fixed_respect_rate: None,
            leadership_also_gets_member_pay: None,
        }
    }

    #[test]
    fn test_ganja_warriors_ground_truth() {
        let members: Vec<MemberInput> = GANJA
            .iter()
            .map(|m| MemberInput {
                torn_id: m.torn_id,
                name: Some(m.name.to_string()),
                flat_respect: m.flat_respect,
                war_hits: m.war_hits,
                assists: m.assists,
                outside_hits: m.outside_hits,
                is_leadership: None,
            })
            .collect();

        let r = compute_payout(349499999, &default_flat_policy(), &members, &PayoutOptions::default());

        let rate = r.derived_rate_per_respect.unwrap();
        assert!(rate > 180283.0 && rate < 180285.0, "derived rate was {}", rate);
        assert_eq!(r.faction_fund, 69900000);
        assert!((r.member_payout_total - r.members_pool).abs() < 50);

        for gm in GANJA {
            let got = r.members.iter().find(|m| m.torn_id == gm.torn_id).unwrap();
            assert!(
                (got.amount - gm.expected_payout).abs() < 2000,
                "mismatch for {}: got {}, expected {}",
                gm.name,
                got.amount,
                gm.expected_payout
            );
        }

        let zero = r.members.iter().find(|m| m.torn_id == 3706442).unwrap();
        assert_eq!(zero.amount, 0);
        assert!((r.grand_total - 349499999).abs() < 50);
    }

    #[test]
    fn test_pool_share_model() {
        let mut policy = default_flat_policy();
        policy.model = PayoutModel::PoolShare;

        let members = vec![
            MemberInput { torn_id: 1, name: Some("A".to_string()), flat_respect: 0.0, war_hits: 3, assists: 0, outside_hits: 0, is_leadership: None },
            MemberInput { torn_id: 2, name: Some("B".to_string()), flat_respect: 0.0, war_hits: 1, assists: 0, outside_hits: 0, is_leadership: None },
        ];

        let r = compute_payout(1250000, &policy, &members, &PayoutOptions::default());
        assert_eq!(r.members_pool, 1000000);
        assert_eq!(r.members.iter().find(|m| m.torn_id == 1).unwrap().amount, 750000);
        assert_eq!(r.members.iter().find(|m| m.torn_id == 2).unwrap().amount, 250000);
        assert_eq!(r.member_payout_total, 1000000);
    }

    #[test]
    fn test_leadership_also_gets_member_pay() {
        let mut policy = default_flat_policy();
        policy.model = PayoutModel::PoolShare;
        policy.split_members_pct = 0.85;
        policy.split_faction_pct = 0.10;
        policy.split_leadership_pct = 0.05;
        policy.leadership_recipient_torn_id = Some(99);
        policy.leadership_also_gets_member_pay = Some(true);

        let members = vec![
            MemberInput { torn_id: 99, name: Some("Leader".to_string()), flat_respect: 0.0, war_hits: 50, assists: 0, outside_hits: 0, is_leadership: Some(true) },
            MemberInput { torn_id: 1, name: Some("A".to_string()), flat_respect: 0.0, war_hits: 1, assists: 0, outside_hits: 0, is_leadership: None },
            MemberInput { torn_id: 2, name: Some("B".to_string()), flat_respect: 0.0, war_hits: 1, assists: 0, outside_hits: 0, is_leadership: None },
        ];

        let r = compute_payout(1000000, &policy, &members, &PayoutOptions::default());
        let leader = r.members.iter().find(|m| m.torn_id == 99).unwrap();
        assert_eq!(leader.kind, "leadership");
        assert_eq!(leader.leadership_bonus, 50000);
        assert_eq!(leader.base_amount, 817308);
        assert_eq!(leader.amount, 867308);
        assert_eq!(r.grand_total, 1000000);
    }

    #[test]
    fn test_flat_bonus_rewards_war_hitters_only() {
        let mut policy = default_flat_policy();
        policy.model = PayoutModel::PoolShare;

        let members = vec![
            MemberInput { torn_id: 1, name: Some("Fighter".to_string()), flat_respect: 0.0, war_hits: 3, assists: 0, outside_hits: 0, is_leadership: None },
            MemberInput { torn_id: 2, name: Some("AssistOnly".to_string()), flat_respect: 0.0, war_hits: 0, assists: 4, outside_hits: 0, is_leadership: None },
            MemberInput { torn_id: 3, name: Some("OutsideOnly".to_string()), flat_respect: 0.0, war_hits: 0, assists: 0, outside_hits: 6, is_leadership: None },
            MemberInput { torn_id: 4, name: Some("NoShow".to_string()), flat_respect: 0.0, war_hits: 0, assists: 0, outside_hits: 0, is_leadership: None },
        ];

        let opts = PayoutOptions { flat_bonus_per_member: Some(5000) };
        let r = compute_payout(1000000, &policy, &members, &opts);

        assert_eq!(r.members.iter().find(|m| m.torn_id == 1).unwrap().flat_bonus, 5000);
        assert_eq!(r.members.iter().find(|m| m.torn_id == 2).unwrap().flat_bonus, 0);
        assert_eq!(r.members.iter().find(|m| m.torn_id == 3).unwrap().flat_bonus, 0);
        assert_eq!(r.members.iter().find(|m| m.torn_id == 4).unwrap().flat_bonus, 0);
        assert_eq!(r.flat_bonus_total, 5000);
    }
}
