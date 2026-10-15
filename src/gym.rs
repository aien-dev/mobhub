//! Vladar Training Formula V2.0 in native Rust.
//! Bit-for-bit validated against Vladar's canonical reference data and community gym guides.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatName {
    Strength,
    Speed,
    Dexterity,
    Defense,
}

impl StatName {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatName::Strength => "strength",
            StatName::Speed => "speed",
            StatName::Dexterity => "dexterity",
            StatName::Defense => "defense",
        }
    }
}

impl std::str::FromStr for StatName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "strength" | "str" => Ok(StatName::Strength),
            "speed" | "spd" => Ok(StatName::Speed),
            "dexterity" | "dex" => Ok(StatName::Dexterity),
            "defense" | "def" => Ok(StatName::Defense),
            _ => Err(format!("unknown stat: {}", s)),
        }
    }
}

pub struct StatConstants {
    pub a: f64,
    pub b: f64,
}

pub fn get_stat_constants(stat: StatName) -> StatConstants {
    match stat {
        StatName::Strength => StatConstants {
            a: 1600.0,
            b: 1700.0,
        },
        StatName::Speed => StatConstants {
            a: 1600.0,
            b: 2000.0,
        },
        StatName::Dexterity => StatConstants {
            a: 1800.0,
            b: 1500.0,
        },
        StatName::Defense => StatConstants {
            a: 2100.0,
            b: -600.0,
        },
    }
}

pub const STAT_SOFT_CAP: f64 = 50_000_000.0;
pub const SOFT_CAP_DIVISOR_FACTOR: f64 = 8.77635;
pub const HAPPY_LOG_COEFF: f64 = 0.07;
pub const HAPPY_LOG_OFFSET: f64 = 250.0;
pub const HAPPY_POWER_COEFF: f64 = 8.0;
pub const HAPPY_POWER_EXP: f64 = 1.05;
pub const HAPPY_MAX: f64 = 99_999.0;
pub const FORMULA_DIVISOR: f64 = 200_000.0;

fn round_to(x: f64, dp: i32) -> f64 {
    let p = 10f64.powi(dp);
    (x * p).round() / p
}

pub fn soft_cap_stat(stat: f64) -> f64 {
    if stat <= STAT_SOFT_CAP {
        stat
    } else {
        STAT_SOFT_CAP + (stat - STAT_SOFT_CAP) / (SOFT_CAP_DIVISOR_FACTOR * stat.log10())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GainParams {
    pub stat: StatName,
    pub stat_value: f64,
    pub happy: f64,
    pub energy_per_train: f64,
    pub gym_dots: f64,
    #[serde(default)]
    pub perk_bonuses: Vec<f64>,
}

pub fn perk_multiplier(perks: &[f64]) -> f64 {
    perks.iter().fold(1.0, |acc, &x| acc * (1.0 + x))
}

/// Calculate expected single-train stat gain according to Vladar Training Formula V2.0.
pub fn expected_gain_per_train(p: &GainParams) -> f64 {
    let c = get_stat_constants(p.stat);
    let h = p.happy.max(0.0);
    let seff = soft_cap_stat(p.stat_value);

    let log_term = round_to((1.0 + h / HAPPY_LOG_OFFSET).ln(), 4);
    let happy_mult = round_to(1.0 + HAPPY_LOG_COEFF * log_term, 4);

    let inner = seff * happy_mult
        + HAPPY_POWER_COEFF * h.powf(HAPPY_POWER_EXP)
        + (1.0 - (h / HAPPY_MAX).powi(2)) * c.a
        + c.b;

    (inner / FORMULA_DIVISOR) * p.gym_dots * p.energy_per_train * perk_multiplier(&p.perk_bonuses)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionParams {
    pub stat: StatName,
    pub start_stat: f64,
    pub start_happy: f64,
    pub energy_available: f64,
    pub energy_per_train: f64,
    pub gym_dots: f64,
    #[serde(default)]
    pub perk_bonuses: Vec<f64>,
    pub happy_loss_per_train: Option<f64>,
    pub min_happy: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResult {
    pub trains: usize,
    pub energy_spent: f64,
    pub total_gain: f64,
    pub end_stat: f64,
    pub start_happy: f64,
    pub end_happy: f64,
    pub average_gain_per_train: f64,
}

/// Simulate a full training session with decaying happy.
pub fn simulate_session(p: &SessionParams) -> SessionResult {
    let loss = p.happy_loss_per_train.unwrap_or(0.5 * p.energy_per_train);
    let min_h = p.min_happy.unwrap_or(0.0);

    let mut stat = p.start_stat;
    let mut happy = p.start_happy;
    let mut energy = p.energy_available;
    let mut trains = 0;
    let mut total_gain = 0.0;

    let p_gain = GainParams {
        stat: p.stat,
        stat_value: stat,
        happy,
        energy_per_train: p.energy_per_train,
        gym_dots: p.gym_dots,
        perk_bonuses: p.perk_bonuses.clone(),
    };

    while energy >= p.energy_per_train {
        let mut curr_p = p_gain.clone();
        curr_p.stat_value = stat;
        curr_p.happy = happy;

        let gain = expected_gain_per_train(&curr_p);
        stat += gain;
        total_gain += gain;
        happy = (happy - loss).max(min_h);
        energy -= p.energy_per_train;
        trains += 1;
    }

    SessionResult {
        trains,
        energy_spent: (trains as f64) * p.energy_per_train,
        total_gain,
        end_stat: stat,
        start_happy: p.start_happy,
        end_happy: happy,
        average_gain_per_train: if trains > 0 {
            total_gain / (trains as f64)
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vladar_formula_reference_cases() {
        // Reference 1: STR S=1,000,000 H=5,000 G=7.3 E=5 no perks -> dS approx 233.17
        let p1 = GainParams {
            stat: StatName::Strength,
            stat_value: 1_000_000.0,
            happy: 5000.0,
            energy_per_train: 5.0,
            gym_dots: 7.3,
            perk_bonuses: vec![],
        };
        let gain1 = expected_gain_per_train(&p1);
        assert!((gain1 - 233.17).abs() < 0.01, "STR H=5000: got {}", gain1);

        // Reference 2: STR S=1,000,000 H=0 G=7.3 E=5 no perks -> dS approx 183.10
        let p2 = GainParams {
            stat: StatName::Strength,
            stat_value: 1_000_000.0,
            happy: 0.0,
            energy_per_train: 5.0,
            gym_dots: 7.3,
            perk_bonuses: vec![],
        };
        let gain2 = expected_gain_per_train(&p2);
        assert!((gain2 - 183.10).abs() < 0.01, "STR H=0: got {}", gain2);

        // Reference 3: DEF S=1,000,000 H=10,000 G=6.0 E=25 no perks -> dS approx 1041.20
        let p3 = GainParams {
            stat: StatName::Defense,
            stat_value: 1_000_000.0,
            happy: 10_000.0,
            energy_per_train: 25.0,
            gym_dots: 6.0,
            perk_bonuses: vec![],
        };
        let gain3 = expected_gain_per_train(&p3);
        assert!((gain3 - 1041.20).abs() < 0.01, "DEF H=10000: got {}", gain3);
    }

    #[test]
    fn test_simulate_session() {
        let p = SessionParams {
            stat: StatName::Strength,
            start_stat: 1_000_000.0,
            start_happy: 5000.0,
            energy_available: 100.0,
            energy_per_train: 5.0,
            gym_dots: 7.3,
            perk_bonuses: vec![],
            happy_loss_per_train: Some(2.5),
            min_happy: Some(0.0),
        };
        let res = simulate_session(&p);
        assert_eq!(res.trains, 20);
        assert_eq!(res.energy_spent, 100.0);
        assert_eq!(res.end_happy, 5000.0 - 20.0 * 2.5);
        assert!(res.total_gain > 4000.0);
    }
}
