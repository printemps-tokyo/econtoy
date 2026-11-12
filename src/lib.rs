//! econtoy: a tiny agent-based economy simulator.
//!
//! This crate contains all of the pure simulation logic. It has zero external
//! dependencies: the pseudo-random number generator, the time-series
//! bookkeeping, the sparkline renderer, and the JSON/CSV serializers are all
//! implemented here so that the whole model is deterministic and legible.
//!
//! The public entry point is [`Simulation`]: build one from a [`Params`],
//! call [`Simulation::run`], and read the resulting [`Series`] and
//! [`Summary`]. Given the same seed and the same parameters the simulation
//! produces byte-identical output.
//!
//! This is an educational "emergence toy", not a real macroeconomic model.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

pub mod scenario;

/// Number of ticks that make up one year. Annualized rates (interest,
/// money growth) are divided by this to obtain a per-tick rate.
pub const PERIODS_PER_YEAR: f64 = 12.0;

/// Goods produced per worker per tick.
const PRODUCTIVITY: f64 = 1.0;
/// Nominal wage paid per worker per tick (sticky in this toy).
const WAGE: f64 = 1.0;
/// Speed at which firms move their price in response to demand/supply.
const PRICE_ADJ: f64 = 0.10;
/// Speed at which firms move their planned output toward profitable levels.
const OUTPUT_ADJ: f64 = 0.25;
/// Precautionary cash buffer firms keep, expressed in wage bills.
const BUFFER_FACTOR: f64 = 1.0;
/// Small epsilon used to avoid divisions by zero.
const EPS: f64 = 1e-9;

/// The eight block glyphs used by [`sparkline`], from lowest to highest.
pub const SPARK_BLOCKS: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];

// ---------------------------------------------------------------------------
// Deterministic RNG (xorshift64*)
// ---------------------------------------------------------------------------

/// A tiny deterministic pseudo-random number generator (`xorshift64*`).
///
/// It is seeded through `splitmix64` so that even "boring" seeds such as `0`
/// produce a well-mixed stream. It is not cryptographically secure; it exists
/// only to make the simulation reproducible.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Create a generator from a 64-bit seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        // splitmix64 to derive a non-zero, well-mixed initial state.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Self {
            state: if z == 0 { 0x9E37_79B9_7F4A_7C15 } else { z },
        }
    }

    /// Return the next 64-bit value in the stream.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Return a float uniformly distributed in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        // Use the top 53 bits for a uniform double.
        ((self.next_u64() >> 11) as f64) * (1.0 / (1u64 << 53) as f64)
    }

    /// Return a float uniformly distributed in `[-amp, amp)`.
    pub fn jitter(&mut self, amp: f64) -> f64 {
        (self.next_f64() - 0.5) * 2.0 * amp
    }
}

// ---------------------------------------------------------------------------
// Shocks
// ---------------------------------------------------------------------------

/// A one-off exogenous disturbance that can be scheduled at a given tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Shock {
    /// Inject `pct` percent of the current base money into households at `tick`.
    Money { tick: usize, pct: f64 },
    /// Multiply the propensity to consume by `1 + pct/100` for `dur` ticks
    /// starting at `tick` (a confidence / demand shock).
    Demand { tick: usize, pct: f64, dur: usize },
    /// Set a policy knob to `value` from `tick` onward (a policy change).
    Policy { tick: usize, knob: Knob, value: f64 },
}

/// A policy setting that can change mid-run through [`Shock::Policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knob {
    /// Annualized policy interest rate, percent.
    InterestRate,
    /// Annualized base-money growth, percent.
    MoneyGrowth,
    /// Flat tax on household income (wages and dividends), percent.
    IncomeTax,
    /// Flat tax on firm profit, percent.
    ProfitTax,
    /// Government purchases, percent of full-employment wage income.
    GovSpending,
}

impl Knob {
    /// Every knob, in display order.
    pub const ALL: [Knob; 5] = [
        Knob::InterestRate,
        Knob::MoneyGrowth,
        Knob::IncomeTax,
        Knob::ProfitTax,
        Knob::GovSpending,
    ];

    /// The knob's name as used by flags, shock specs, and scenario files.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Knob::InterestRate => "interest-rate",
            Knob::MoneyGrowth => "money-growth",
            Knob::IncomeTax => "income-tax",
            Knob::ProfitTax => "profit-tax",
            Knob::GovSpending => "gov-spending",
        }
    }

    /// Look a knob up by name (`_` and `-` are interchangeable).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let name = normalize_key(name);
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Check that `value` is in range for this knob.
    ///
    /// # Errors
    /// Returns a message naming the knob and its valid range.
    pub fn check(self, value: f64) -> Result<(), String> {
        let ok = value.is_finite()
            && match self {
                Knob::InterestRate | Knob::IncomeTax | Knob::ProfitTax | Knob::GovSpending => {
                    (0.0..=100.0).contains(&value)
                }
                Knob::MoneyGrowth => value >= -100.0,
            };
        if ok {
            return Ok(());
        }
        Err(match self {
            Knob::InterestRate => {
                "interest-rate must be between 0 and 100 (annualized percent)".to_string()
            }
            Knob::MoneyGrowth => {
                "money-growth must be finite and >= -100 (annualized percent)".to_string()
            }
            Knob::IncomeTax => "income-tax must be between 0 and 100 (percent)".to_string(),
            Knob::ProfitTax => "profit-tax must be between 0 and 100 (percent)".to_string(),
            Knob::GovSpending => {
                "gov-spending must be between 0 and 100 (percent of full-employment wages)"
                    .to_string()
            }
        })
    }
}

fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace('_', "-")
}

impl Shock {
    /// Parse a shock specification such as `money:100:20` or
    /// `demand:100:50:10`.
    ///
    /// * `money:TICK:PCT` injects `PCT`% of base money at tick `TICK`.
    /// * `demand:TICK:PCT:DUR` raises the propensity to consume by `PCT`%
    ///   for `DUR` ticks starting at `TICK`.
    ///
    /// # Errors
    /// Returns a human-readable message if the spec is malformed.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split(':').collect();
        let kind = parts.first().copied().unwrap_or_default();
        match kind {
            "money" => {
                let [_, tick, pct] = parts.as_slice() else {
                    return Err(format!(
                        "money shock expects 'money:TICK:PCT', got '{spec}'"
                    ));
                };
                let tick = parse_usize(tick, "shock tick")?;
                let pct = parse_f64(pct, "shock percent")?;
                Ok(Shock::Money { tick, pct })
            }
            "demand" => {
                let [_, tick, pct, dur] = parts.as_slice() else {
                    return Err(format!(
                        "demand shock expects 'demand:TICK:PCT:DUR', got '{spec}'"
                    ));
                };
                let tick = parse_usize(tick, "shock tick")?;
                let pct = parse_f64(pct, "shock percent")?;
                let dur = parse_usize(dur, "shock duration")?;
                if dur == 0 {
                    return Err("demand shock duration must be >= 1".to_string());
                }
                Ok(Shock::Demand { tick, pct, dur })
            }
            "set" => {
                let [_, tick, knob, value] = parts.as_slice() else {
                    return Err(format!(
                        "policy change expects 'set:TICK:KNOB:VALUE', got '{spec}'"
                    ));
                };
                let tick = parse_usize(tick, "policy tick")?;
                let knob = Knob::from_name(knob).ok_or_else(|| unknown_knob(knob))?;
                let value = parse_f64(value, "policy value")?;
                Ok(Shock::Policy { tick, knob, value })
            }
            other => Err(format!(
                "unknown shock kind '{other}' (expected 'money', 'demand', or 'set')"
            )),
        }
    }

    /// The tick at which the shock starts.
    #[must_use]
    pub fn tick(&self) -> usize {
        match *self {
            Shock::Money { tick, .. } | Shock::Demand { tick, .. } | Shock::Policy { tick, .. } => {
                tick
            }
        }
    }

    /// The shock as a spec string that [`Shock::parse`] reads back.
    #[must_use]
    pub fn spec(&self) -> String {
        match *self {
            Shock::Money { tick, pct } => format!("money:{tick}:{pct}"),
            Shock::Demand { tick, pct, dur } => format!("demand:{tick}:{pct}:{dur}"),
            Shock::Policy { tick, knob, value } => format!("set:{tick}:{}:{value}", knob.name()),
        }
    }
}

fn unknown_knob(name: &str) -> String {
    let names: Vec<&str> = Knob::ALL.iter().map(|k| k.name()).collect();
    format!(
        "unknown policy knob '{}' (expected one of: {})",
        name.trim(),
        names.join(", ")
    )
}

fn parse_usize(s: &str, what: &str) -> Result<usize, String> {
    s.trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid {what}: '{s}'"))
}

fn parse_f64(s: &str, what: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .map_err(|_| format!("invalid {what}: '{s}'"))
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// All of the knobs that define a simulation run.
#[derive(Debug, Clone)]
pub struct Params {
    /// Number of ticks to simulate.
    pub ticks: usize,
    /// Seed for the deterministic RNG.
    pub seed: u64,
    /// Number of household agents.
    pub households: usize,
    /// Number of firm agents.
    pub firms: usize,
    /// Annualized policy interest rate, in percent (0..=100).
    pub interest_rate: f64,
    /// Annualized money-supply growth, in percent.
    pub money_growth: f64,
    /// Marginal propensity to consume out of money on hand (0..=1).
    pub mpc: f64,
    /// Bank reserve ratio; the money multiplier is `1 / reserve_ratio`.
    pub reserve_ratio: f64,
    /// Flat tax on household income (wages and dividends), percent (0..=100).
    pub income_tax: f64,
    /// Flat tax on firm profit before dividends, percent (0..=100).
    pub profit_tax: f64,
    /// Government purchases per tick, as a percent of full-employment wage
    /// income indexed to the price level (0..=100).
    pub gov_spending: f64,
    /// Scheduled shocks.
    pub shocks: Vec<Shock>,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            ticks: 200,
            seed: 42,
            households: 200,
            firms: 20,
            interest_rate: 5.0,
            money_growth: 2.0,
            mpc: 0.3,
            reserve_ratio: 0.1,
            income_tax: 0.0,
            profit_tax: 0.0,
            gov_spending: 0.0,
            shocks: Vec::new(),
        }
    }
}

impl Params {
    /// Validate every numeric range and return a clear error otherwise.
    ///
    /// # Errors
    /// Returns a message describing the first invalid parameter.
    pub fn validate(&self) -> Result<(), String> {
        if self.ticks == 0 {
            return Err("ticks must be >= 1".to_string());
        }
        if self.households == 0 {
            return Err("households must be >= 1".to_string());
        }
        if self.firms == 0 {
            return Err("firms must be >= 1".to_string());
        }
        for knob in Knob::ALL {
            knob.check(self.knob(knob))?;
        }
        if !(0.0..=1.0).contains(&self.mpc) {
            return Err("mpc must be between 0 and 1".to_string());
        }
        if !(0.001..=1.0).contains(&self.reserve_ratio) {
            return Err("reserve-ratio must be between 0.001 and 1".to_string());
        }
        for shock in &self.shocks {
            let tick = shock.tick();
            if tick >= self.ticks {
                return Err(format!(
                    "shock '{}' is scheduled at tick {tick} but the run is only {} ticks",
                    shock.spec(),
                    self.ticks
                ));
            }
            if let Shock::Policy { knob, value, .. } = *shock {
                knob.check(value)
                    .map_err(|e| format!("in '{}': {e}", shock.spec()))?;
            }
        }
        Ok(())
    }

    /// The setting names accepted by [`Params::set`], as used by flags
    /// (`--NAME`) and scenario files (`NAME = VALUE`).
    pub const KEYS: [&'static str; 13] = [
        "ticks",
        "seed",
        "households",
        "firms",
        "interest-rate",
        "money-growth",
        "mpc",
        "reserve-ratio",
        "income-tax",
        "profit-tax",
        "gov-spending",
        "shock",
        "set",
    ];

    /// Set one parameter from its name and raw text. `shock` appends a shock
    /// spec, and `set` appends a policy change written `TICK:KNOB:VALUE`.
    /// Ranges are checked later by [`Params::validate`].
    ///
    /// # Errors
    /// Returns a message for an unknown name or an unparsable value.
    pub fn set(&mut self, key: &str, raw: &str) -> Result<(), String> {
        let key = normalize_key(key);
        let raw = raw.trim();
        fn num<T: std::str::FromStr>(key: &str, raw: &str) -> Result<T, String> {
            raw.parse::<T>()
                .map_err(|_| format!("invalid value for {key}: '{raw}'"))
        }
        match key.as_str() {
            "ticks" => self.ticks = num(&key, raw)?,
            "seed" => self.seed = num(&key, raw)?,
            "households" => self.households = num(&key, raw)?,
            "firms" => self.firms = num(&key, raw)?,
            "mpc" => self.mpc = num(&key, raw)?,
            "reserve-ratio" => self.reserve_ratio = num(&key, raw)?,
            "shock" => self.shocks.push(Shock::parse(raw)?),
            "set" => self.shocks.push(Shock::parse(&format!("set:{raw}"))?),
            other => match Knob::from_name(other) {
                Some(knob) => *self.knob_mut(knob) = num(&key, raw)?,
                None => return Err(format!("unknown setting '{other}'")),
            },
        }
        Ok(())
    }

    /// The current value of a policy knob.
    #[must_use]
    pub fn knob(&self, knob: Knob) -> f64 {
        match knob {
            Knob::InterestRate => self.interest_rate,
            Knob::MoneyGrowth => self.money_growth,
            Knob::IncomeTax => self.income_tax,
            Knob::ProfitTax => self.profit_tax,
            Knob::GovSpending => self.gov_spending,
        }
    }

    fn knob_mut(&mut self, knob: Knob) -> &mut f64 {
        match knob {
            Knob::InterestRate => &mut self.interest_rate,
            Knob::MoneyGrowth => &mut self.money_growth,
            Knob::IncomeTax => &mut self.income_tax,
            Knob::ProfitTax => &mut self.profit_tax,
            Knob::GovSpending => &mut self.gov_spending,
        }
    }

    /// Whether the run has a government sector at any point: a tax or
    /// spending setting above zero, or a policy change that sets one.
    #[must_use]
    pub fn has_government(&self) -> bool {
        self.income_tax > 0.0
            || self.profit_tax > 0.0
            || self.gov_spending > 0.0
            || self.shocks.iter().any(|s| {
                matches!(
                    s,
                    Shock::Policy {
                        knob: Knob::IncomeTax | Knob::ProfitTax | Knob::GovSpending,
                        ..
                    }
                )
            })
    }
}

// ---------------------------------------------------------------------------
// Time series & summary
// ---------------------------------------------------------------------------

/// The tracked macro time series, one entry per tick.
#[derive(Debug, Clone, Default)]
pub struct Series {
    /// Consumer price index, base 100 at t0.
    pub cpi: Vec<f64>,
    /// Real output (units of goods sold per tick).
    pub gdp: Vec<f64>,
    /// Unemployment rate in `[0, 1]`.
    pub unemployment: Vec<f64>,
    /// Average money (deposits) held per household.
    pub avg_money: Vec<f64>,
    /// Total credit (loans outstanding).
    pub credit: Vec<f64>,
    /// Period-over-period CPI inflation, in percent.
    pub inflation: Vec<f64>,
    /// Broad money: total deposits held by households and firms.
    pub broad_money: Vec<f64>,
    /// Nominal output: the value of goods sold this tick.
    pub nominal_gdp: Vec<f64>,
    /// Tax revenue collected this tick (income plus profit tax).
    pub taxes: Vec<f64>,
    /// Government purchases paid for this tick.
    pub gov_spending: Vec<f64>,
    /// Government deficit this tick: spending plus debt interest minus taxes.
    pub deficit: Vec<f64>,
    /// Public debt at the end of the tick (negative means net savings).
    pub gov_debt: Vec<f64>,
}

impl Series {
    /// Number of recorded ticks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cpi.len()
    }

    /// Whether no tick has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cpi.is_empty()
    }
}

/// Final indicators summarizing a completed run.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    /// Final CPI level.
    pub cpi: f64,
    /// Average annualized inflation over the run, in percent.
    pub avg_inflation: f64,
    /// Final real output.
    pub gdp: f64,
    /// Final unemployment rate.
    pub unemployment: f64,
    /// Final average household money.
    pub avg_money: f64,
    /// Final total credit outstanding.
    pub credit: f64,
    /// Final broad money.
    pub broad_money: f64,
    /// Conserved base money.
    pub base_money: f64,
    /// Final tick's tax revenue.
    pub taxes: f64,
    /// Final tick's government purchases.
    pub gov_spending: f64,
    /// Final tick's deficit.
    pub deficit: f64,
    /// Final public debt (negative means net savings).
    pub gov_debt: f64,
    /// Final public debt as a percent of annualized nominal output.
    pub debt_to_gdp: f64,
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Household {
    money: f64,
    noise: f64,
}

#[derive(Debug, Clone)]
struct Firm {
    money: f64,
    loan: f64,
    inventory: f64,
    price: f64,
    exp_sales: f64,
}

#[derive(Debug, Clone, Default)]
struct Bank {
    equity: f64,
}

// ---------------------------------------------------------------------------
// Simulation
// ---------------------------------------------------------------------------

/// A running economy. Build with [`Simulation::new`], advance with
/// [`Simulation::step`], or run to completion with [`Simulation::run`].
#[derive(Debug, Clone)]
pub struct Simulation {
    params: Params,
    rng: Rng,
    households: Vec<Household>,
    firms: Vec<Firm>,
    bank: Bank,
    /// Conserved outside/base money. Only money growth and money shocks change
    /// it; every other operation preserves the identity
    /// `base_money == total_deposits + bank_equity - total_loans`.
    base_money: f64,
    /// The treasury's account at the bank. It goes negative when spending
    /// runs ahead of taxes (an overdraft standing in for bond sales); minus
    /// this balance is the public debt.
    gov_balance: f64,
    init_price_mean: f64,
    tick: usize,
    series: Series,
}

impl Simulation {
    /// Create a new simulation from validated parameters.
    ///
    /// The caller should have run [`Params::validate`] first; `new` assumes the
    /// parameters are sane.
    #[must_use]
    pub fn new(params: &Params) -> Self {
        let mut rng = Rng::new(params.seed);

        let init_money = WAGE / params.mpc.max(EPS);
        let households: Vec<Household> = (0..params.households)
            .map(|_| Household {
                money: init_money * (1.0 + rng.jitter(0.10)),
                noise: 0.0,
            })
            .collect();

        let seed_sales = (params.households as f64 * WAGE) / params.firms as f64;
        let firms: Vec<Firm> = (0..params.firms)
            .map(|_| {
                let price = 1.0 * (1.0 + rng.jitter(0.05));
                Firm {
                    money: 0.0,
                    loan: 0.0,
                    inventory: seed_sales,
                    price,
                    exp_sales: seed_sales,
                }
            })
            .collect();

        let init_price_mean = mean(firms.iter().map(|f| f.price));
        let base_money: f64 = households.iter().map(|h| h.money).sum();

        Self {
            params: params.clone(),
            rng,
            households,
            firms,
            bank: Bank::default(),
            base_money,
            gov_balance: 0.0,
            init_price_mean,
            tick: 0,
            series: Series::default(),
        }
    }

    /// Public debt: minus the treasury's balance (negative means net savings).
    #[must_use]
    pub fn gov_debt(&self) -> f64 {
        0.0 - self.gov_balance
    }

    /// The parameters in force now, after any policy changes so far.
    #[must_use]
    pub fn current_params(&self) -> &Params {
        &self.params
    }

    /// Total deposits held across households and firms (broad money).
    #[must_use]
    pub fn total_deposits(&self) -> f64 {
        let h: f64 = self.households.iter().map(|x| x.money).sum();
        let f: f64 = self.firms.iter().map(|x| x.money).sum();
        h + f
    }

    /// Total loans outstanding.
    #[must_use]
    pub fn total_loans(&self) -> f64 {
        self.firms.iter().map(|x| x.loan).sum()
    }

    /// Accumulated bank equity (retained interest).
    #[must_use]
    pub fn bank_equity(&self) -> f64 {
        self.bank.equity
    }

    /// The conserved base-money quantity.
    #[must_use]
    pub fn base_money(&self) -> f64 {
        self.base_money
    }

    /// The residual of the accounting identity
    /// `base_money - (deposits + treasury balance + equity - loans)`. It must
    /// stay ~0 every tick.
    #[must_use]
    pub fn accounting_residual(&self) -> f64 {
        self.base_money
            - (self.total_deposits() + self.gov_balance + self.bank.equity - self.total_loans())
    }

    /// Borrow the recorded time series.
    #[must_use]
    pub fn series(&self) -> &Series {
        &self.series
    }

    /// The per-tick interest rate implied by the annualized policy rate.
    #[must_use]
    pub fn rate_per_tick(&self) -> f64 {
        self.params.interest_rate / 100.0 / PERIODS_PER_YEAR
    }

    /// Inject `amount` of new base money, split equally across households.
    /// A negative amount withdraws money, but never more than a household
    /// holds; base money falls by what was actually withdrawn.
    fn inject(&mut self, amount: f64) {
        if amount == 0.0 {
            return;
        }
        let per = amount / self.households.len() as f64;
        let mut applied = 0.0;
        for h in &mut self.households {
            let delta = if per < 0.0 {
                per.max(-h.money.max(0.0))
            } else {
                per
            };
            h.money += delta;
            applied += delta;
        }
        self.base_money += if per < 0.0 { applied } else { amount };
    }

    /// Advance the economy by exactly one tick.
    ///
    /// The ordered sequence is: policy changes, money growth, scheduled
    /// shocks, output & hiring plan, wage financing, payment and income tax,
    /// goods-market clearing (households and government) with price
    /// adjustment, interest accrual, repayment, profit tax and dividends, then
    /// metrics.
    pub fn step(&mut self) {
        let t = self.tick;

        // 0. Policy changes take effect at the start of their tick.
        let changes: Vec<(Knob, f64)> = self
            .params
            .shocks
            .iter()
            .filter_map(|s| match *s {
                Shock::Policy { tick, knob, value } if tick == t => Some((knob, value)),
                _ => None,
            })
            .collect();
        for (knob, value) in changes {
            *self.params.knob_mut(knob) = value;
        }

        let rate = self.rate_per_tick();
        let households = self.households.len() as f64;
        let income_tax = self.params.income_tax / 100.0;
        let profit_tax = self.params.profit_tax / 100.0;
        let mut taxes = 0.0;

        // 1. Money-supply growth (annualized percent -> per tick).
        let mg_tick = self.params.money_growth / 100.0 / PERIODS_PER_YEAR;
        if mg_tick != 0.0 {
            self.inject(self.base_money * mg_tick);
        }

        // 2. Scheduled shocks. Accumulate the effects first so we do not need
        //    a mutable borrow of `self` while reading the shock schedule.
        let mut demand_mult = 1.0;
        let mut shock_injection = 0.0;
        for shock in &self.params.shocks {
            match *shock {
                Shock::Money { tick, pct } if tick == t => {
                    shock_injection += self.base_money * pct / 100.0;
                }
                Shock::Demand { tick, pct, dur } if t >= tick && t < tick + dur => {
                    demand_mult *= 1.0 + pct / 100.0;
                }
                _ => {}
            }
        }
        self.inject(shock_injection);

        // 3. Output & hiring plan. Firms scale planned output by profitability;
        //    financing cost rises with the interest rate, damping output.
        let unit_cost = WAGE / PRODUCTIVITY + WAGE * rate * BUFFER_FACTOR;
        let mut desired_workers: Vec<f64> = self
            .firms
            .iter()
            .map(|f| {
                let margin = (f.price - unit_cost) / unit_cost.max(EPS);
                let desired = (f.exp_sales * (1.0 + OUTPUT_ADJ * margin.clamp(-1.0, 1.0))).max(0.0);
                desired / PRODUCTIVITY
            })
            .collect();

        // Labor supply is fixed at one unit per household; ration if scarce.
        let demand_labor: f64 = desired_workers.iter().sum();
        if demand_labor > households {
            let scale = households / demand_labor.max(EPS);
            for w in &mut desired_workers {
                *w *= scale;
            }
        }

        // 4. Wage financing and payment. Firms borrow the shortfall subject to
        //    the reserve constraint; if credit is scarce they hire fewer.
        //    Loans create deposits, so borrowing raises the running deposit
        //    total against which the money-multiplier cap is measured.
        //    The treasury's balance counts with deposits, so the cap works out
        //    as a limit on private loans against base money: government
        //    borrowing neither crowds out nor adds to private credit.
        let deposit_cap = self.base_money / self.params.reserve_ratio;
        let mut deposits = self.total_deposits() + self.gov_balance;
        let mut wage_pool = 0.0;
        let mut employment = 0.0;
        for (f, workers) in self.firms.iter_mut().zip(desired_workers.iter_mut()) {
            let mut wage_bill = *workers * WAGE;
            let need = wage_bill - f.money;
            if need > 0.0 {
                let available = (deposit_cap - deposits).max(0.0);
                let borrow = need.min(available);
                f.money += borrow;
                f.loan += borrow;
                deposits += borrow;
                if borrow < need {
                    // Credit-constrained: hire only what the firm can pay for.
                    *workers = f.money / WAGE;
                    wage_bill = f.money;
                }
            }
            f.money -= wage_bill;
            f.inventory += *workers * PRODUCTIVITY;
            wage_pool += wage_bill;
            employment += *workers;
        }
        // Working-capital credit outstanding at its within-tick peak (before
        // revenue lets firms repay). This is the credit series we report.
        let peak_credit = self.total_loans();
        // Distribute wages uniformly across the labor force (representative
        // households); unemployment is tracked as the utilization gap. The
        // income tax is withheld first.
        let wage_tax = wage_pool * income_tax;
        taxes += wage_tax;
        self.gov_balance += wage_tax;
        let wage_each = (wage_pool - wage_tax) / households;
        for h in &mut self.households {
            h.money += wage_each;
        }

        // 5. Goods market: households spend `mpc * money`, firms clear demand
        //    against inventory and adjust prices toward market balance.
        for h in &mut self.households {
            h.noise = 1.0 + self.rng.jitter(0.03);
        }
        let effective_mpc = self.params.mpc * demand_mult;
        let intents: Vec<f64> = self
            .households
            .iter()
            .map(|h| (effective_mpc * h.money * h.noise).clamp(0.0, h.money.max(0.0)))
            .collect();
        // Government purchases: a share of full-employment wage income,
        // indexed to last tick's price level so the real program is stable.
        let price_level = last_or(&self.series.cpi, 100.0) / 100.0;
        let gov_demand = self.params.gov_spending / 100.0 * households * WAGE * price_level;
        let household_demand: f64 = intents.iter().sum();
        let nominal_demand = household_demand + gov_demand;
        let total_inv: f64 = self.firms.iter().map(|f| f.inventory).sum();

        let mut total_sales = 0.0;
        let mut real_output = 0.0;
        for f in &mut self.firms {
            let supply = f.inventory;
            let receipts = if total_inv > EPS {
                nominal_demand * (supply / total_inv)
            } else {
                0.0
            };
            let desired_units = if f.price > EPS {
                receipts / f.price
            } else {
                0.0
            };
            let units_sold = desired_units.min(supply).max(0.0);
            let sale = units_sold * f.price;
            f.money += sale;
            f.inventory -= units_sold;
            total_sales += sale;
            real_output += units_sold;

            let signal = if supply > EPS {
                ((desired_units - supply) / supply).clamp(-1.0, 1.0)
            } else if nominal_demand > EPS {
                1.0
            } else {
                0.0
            };
            f.price = (f.price * (1.0 + PRICE_ADJ * signal)).max(1e-6);
            f.exp_sales = (0.5 * f.exp_sales + 0.5 * units_sold).max(EPS);
        }

        // Debit households in proportion to how much of demand actually cleared.
        let fill = if nominal_demand > EPS {
            total_sales / nominal_demand
        } else {
            0.0
        };
        for (h, intent) in self.households.iter_mut().zip(intents.iter()) {
            h.money -= intent * fill;
        }
        let gov_paid = gov_demand * fill;
        self.gov_balance -= gov_paid;

        // 6. Interest accrual: borrowers pay interest on the working-capital
        //    loan they carried through the tick. Interest flows to the bank as
        //    equity, permanently leaving circulation -- this is the toy's
        //    monetary-transmission channel. Unpayable interest is capitalized
        //    onto the loan, preserving the accounting identity.
        for f in &mut self.firms {
            let interest = f.loan * rate;
            f.money -= interest;
            if f.money < 0.0 {
                let shortfall = -f.money;
                f.loan += shortfall;
                f.money = 0.0;
            }
            self.bank.equity += interest;
        }
        // The treasury pays the policy rate on its overdraft (the public debt).
        let debt_interest = (0.0 - self.gov_balance).max(0.0) * rate;
        self.gov_balance -= debt_interest;
        self.bank.equity += debt_interest;

        // 7. Repayment and dividends. Firms run lean: they repay their loan
        //    from revenue and pay out any remaining profit to households as
        //    dividends, so next tick they must borrow the wage bill again. This
        //    keeps credit and interest flowing every tick.
        let mut dividend_pool = 0.0;
        for f in &mut self.firms {
            let repay = f.loan.min(f.money.max(0.0));
            f.money -= repay;
            f.loan -= repay;
            if f.money > 0.0 {
                let tax = f.money * profit_tax;
                taxes += tax;
                self.gov_balance += tax;
                dividend_pool += f.money - tax;
                f.money = 0.0;
            }
        }
        let dividend_tax = dividend_pool * income_tax;
        taxes += dividend_tax;
        self.gov_balance += dividend_tax;
        let dividend_each = (dividend_pool - dividend_tax) / households;
        for h in &mut self.households {
            h.money += dividend_each;
        }

        // 8. Metrics.
        let cpi = 100.0 * mean(self.firms.iter().map(|f| f.price)) / self.init_price_mean.max(EPS);
        let unemployment = (1.0 - employment / households).clamp(0.0, 1.0);
        let avg_money = mean(self.households.iter().map(|h| h.money));
        let credit = peak_credit;
        let broad_money = self.total_deposits();
        let inflation = match self.series.cpi.last() {
            Some(&prev) if prev > EPS => (cpi / prev - 1.0) * 100.0,
            _ => 0.0,
        };

        self.series.cpi.push(cpi);
        self.series.gdp.push(real_output);
        self.series.unemployment.push(unemployment);
        self.series.avg_money.push(avg_money);
        self.series.credit.push(credit);
        self.series.inflation.push(inflation);
        self.series.broad_money.push(broad_money);

        self.series.nominal_gdp.push(total_sales);
        self.series.taxes.push(taxes);
        self.series.gov_spending.push(gov_paid);
        self.series.deficit.push(gov_paid + debt_interest - taxes);
        self.series.gov_debt.push(0.0 - self.gov_balance);

        self.tick += 1;
    }

    /// Run every remaining tick and return a final [`Summary`].
    pub fn run(&mut self) -> Summary {
        while self.tick < self.params.ticks {
            self.step();
        }
        self.summary()
    }

    /// Compute the final-indicator summary from the recorded series.
    #[must_use]
    pub fn summary(&self) -> Summary {
        let s = &self.series;
        let avg_inflation = if s.inflation.is_empty() {
            0.0
        } else {
            // Period inflation averaged, then annualized.
            let mean_period = s.inflation.iter().sum::<f64>() / s.inflation.len() as f64;
            mean_period * PERIODS_PER_YEAR
        };
        Summary {
            cpi: last_or(&s.cpi, 100.0),
            avg_inflation,
            gdp: last_or(&s.gdp, 0.0),
            unemployment: last_or(&s.unemployment, 0.0),
            avg_money: last_or(&s.avg_money, 0.0),
            credit: last_or(&s.credit, 0.0),
            broad_money: last_or(&s.broad_money, 0.0),
            base_money: self.base_money,
            taxes: last_or(&s.taxes, 0.0),
            gov_spending: last_or(&s.gov_spending, 0.0),
            deficit: last_or(&s.deficit, 0.0),
            gov_debt: 0.0 - self.gov_balance,
            debt_to_gdp: {
                let annual = last_or(&s.nominal_gdp, 0.0) * PERIODS_PER_YEAR;
                if annual > EPS {
                    (0.0 - self.gov_balance) / annual * 100.0
                } else {
                    0.0
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Small numeric helpers
// ---------------------------------------------------------------------------

fn mean<I: Iterator<Item = f64>>(iter: I) -> f64 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for x in iter {
        sum += x;
        count += 1.0;
    }
    if count == 0.0 {
        0.0
    } else {
        sum / count
    }
}

fn last_or(v: &[f64], default: f64) -> f64 {
    v.last().copied().unwrap_or(default)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render `data` as a single-line block sparkline of at most `width` glyphs.
///
/// If the data is longer than `width` it is averaged into `width` buckets; if
/// it is shorter the output has one glyph per point. A flat series renders as a
/// row of mid-height blocks. All glyphs are drawn from [`SPARK_BLOCKS`].
#[must_use]
pub fn sparkline(data: &[f64], width: usize) -> String {
    if data.is_empty() || width == 0 {
        return String::new();
    }
    let buckets = width.min(data.len());
    let sampled: Vec<f64> = (0..buckets)
        .map(|i| {
            let start = i * data.len() / buckets;
            let end = ((i + 1) * data.len() / buckets)
                .max(start + 1)
                .min(data.len());
            let slice = &data[start..end];
            slice.iter().sum::<f64>() / slice.len() as f64
        })
        .collect();

    let min = sampled.iter().copied().fold(f64::INFINITY, f64::min);
    let max = sampled.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = max - min;

    sampled
        .iter()
        .map(|&v| {
            let level = if span <= EPS {
                SPARK_BLOCKS.len() / 2
            } else {
                let norm = (v - min) / span;
                ((norm * (SPARK_BLOCKS.len() - 1) as f64).round() as usize)
                    .min(SPARK_BLOCKS.len() - 1)
            };
            SPARK_BLOCKS[level]
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Serialization (hand-rolled, zero dependencies)
// ---------------------------------------------------------------------------

/// Serialize the run as CSV with a header and one row per tick.
#[must_use]
pub fn to_csv(series: &Series) -> String {
    let mut out = String::from(
        "tick,cpi,gdp,unemployment,inflation,avg_money,credit,broad_money,\
         nominal_gdp,taxes,gov_spending,deficit,gov_debt\n",
    );
    for i in 0..series.len() {
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{},{},{}",
            i,
            series.cpi[i],
            series.gdp[i],
            series.unemployment[i],
            series.inflation[i],
            series.avg_money[i],
            series.credit[i],
            series.broad_money[i],
            series.nominal_gdp[i],
            series.taxes[i],
            series.gov_spending[i],
            series.deficit[i],
            series.gov_debt[i],
        );
    }
    out
}

/// Serialize the whole run (parameters, series, summary) as JSON.
/// `scenario` is the scenario name, when the run came from one.
#[must_use]
pub fn to_json(
    params: &Params,
    series: &Series,
    summary: &Summary,
    scenario: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    if let Some(name) = scenario {
        let _ = writeln!(out, "  \"scenario\": {},", json_string(name));
    }

    // params
    out.push_str("  \"params\": {\n");
    let _ = writeln!(out, "    \"ticks\": {},", params.ticks);
    let _ = writeln!(out, "    \"seed\": {},", params.seed);
    let _ = writeln!(out, "    \"households\": {},", params.households);
    let _ = writeln!(out, "    \"firms\": {},", params.firms);
    let _ = writeln!(out, "    \"interest_rate\": {},", params.interest_rate);
    let _ = writeln!(out, "    \"money_growth\": {},", params.money_growth);
    let _ = writeln!(out, "    \"mpc\": {},", params.mpc);
    let _ = writeln!(out, "    \"reserve_ratio\": {},", params.reserve_ratio);
    let _ = writeln!(out, "    \"income_tax\": {},", params.income_tax);
    let _ = writeln!(out, "    \"profit_tax\": {},", params.profit_tax);
    let _ = writeln!(out, "    \"gov_spending\": {},", params.gov_spending);
    let specs: Vec<String> = params
        .shocks
        .iter()
        .map(|s| json_string(&s.spec()))
        .collect();
    let _ = writeln!(out, "    \"shocks\": [{}]", specs.join(", "));
    out.push_str("  },\n");

    // series
    out.push_str("  \"series\": {\n");
    push_json_array(&mut out, "cpi", &series.cpi, false);
    push_json_array(&mut out, "gdp", &series.gdp, false);
    push_json_array(&mut out, "unemployment", &series.unemployment, false);
    push_json_array(&mut out, "inflation", &series.inflation, false);
    push_json_array(&mut out, "avg_money", &series.avg_money, false);
    push_json_array(&mut out, "credit", &series.credit, false);
    push_json_array(&mut out, "broad_money", &series.broad_money, false);
    push_json_array(&mut out, "nominal_gdp", &series.nominal_gdp, false);
    push_json_array(&mut out, "taxes", &series.taxes, false);
    push_json_array(&mut out, "gov_spending", &series.gov_spending, false);
    push_json_array(&mut out, "deficit", &series.deficit, false);
    push_json_array(&mut out, "gov_debt", &series.gov_debt, true);
    out.push_str("  },\n");

    // summary
    out.push_str("  \"summary\": {\n");
    let _ = writeln!(out, "    \"cpi\": {},", summary.cpi);
    let _ = writeln!(out, "    \"avg_inflation\": {},", summary.avg_inflation);
    let _ = writeln!(out, "    \"gdp\": {},", summary.gdp);
    let _ = writeln!(out, "    \"unemployment\": {},", summary.unemployment);
    let _ = writeln!(out, "    \"avg_money\": {},", summary.avg_money);
    let _ = writeln!(out, "    \"credit\": {},", summary.credit);
    let _ = writeln!(out, "    \"broad_money\": {},", summary.broad_money);
    let _ = writeln!(out, "    \"base_money\": {},", summary.base_money);
    let _ = writeln!(out, "    \"taxes\": {},", summary.taxes);
    let _ = writeln!(out, "    \"gov_spending\": {},", summary.gov_spending);
    let _ = writeln!(out, "    \"deficit\": {},", summary.deficit);
    let _ = writeln!(out, "    \"gov_debt\": {},", summary.gov_debt);
    let _ = writeln!(out, "    \"debt_to_gdp\": {}", summary.debt_to_gdp);
    out.push_str("  }\n");

    out.push_str("}\n");
    out
}

fn push_json_array(out: &mut String, name: &str, data: &[f64], last: bool) {
    let _ = write!(out, "    \"{name}\": [");
    for (i, v) in data.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{v}");
    }
    out.push(']');
    out.push_str(if last { "\n" } else { ",\n" });
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
