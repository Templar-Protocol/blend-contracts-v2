#![cfg(test)]

use std::collections::VecDeque;

type Price = i128;

const PRICE_SCALE: i128 = 10_000_000;
const WINDOW_SECS: u64 = 300;
const MAX_LOOKBACK_WINDOWS: usize = 3;

const USTRY_CLEAN_PRICE: Price = 10_574_000;
const USTRY_MANIPULATED_PRICE: Price = 1_067_373_000;
const XLM_PRICE: Price = 1_609_348;

const T0_CLEAN: u64 = 1_771_718_700;
const T1_CLEAN: u64 = 1_771_719_000;
const T2_MANIP: u64 = 1_771_719_300;
const T3_MANIP: u64 = 1_771_719_600;

#[derive(Clone, Copy, Debug)]
struct Trade {
    price: Price,
    volume_quote: i128,
}

#[derive(Clone, Debug)]
struct ReflectorWindow {
    trades: Vec<Trade>,
    active_market_makers: u32,
    peak_orderbook_depth_quote: i128,
}

impl ReflectorWindow {
    fn vwap(&self) -> Option<Price> {
        if self.trades.is_empty() {
            return None;
        }
        let total_volume: i128 = self.trades.iter().map(|t| t.volume_quote).sum();
        if total_volume == 0 {
            return None;
        }
        let weighted: i128 = self.trades.iter().map(|t| t.price * t.volume_quote).sum();
        Some(weighted / total_volume)
    }

    fn total_volume_quote(&self) -> i128 {
        self.trades.iter().map(|t| t.volume_quote).sum()
    }
}

#[derive(Clone, Debug, Default)]
struct OracleHistory {
    windows: VecDeque<ReflectorWindow>,
}

impl OracleHistory {
    fn push(&mut self, window: ReflectorWindow) {
        self.windows.push_back(window);
        while self.windows.len() > 16 {
            self.windows.pop_front();
        }
    }

    fn latest(&self) -> Option<&ReflectorWindow> {
        self.windows.back()
    }

    fn at_offset(&self, offset_from_latest: usize) -> Option<&ReflectorWindow> {
        let len = self.windows.len();
        if offset_from_latest >= len {
            return None;
        }
        self.windows.get(len - 1 - offset_from_latest)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PriceResult {
    Ok(Price),
    RejectedDeviation,
    RejectedLowVolume,
    RejectedCircuitBreaker,
    RejectedThinMarket,
    Unavailable,
}

#[derive(Clone, Copy, Debug)]
struct OracleConfig {
    max_dev_bps: u32,
    min_window_volume_quote: i128,
    min_reference_windows: usize,
    circuit_breaker_bps: u32,
    min_market_makers: u32,
    min_orderbook_depth_quote: i128,
}

impl OracleConfig {
    fn secure_defaults() -> Self {
        Self {
            max_dev_bps: 1_000,
            min_window_volume_quote: 100 * PRICE_SCALE,
            min_reference_windows: 2,
            circuit_breaker_bps: 5_000,
            min_market_makers: 2,
            min_orderbook_depth_quote: 1_000 * PRICE_SCALE,
        }
    }

    fn vulnerable_defaults() -> Self {
        Self {
            max_dev_bps: 1_000,
            min_window_volume_quote: 0,
            min_reference_windows: 1,
            circuit_breaker_bps: u32::MAX,
            min_market_makers: 0,
            min_orderbook_depth_quote: 0,
        }
    }
}

trait ProxyOracle {
    fn quote(&self, history: &OracleHistory, cfg: &OracleConfig) -> PriceResult;
}

struct VulnerableOracle;

impl ProxyOracle for VulnerableOracle {
    fn quote(&self, history: &OracleHistory, _cfg: &OracleConfig) -> PriceResult {
        let current = match history.latest().and_then(ReflectorWindow::vwap) {
            Some(price) => price,
            None => return PriceResult::Unavailable,
        };
        for offset in 1..=MAX_LOOKBACK_WINDOWS {
            if let Some(window) = history.at_offset(offset) {
                if let Some(older) = window.vwap() {
                    let diff = (current - older).abs();
                    let dev_bps = (diff * 10_000) / older.max(1);
                    if dev_bps > 1_000 {
                        return PriceResult::RejectedDeviation;
                    }
                    return PriceResult::Ok(current);
                }
            }
        }
        PriceResult::Ok(current)
    }
}

struct FixedOracle;

impl ProxyOracle for FixedOracle {
    fn quote(&self, history: &OracleHistory, cfg: &OracleConfig) -> PriceResult {
        let latest = match history.latest() {
            Some(window) => window,
            None => return PriceResult::Unavailable,
        };

        if latest.active_market_makers < cfg.min_market_makers {
            return PriceResult::RejectedThinMarket;
        }
        if latest.peak_orderbook_depth_quote < cfg.min_orderbook_depth_quote {
            return PriceResult::RejectedThinMarket;
        }
        if latest.total_volume_quote() < cfg.min_window_volume_quote {
            return PriceResult::RejectedLowVolume;
        }

        let current = match latest.vwap() {
            Some(price) => price,
            None => return PriceResult::Unavailable,
        };

        let mut refs = Vec::new();
        for offset in 1..=MAX_LOOKBACK_WINDOWS.max(cfg.min_reference_windows + 2) {
            if let Some(window) = history.at_offset(offset) {
                if window.total_volume_quote() < cfg.min_window_volume_quote {
                    continue;
                }
                if window.active_market_makers < cfg.min_market_makers {
                    continue;
                }
                if let Some(price) = window.vwap() {
                    refs.push(price);
                }
            }
        }

        if refs.len() < cfg.min_reference_windows {
            return PriceResult::RejectedLowVolume;
        }

        let all_identical = refs.windows(2).all(|pair| pair[0] == pair[1]);
        if all_identical && refs.len() < cfg.min_reference_windows + 1 {
            return PriceResult::RejectedDeviation;
        }

        let reference = *refs.last().unwrap();
        let diff = (current - reference).abs();
        let dev_bps = (diff * 10_000) / reference.max(1);
        if dev_bps as u32 > cfg.circuit_breaker_bps {
            return PriceResult::RejectedCircuitBreaker;
        }
        if dev_bps as u32 > cfg.max_dev_bps {
            return PriceResult::RejectedDeviation;
        }

        PriceResult::Ok(current)
    }
}

fn clean_window(timestamp: u64) -> ReflectorWindow {
    let _ = timestamp;
    ReflectorWindow {
        trades: vec![
            Trade {
                price: USTRY_CLEAN_PRICE,
                volume_quote: 500 * PRICE_SCALE,
            },
            Trade {
                price: USTRY_CLEAN_PRICE + 100,
                volume_quote: 300 * PRICE_SCALE,
            },
        ],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    }
}

fn manipulated_window(timestamp: u64, price: Price) -> ReflectorWindow {
    let _ = timestamp;
    ReflectorWindow {
        trades: vec![Trade {
            price,
            volume_quote: 5 * PRICE_SCALE,
        }],
        active_market_makers: 1,
        peak_orderbook_depth_quote: 5 * PRICE_SCALE,
    }
}

fn exploit_history() -> OracleHistory {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(manipulated_window(T2_MANIP, USTRY_MANIPULATED_PRICE));
    history.push(manipulated_window(T3_MANIP, USTRY_MANIPULATED_PRICE));
    history
}

fn health_factor(
    collateral_price: Price,
    collateral_amount: i128,
    cf: f64,
    liability_price: Price,
    liability_amount: i128,
    lf: f64,
) -> f64 {
    let collateral = (collateral_price as f64) * (collateral_amount as f64) * cf;
    let liability = (liability_price as f64) * (liability_amount as f64) / lf;
    collateral / liability
}

#[test]
fn f1_vulnerable_oracle_accepts_the_exact_exploit_sequence() {
    let history = exploit_history();
    let result = VulnerableOracle.quote(&history, &OracleConfig::vulnerable_defaults());
    assert_eq!(result, PriceResult::Ok(USTRY_MANIPULATED_PRICE));
}

#[test]
fn f1_fixed_oracle_rejects_the_exact_exploit_sequence() {
    let history = exploit_history();
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(!matches!(result, PriceResult::Ok(_)));
}

#[test]
fn f1_fix_rejects_two_identical_manipulated_windows_with_no_clean_reference_behind_them() {
    let mut history = OracleHistory::default();
    history.push(manipulated_window(T2_MANIP, USTRY_MANIPULATED_PRICE));
    history.push(manipulated_window(T3_MANIP, USTRY_MANIPULATED_PRICE));
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(matches!(
        result,
        PriceResult::RejectedDeviation
            | PriceResult::RejectedLowVolume
            | PriceResult::RejectedThinMarket
    ));
}

#[test]
fn f1_fix_rejects_three_identical_manipulated_windows() {
    let mut history = OracleHistory::default();
    history.push(manipulated_window(
        T2_MANIP - WINDOW_SECS,
        USTRY_MANIPULATED_PRICE,
    ));
    history.push(manipulated_window(T2_MANIP, USTRY_MANIPULATED_PRICE));
    history.push(manipulated_window(T3_MANIP, USTRY_MANIPULATED_PRICE));
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(!matches!(result, PriceResult::Ok(_)));
}

#[test]
fn f1_fix_accepts_a_genuinely_stable_price() {
    let mut history = OracleHistory::default();
    for i in 0..5 {
        history.push(clean_window(T0_CLEAN + i * WINDOW_SECS));
    }
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(matches!(result, PriceResult::Ok(price) if (price - USTRY_CLEAN_PRICE).abs() < 1_000));
}

#[test]
fn f1_fix_accepts_a_realistic_gradual_price_drift() {
    let mut history = OracleHistory::default();
    for i in 0..5 {
        let drift = (USTRY_CLEAN_PRICE * i as i128) / 200;
        history.push(ReflectorWindow {
            trades: vec![Trade {
                price: USTRY_CLEAN_PRICE + drift,
                volume_quote: 500 * PRICE_SCALE,
            }],
            active_market_makers: 3,
            peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
        });
    }
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(matches!(result, PriceResult::Ok(_)));
}

#[test]
fn f2_fix_rejects_window_with_sub_threshold_volume_even_if_price_looks_sane() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE,
            volume_quote: 5 * PRICE_SCALE,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert_eq!(result, PriceResult::RejectedLowVolume);
}

#[test]
fn f2_fix_accepts_threshold_exactly_at_minimum() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    let min_volume = OracleConfig::secure_defaults().min_window_volume_quote;
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE,
            volume_quote: min_volume,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(matches!(result, PriceResult::Ok(_)));
}

#[test]
fn f2_fix_rejects_volume_one_stroop_below_threshold() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    let min_volume = OracleConfig::secure_defaults().min_window_volume_quote;
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE,
            volume_quote: min_volume - 1,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert_eq!(result, PriceResult::RejectedLowVolume);
}

#[test]
fn f3_100x_jump_trips_circuit_breaker_even_if_deviation_math_were_bypassed() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_MANIPULATED_PRICE,
            volume_quote: 1_000 * PRICE_SCALE,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert_eq!(result, PriceResult::RejectedCircuitBreaker);
}

#[test]
fn f3_minimum_inflation_threshold_from_report_is_blocked() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE * 92,
            volume_quote: 1_000 * PRICE_SCALE,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(!matches!(result, PriceResult::Ok(_)));
}

#[test]
fn f3_circuit_breaker_trips_below_max_dev_boundary() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE * 2,
            volume_quote: 1_000 * PRICE_SCALE,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let cfg = OracleConfig {
        max_dev_bps: 100_000,
        circuit_breaker_bps: 5_000,
        ..OracleConfig::secure_defaults()
    };
    let result = FixedOracle.quote(&history, &cfg);
    assert_eq!(result, PriceResult::RejectedCircuitBreaker);
}

#[test]
fn f4_single_market_maker_pair_is_rejected_even_with_sensible_price() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE,
            volume_quote: 1_000 * PRICE_SCALE,
        }],
        active_market_makers: 1,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert_eq!(result, PriceResult::RejectedThinMarket);
}

#[test]
fn f4_shallow_book_is_rejected() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: USTRY_CLEAN_PRICE,
            volume_quote: 1_000 * PRICE_SCALE,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 10 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert_eq!(result, PriceResult::RejectedThinMarket);
}

#[test]
fn variant_attacker_spreads_manipulation_across_three_windows() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    for i in 0..4 {
        history.push(manipulated_window(
            T0_CLEAN + WINDOW_SECS * (i + 1),
            USTRY_MANIPULATED_PRICE,
        ));
    }
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(!matches!(result, PriceResult::Ok(_)));
}

#[test]
fn variant_attacker_uses_slightly_varying_manipulated_prices() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    for i in 0..4 {
        history.push(ReflectorWindow {
            trades: vec![Trade {
                price: USTRY_MANIPULATED_PRICE + i as i128 * 1_000,
                volume_quote: 5 * PRICE_SCALE,
            }],
            active_market_makers: 1,
            peak_orderbook_depth_quote: 5 * PRICE_SCALE,
        });
    }
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(!matches!(result, PriceResult::Ok(_)));
}

#[test]
fn variant_attacker_pumps_volume_with_wash_trades() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(ReflectorWindow {
        trades: vec![
            Trade {
                price: USTRY_MANIPULATED_PRICE,
                volume_quote: 500 * PRICE_SCALE,
            },
            Trade {
                price: USTRY_MANIPULATED_PRICE,
                volume_quote: 500 * PRICE_SCALE,
            },
        ],
        active_market_makers: 1,
        peak_orderbook_depth_quote: 5 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(matches!(
        result,
        PriceResult::RejectedThinMarket | PriceResult::RejectedCircuitBreaker
    ));
}

#[test]
fn variant_attacker_sandwiches_manipulation_between_clean_windows() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    history.push(manipulated_window(T2_MANIP, USTRY_MANIPULATED_PRICE));
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert!(!matches!(result, PriceResult::Ok(_)));
}

#[test]
fn variant_attacker_submits_manipulated_price_just_under_circuit_breaker() {
    let mut history = OracleHistory::default();
    history.push(clean_window(T0_CLEAN));
    history.push(clean_window(T1_CLEAN));
    let pumped = USTRY_CLEAN_PRICE + (USTRY_CLEAN_PRICE * 49) / 100;
    history.push(ReflectorWindow {
        trades: vec![Trade {
            price: pumped,
            volume_quote: 1_000 * PRICE_SCALE,
        }],
        active_market_makers: 3,
        peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
    });
    let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
    assert_eq!(result, PriceResult::RejectedDeviation);
}

#[test]
fn hf_reproduces_attackers_1_0985_at_the_manipulated_price() {
    let hf = health_factor(
        USTRY_MANIPULATED_PRICE,
        149_876_130_000_000,
        0.90,
        XLM_PRICE,
        610_846_237_100_000,
        0.75,
    );
    assert!((hf - 1.0985).abs() < 0.01);
}

#[test]
fn hf_at_clean_price_is_catastrophically_under_one() {
    let hf = health_factor(
        USTRY_CLEAN_PRICE,
        149_876_130_000_000,
        0.90,
        XLM_PRICE,
        610_846_237_100_000,
        0.75,
    );
    assert!(hf < 0.02);
}

#[test]
fn property_fixed_oracle_never_accepts_price_more_than_breaker_from_any_healthy_ref() {
    let multipliers = [2, 5, 10, 50, 91, 92, 100, 1_000];
    for mult in multipliers {
        for lookback in 2..=5 {
            let mut history = OracleHistory::default();
            for i in 0..lookback {
                history.push(clean_window(T0_CLEAN + i as u64 * WINDOW_SECS));
            }
            history.push(ReflectorWindow {
                trades: vec![Trade {
                    price: USTRY_CLEAN_PRICE * mult as i128,
                    volume_quote: 10_000 * PRICE_SCALE,
                }],
                active_market_makers: 5,
                peak_orderbook_depth_quote: 100_000 * PRICE_SCALE,
            });
            let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
            assert!(
                !matches!(result, PriceResult::Ok(_)),
                "mult={mult}, lookback={lookback}"
            );
        }
    }
}

#[test]
fn property_fixed_oracle_accepts_all_small_drifts_on_healthy_market() {
    for drift_bps in [0i128, 10, 50, 100, 500, 900] {
        let mut history = OracleHistory::default();
        for i in 0..3 {
            history.push(clean_window(T0_CLEAN + i * WINDOW_SECS));
        }
        let drifted = USTRY_CLEAN_PRICE + (USTRY_CLEAN_PRICE * drift_bps) / 10_000;
        history.push(ReflectorWindow {
            trades: vec![Trade {
                price: drifted,
                volume_quote: 1_000 * PRICE_SCALE,
            }],
            active_market_makers: 3,
            peak_orderbook_depth_quote: 50_000 * PRICE_SCALE,
        });
        let result = FixedOracle.quote(&history, &OracleConfig::secure_defaults());
        assert!(
            matches!(result, PriceResult::Ok(_)),
            "drift_bps={drift_bps}"
        );
    }
}
