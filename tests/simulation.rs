//! Integration tests for the econtoy simulation engine.

use econtoy::{to_csv, to_json, Params, Shock, Simulation};

fn base_params() -> Params {
    Params {
        ticks: 150,
        seed: 7,
        households: 120,
        firms: 12,
        ..Params::default()
    }
}

fn run(params: &Params) -> Simulation {
    let mut sim = Simulation::new(params);
    sim.run();
    sim
}

// --- Determinism -----------------------------------------------------------

#[test]
fn same_seed_reproduces_identical_series() {
    let params = base_params();
    let a = run(&params);
    let b = run(&params);
    assert_eq!(a.series().cpi, b.series().cpi);
    assert_eq!(a.series().gdp, b.series().gdp);
    assert_eq!(a.series().unemployment, b.series().unemployment);
    assert_eq!(a.series().credit, b.series().credit);
    assert_eq!(a.series().broad_money, b.series().broad_money);
    assert_eq!(a.series().inflation, b.series().inflation);
}

#[test]
fn different_seeds_diverge() {
    let mut p1 = base_params();
    p1.seed = 1;
    let mut p2 = base_params();
    p2.seed = 999;
    assert_ne!(run(&p1).series().cpi, run(&p2).series().cpi);
}

#[test]
fn output_is_byte_identical_across_runs() {
    let params = base_params();
    let a = run(&params);
    let b = run(&params);
    let ja = to_json(&params, a.series(), &a.summary(), None);
    let jb = to_json(&params, b.series(), &b.summary(), None);
    assert_eq!(ja, jb);
    assert_eq!(to_csv(a.series()), to_csv(b.series()));
}

// --- Accounting sanity -----------------------------------------------------

// Identity that must hold EVERY tick, regardless of growth or shocks:
//   base_money == total_deposits + bank_equity - total_loans
// When money-growth == 0 and there is no shock, base_money itself is constant.

#[test]
fn accounting_identity_holds_every_tick() {
    let mut params = base_params();
    params.money_growth = 3.0;
    params.shocks = vec![Shock::Money {
        tick: 40,
        pct: 15.0,
    }];
    let mut sim = Simulation::new(&params);
    for _ in 0..params.ticks {
        sim.step();
        assert!(
            sim.accounting_residual().abs() < 1e-6,
            "identity broken: residual {}",
            sim.accounting_residual()
        );
    }
}

#[test]
fn money_conserved_without_growth_or_shock() {
    let mut params = base_params();
    params.money_growth = 0.0;
    params.interest_rate = 4.0;
    params.shocks.clear();
    let mut sim = Simulation::new(&params);
    let initial_base = sim.base_money();
    for _ in 0..params.ticks {
        sim.step();
        assert!(
            (sim.base_money() - initial_base).abs() < 1e-9,
            "base money changed without growth/shock"
        );
        assert!(sim.accounting_residual().abs() < 1e-6);
    }
}

// --- Behavioral ------------------------------------------------------------

#[test]
fn higher_interest_rate_dampens_inflation() {
    let mut low = base_params();
    low.money_growth = 4.0;
    low.interest_rate = 2.0;
    let mut high = low.clone();
    high.interest_rate = 12.0;

    let low_sim = run(&low);
    let high_sim = run(&high);

    let low_cpi = low_sim.summary().cpi;
    let high_cpi = high_sim.summary().cpi;
    assert!(
        high_cpi < low_cpi,
        "expected high-rate CPI {high_cpi} < low-rate CPI {low_cpi}"
    );
    assert!(
        high_sim.summary().avg_inflation < low_sim.summary().avg_inflation,
        "expected high-rate inflation to be lower"
    );
}

#[test]
fn excess_demand_pushes_prices_up() {
    // Strong money growth pumps nominal demand past productive capacity, so
    // the price index must climb above its base of 100.
    let mut hot = base_params();
    hot.money_growth = 8.0;
    hot.interest_rate = 3.0;
    let hot_cpi = run(&hot).summary().cpi;
    assert!(
        hot_cpi > 100.0,
        "excess demand should lift CPI above 100, got {hot_cpi}"
    );

    // With no money growth the interest drain shrinks demand, so prices are
    // lower than in the hot run.
    let mut cool = hot.clone();
    cool.money_growth = 0.0;
    let cool_cpi = run(&cool).summary().cpi;
    assert!(
        hot_cpi > cool_cpi,
        "more demand should mean higher prices: {hot_cpi} vs {cool_cpi}"
    );
}

#[test]
fn money_shock_spikes_then_reverts() {
    let shock_tick = 80;
    let mut params = base_params();
    params.ticks = 160;
    params.money_growth = 2.0;
    params.interest_rate = 5.0;
    params.shocks = vec![Shock::Money {
        tick: shock_tick,
        pct: 30.0,
    }];
    let sim = run(&params);
    let infl = &sim.series().inflation;
    let cpi = &sim.series().cpi;

    let pre = infl[shock_tick - 2];
    let spike = infl[shock_tick];
    let later = infl[shock_tick + 40];

    assert!(
        spike > pre + 0.5,
        "shock should spike inflation: pre {pre}, spike {spike}"
    );
    assert!(
        later < spike,
        "inflation should partially revert after the spike: spike {spike}, later {later}"
    );
    assert!(
        cpi[shock_tick + 40] > cpi[shock_tick - 2],
        "CPI level should stay elevated after the shock"
    );
}

// --- Bounds ----------------------------------------------------------------

#[test]
fn series_stay_within_bounds_and_finite() {
    for &(rate, mg, mpc) in &[(0.0, 0.0, 0.1), (25.0, 10.0, 0.9), (8.0, -5.0, 0.5)] {
        let mut params = base_params();
        params.interest_rate = rate;
        params.money_growth = mg;
        params.mpc = mpc;
        let sim = run(&params);
        let s = sim.series();
        for &u in &s.unemployment {
            assert!((0.0..=1.0).contains(&u), "unemployment out of range: {u}");
        }
        for &c in &s.cpi {
            assert!(
                c > 0.0 && c.is_finite(),
                "cpi must be positive and finite: {c}"
            );
        }
        for value in [
            &s.gdp,
            &s.avg_money,
            &s.credit,
            &s.broad_money,
            &s.inflation,
        ] {
            for &v in value {
                assert!(v.is_finite(), "found non-finite value: {v}");
            }
        }
    }
}

// --- Validation ------------------------------------------------------------

#[test]
fn parameter_validation_rejects_bad_input() {
    assert!(Params {
        ticks: 0,
        ..Params::default()
    }
    .validate()
    .is_err());
    assert!(Params {
        firms: 0,
        ..Params::default()
    }
    .validate()
    .is_err());
    assert!(Params {
        households: 0,
        ..Params::default()
    }
    .validate()
    .is_err());
    assert!(Params {
        interest_rate: 150.0,
        ..Params::default()
    }
    .validate()
    .is_err());
    assert!(Params {
        mpc: 1.5,
        ..Params::default()
    }
    .validate()
    .is_err());
    assert!(Params {
        reserve_ratio: 0.0,
        ..Params::default()
    }
    .validate()
    .is_err());
    assert!(Params::default().validate().is_ok());
}

#[test]
fn shock_parsing() {
    assert_eq!(
        Shock::parse("money:100:20").unwrap(),
        Shock::Money {
            tick: 100,
            pct: 20.0
        }
    );
    assert_eq!(
        Shock::parse("demand:50:30:10").unwrap(),
        Shock::Demand {
            tick: 50,
            pct: 30.0,
            dur: 10
        }
    );
    assert!(Shock::parse("money:100").is_err());
    assert!(Shock::parse("demand:50:30").is_err());
    assert!(Shock::parse("bogus:1:2").is_err());
    assert!(Shock::parse("demand:50:30:0").is_err());
    assert!(Shock::parse("money:x:2").is_err());
}

#[test]
fn shock_after_horizon_is_rejected() {
    let params = Params {
        ticks: 50,
        shocks: vec![Shock::Money {
            tick: 80,
            pct: 10.0,
        }],
        ..Params::default()
    };
    assert!(params.validate().is_err());
}
