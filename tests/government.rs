//! Tests for the government sector, policy changes, and scenario files.

use econtoy::scenario::{self, BUILTIN_SCENARIOS};
use econtoy::{Knob, Params, Shock, Simulation};

fn gov_params() -> Params {
    Params {
        ticks: 200,
        income_tax: 20.0,
        profit_tax: 20.0,
        gov_spending: 22.0,
        ..Params::default()
    }
}

fn run(params: &Params) -> Simulation {
    params.validate().expect("valid params");
    let mut sim = Simulation::new(params);
    sim.run();
    sim
}

fn max(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

#[test]
fn accounting_identity_holds_with_government_and_policy_changes() {
    let params = Params {
        income_tax: 12.0,
        profit_tax: 12.0,
        gov_spending: 28.0,
        shocks: vec![
            Shock::Policy {
                tick: 60,
                knob: Knob::IncomeTax,
                value: 30.0,
            },
            Shock::Policy {
                tick: 90,
                knob: Knob::GovSpending,
                value: 10.0,
            },
            Shock::Policy {
                tick: 120,
                knob: Knob::InterestRate,
                value: 20.0,
            },
            Shock::Money {
                tick: 40,
                pct: 15.0,
            },
        ],
        ..gov_params()
    };
    params.validate().unwrap();
    let mut sim = Simulation::new(&params);
    for t in 0..params.ticks {
        sim.step();
        let residual = sim.accounting_residual();
        assert!(
            residual.abs() < 1e-6 * sim.base_money().max(1.0),
            "tick {t}: residual {residual}"
        );
    }
}

#[test]
fn no_government_means_zero_fiscal_series() {
    let sim = run(&Params::default());
    let s = sim.series();
    for series in [&s.taxes, &s.gov_spending, &s.deficit, &s.gov_debt] {
        assert!(series.iter().all(|&x| x == 0.0 && x.is_sign_positive()));
    }
    assert!(!Params::default().has_government());
}

#[test]
fn deficit_accumulates_into_debt() {
    let sim = run(&gov_params());
    let s = sim.series();
    let mut debt = 0.0;
    for (i, d) in s.deficit.iter().enumerate() {
        debt += d;
        assert!(
            (debt - s.gov_debt[i]).abs() < 1e-6 * debt.abs().max(1.0),
            "tick {i}"
        );
    }
    assert!(s.taxes.iter().all(|&x| x > 0.0));
    assert!(s.gov_spending.iter().all(|&x| x > 0.0));
}

#[test]
fn spending_beyond_the_tax_take_raises_debt_and_prices() {
    let modest = run(&gov_params());
    let lavish = run(&Params {
        income_tax: 10.0,
        profit_tax: 10.0,
        gov_spending: 30.0,
        ..gov_params()
    });
    assert!(lavish.gov_debt() > 1000.0, "debt {}", lavish.gov_debt());
    assert!(modest.gov_debt() < 100.0, "debt {}", modest.gov_debt());
    assert!(lavish.summary().cpi > 5.0 * modest.summary().cpi);
}

#[test]
fn stimulus_lowers_peak_unemployment_in_a_slump() {
    let slump = Params {
        shocks: vec![Shock::Demand {
            tick: 60,
            pct: -50.0,
            dur: 36,
        }],
        ..gov_params()
    };
    let mut stimulus = slump.clone();
    stimulus.shocks.push(Shock::Policy {
        tick: 66,
        knob: Knob::GovSpending,
        value: 40.0,
    });
    stimulus.shocks.push(Shock::Policy {
        tick: 102,
        knob: Knob::GovSpending,
        value: 22.0,
    });

    let a = run(&slump);
    let b = run(&stimulus);
    // Average unemployment while the slump lasts, from the first tick the
    // extra purchases can change hiring.
    let after = |sim: &Simulation| {
        let u = &sim.series().unemployment[67..96];
        u.iter().sum::<f64>() / u.len() as f64
    };
    assert!(
        after(&b) < after(&a),
        "with {} vs without {}",
        after(&b),
        after(&a)
    );
    assert!(max(&b.series().gov_debt) > max(&a.series().gov_debt));
}

#[test]
fn policy_change_takes_effect_at_its_tick() {
    let base = Params {
        ticks: 120,
        ..Params::default()
    };
    let changed = Params {
        shocks: vec![Shock::Policy {
            tick: 50,
            knob: Knob::GovSpending,
            value: 30.0,
        }],
        ..base.clone()
    };
    let a = run(&base);
    let b = run(&changed);
    assert_eq!(a.series().cpi[..50], b.series().cpi[..50]);
    assert_eq!(b.series().gov_spending[49], 0.0);
    assert!(b.series().gov_spending[50] > 0.0);
    assert_eq!(b.current_params().gov_spending, 30.0);
}

#[test]
fn rate_hike_cools_inflation() {
    let base = Params {
        ticks: 240,
        money_growth: 4.0,
        interest_rate: 1.0,
        ..Params::default()
    };
    let hike = Params {
        shocks: vec![Shock::Policy {
            tick: 100,
            knob: Knob::InterestRate,
            value: 15.0,
        }],
        ..base.clone()
    };
    let a = run(&base);
    let b = run(&hike);
    assert!(b.summary().cpi < a.summary().cpi);
}

#[test]
fn policy_specs_parse_round_trip_and_validate() {
    let shock = Shock::parse("set:10:gov_spending:25").unwrap();
    assert_eq!(
        shock,
        Shock::Policy {
            tick: 10,
            knob: Knob::GovSpending,
            value: 25.0
        }
    );
    assert_eq!(shock.spec(), "set:10:gov-spending:25");
    assert_eq!(Shock::parse(&shock.spec()).unwrap(), shock);
    assert!(Shock::parse("set:10:mpc:0.5")
        .unwrap_err()
        .contains("unknown policy knob"));
    assert!(Shock::parse("set:10:income-tax").is_err());

    let out_of_range = Params {
        shocks: vec![Shock::Policy {
            tick: 5,
            knob: Knob::IncomeTax,
            value: 120.0,
        }],
        ..Params::default()
    };
    assert!(out_of_range.validate().unwrap_err().contains("income-tax"));
    for (field, value) in [
        ("income-tax", -1.0),
        ("profit-tax", 101.0),
        ("gov-spending", 200.0),
    ] {
        let mut p = Params::default();
        p.set(field, &value.to_string()).unwrap();
        assert!(p.validate().is_err(), "{field}={value}");
    }
}

#[test]
fn params_set_covers_every_key() {
    let mut p = Params::default();
    for (key, value) in [
        ("ticks", "300"),
        ("seed", "9"),
        ("households", "50"),
        ("firms", "5"),
        ("interest-rate", "7"),
        ("money_growth", "3"),
        ("mpc", "0.4"),
        ("reserve-ratio", "0.2"),
        ("income-tax", "15"),
        ("profit-tax", "10"),
        ("gov-spending", "18"),
        ("shock", "money:10:5"),
        ("set", "20:interest-rate:9"),
    ] {
        p.set(key, value).unwrap_or_else(|e| panic!("{key}: {e}"));
    }
    assert_eq!(p.ticks, 300);
    assert_eq!(p.money_growth, 3.0);
    assert_eq!(p.gov_spending, 18.0);
    assert_eq!(p.shocks.len(), 2);
    assert!(p.validate().is_ok());
    assert!(p.set("bogus", "1").unwrap_err().contains("unknown setting"));
    assert!(p
        .set("ticks", "many")
        .unwrap_err()
        .contains("invalid value for ticks"));
}

#[test]
fn scenario_file_applies_settings_and_policy_changes() {
    let text = "\
# a comment
name = Test run
description = Something to look at.

ticks = 120   # trailing comment
income_tax = 15
gov-spending = 20
shock = demand:30:-20:5
at 40: gov-spending = 35
AT 50 : interest-rate = 9
";
    let mut p = Params::default();
    let s = scenario::apply(text, &mut p).unwrap();
    assert_eq!(s.name.as_deref(), Some("Test run"));
    assert_eq!(s.description.as_deref(), Some("Something to look at."));
    assert_eq!(p.ticks, 120);
    assert_eq!(p.income_tax, 15.0);
    assert_eq!(
        p.shocks,
        vec![
            Shock::Demand {
                tick: 30,
                pct: -20.0,
                dur: 5
            },
            Shock::Policy {
                tick: 40,
                knob: Knob::GovSpending,
                value: 35.0
            },
            Shock::Policy {
                tick: 50,
                knob: Knob::InterestRate,
                value: 9.0
            },
        ]
    );
    assert!(p.validate().is_ok());
}

#[test]
fn scenario_errors_name_the_line() {
    let cases = [
        (
            "ticks = 10\nticks = 20\n",
            "line 2: 'ticks' is set more than once",
        ),
        ("ticks\n", "line 1: expected 'KEY = VALUE'"),
        ("\n\nwhat = 1\n", "line 3: unknown setting 'what'"),
        ("at x: mpc = 1\n", "line 1: invalid tick 'x'"),
        ("at 5: mpc = 0.5\n", "line 1: 'mpc' cannot change mid-run"),
        (
            "at 5 mpc = 0.5\n",
            "line 1: expected 'at TICK: KNOB = VALUE'",
        ),
        ("seed =\n", "line 1: missing value for 'seed'"),
        ("shock = boom:1\n", "line 1: unknown shock kind 'boom'"),
    ];
    for (text, want) in cases {
        let err = scenario::apply(text, &mut Params::default()).unwrap_err();
        assert!(err.starts_with(want), "{text:?}: got {err:?}");
    }
}

#[test]
fn every_builtin_scenario_is_valid_and_keeps_the_books() {
    for (name, text) in BUILTIN_SCENARIOS {
        let mut p = Params::default();
        let s = scenario::apply(text, &mut p).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(s.name.is_some() && s.description.is_some(), "{name}");
        p.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut sim = Simulation::new(&p);
        for _ in 0..p.ticks {
            sim.step();
            assert!(
                sim.accounting_residual().abs() < 1e-6 * sim.base_money().max(1.0),
                "{name}"
            );
        }
        assert_eq!(scenario::builtin(name), Some(text));
    }
    assert_eq!(scenario::builtin("nope"), None);
}

#[test]
fn shrinking_money_with_taxes_does_not_panic_and_keeps_the_books() {
    for (tax, growth) in [(20.0, -2.0), (50.0, -30.0), (0.0, -100.0)] {
        let params = Params {
            income_tax: tax,
            money_growth: growth,
            ..Params::default()
        };
        params.validate().unwrap();
        let mut sim = Simulation::new(&params);
        for _ in 0..params.ticks {
            sim.step();
            assert!(sim.accounting_residual().abs() < 1e-6 * sim.base_money().abs().max(1.0));
        }
    }
}

#[test]
fn taxes_reach_the_treasury_before_it_pays_interest() {
    // Wage tax alone covers the purchases, so the treasury never borrows and
    // owes no interest within the tick.
    let params = Params {
        ticks: 1,
        income_tax: 100.0,
        gov_spending: 20.0,
        interest_rate: 12.0,
        ..Params::default()
    };
    let sim = run(&params);
    let s = sim.series();
    let interest = s.deficit[0] - (s.gov_spending[0] - s.taxes[0]);
    assert!(interest.abs() < 1e-9, "debt interest {interest}");
}
