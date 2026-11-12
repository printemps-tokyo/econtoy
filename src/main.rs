//! Command-line entry point for econtoy.
//!
//! The argument parser is hand-rolled (zero dependencies, matching the crate's
//! no-crate policy) and supports `--help` / `--version` plus the simulation
//! knobs and output modes.

use std::process::ExitCode;

use econtoy::scenario::{self, Scenario, BUILTIN_SCENARIOS};
use econtoy::{sparkline, to_csv, to_json, Params, Series, Shock, Simulation, Summary};

const HELP: &str = "\
econtoy - a tiny agent-based economy simulator

USAGE:
    econtoy [OPTIONS]

Watch emergent macro behavior (inflation, unemployment, credit-driven booms and
busts) as you turn knobs like the interest rate and money-supply growth. This is
an educational emergence toy, not a real macroeconomic model.

OPTIONS:
    --ticks <N>            Number of ticks to simulate [default: 200]
    --seed <N>             Seed for the deterministic RNG [default: 42]
    --households <N>       Number of household agents [default: 200]
    --firms <N>            Number of firm agents [default: 20]
    --interest-rate <PCT>  Annualized policy interest rate, percent 0..=100 [default: 5]
    --money-growth <PCT>   Annualized money-supply growth, percent [default: 2]
    --mpc <F>              Marginal propensity to consume, 0..=1 [default: 0.3]
    --reserve-ratio <F>    Bank reserve ratio; multiplier is 1/ratio [default: 0.1]
    --income-tax <PCT>     Tax on household wages and dividends, 0..=100 [default: 0]
    --profit-tax <PCT>     Tax on firm profit, 0..=100 [default: 0]
    --gov-spending <PCT>   Government purchases, percent of full-employment
                           wage income (price indexed), 0..=100 [default: 0]
    --shock <SPEC>         Add a shock or policy change (repeatable). Specs:
                             money:TICK:PCT          inject PCT% of base money at TICK
                             demand:TICK:PCT:DUR     raise MPC by PCT% for DUR ticks
                             set:TICK:KNOB:VALUE     set a policy knob from TICK on
                           Knobs: interest-rate, money-growth, income-tax,
                           profit-tax, gov-spending
    --scenario <NAME|FILE> Load a scenario file, or a built-in scenario by
                           name. Flags after it override its settings
    --list-scenarios       List the built-in scenarios
    --json                 Emit the full time series and summary as JSON
    --csv                  Emit one CSV row per tick
    --quiet                Emit only the final summary line
    -h, --help             Print help
    -V, --version          Print version

EXAMPLES:
    econtoy
    econtoy --interest-rate 12 --money-growth 6 --ticks 300
    econtoy --shock money:120:25 --json
    econtoy --income-tax 20 --gov-spending 25 --shock set:100:gov-spending:40
    econtoy --scenario stimulus
    econtoy --scenario my-economy.scenario --ticks 400
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Clone, Copy)]
enum OutputMode {
    Human,
    Json,
    Csv,
    Quiet,
}

fn run() -> Result<(), String> {
    let mut params = Params::default();
    let mut mode = OutputMode::Human;
    let mut scenario: Option<Scenario> = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("econtoy {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--list-scenarios" => {
                list_scenarios();
                return Ok(());
            }
            "--json" => mode = OutputMode::Json,
            "--csv" => mode = OutputMode::Csv,
            "--quiet" => mode = OutputMode::Quiet,
            "--scenario" => {
                let target = next_value(&mut args, "--scenario")?;
                scenario = Some(load_scenario(&target, &mut params)?);
            }
            flag => {
                let key = flag
                    .strip_prefix("--")
                    .filter(|k| *k != "set" && Params::KEYS.contains(k))
                    .ok_or_else(|| format!("unknown argument '{flag}' (try --help)"))?;
                let value = next_value(&mut args, flag)?;
                params.set(key, &value)?;
            }
        }
    }

    params.validate()?;

    let mut sim = Simulation::new(&params);
    let summary = sim.run();
    let series = sim.series();
    let name = scenario.as_ref().and_then(|s| s.name.as_deref());

    match mode {
        OutputMode::Json => print!("{}", to_json(&params, series, &summary, name)),
        OutputMode::Csv => print!("{}", to_csv(series)),
        OutputMode::Quiet => println!("{}", summary_line(&params, &summary)),
        OutputMode::Human => print_human(&params, scenario.as_ref(), series, &summary),
    }

    Ok(())
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

/// Load `target` as a file when it names one (or looks like a path), and as a
/// built-in scenario otherwise.
fn load_scenario(target: &str, params: &mut Params) -> Result<Scenario, String> {
    let path = std::path::Path::new(target);
    let looks_like_path = target.contains('/') || target.contains('.') || path.exists();
    let (label, text) = if looks_like_path {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read scenario '{target}': {e}"))?;
        (target.to_string(), text)
    } else if let Some(text) = scenario::builtin(target) {
        (format!("built-in scenario '{target}'"), text.to_string())
    } else {
        let names: Vec<&str> = BUILTIN_SCENARIOS.iter().map(|(n, _)| *n).collect();
        return Err(format!(
            "no scenario file or built-in scenario named '{target}' (built-in: {})",
            names.join(", ")
        ));
    };
    scenario::apply(&text, params).map_err(|e| format!("{label}: {e}"))
}

fn list_scenarios() {
    for (name, text) in BUILTIN_SCENARIOS {
        let mut params = Params::default();
        let description = scenario::apply(text, &mut params)
            .ok()
            .and_then(|s| s.description)
            .unwrap_or_default();
        println!("{name:<14} {description}");
    }
}

fn summary_line(params: &Params, s: &Summary) -> String {
    let mut line = format!(
        "CPI={:.1} inflation={:.2}%/yr GDP={:.1} unemployment={:.1}% avg_money={:.2} credit={:.1}",
        s.cpi,
        s.avg_inflation,
        s.gdp,
        s.unemployment * 100.0,
        s.avg_money,
        s.credit,
    );
    if params.has_government() {
        line.push_str(&format!(
            " deficit={:.2} debt={:.1} debt/GDP={:.1}%",
            s.deficit, s.gov_debt, s.debt_to_gdp
        ));
    }
    line
}

fn print_human(params: &Params, scenario: Option<&Scenario>, series: &Series, summary: &Summary) {
    let width = term_width().clamp(20, 80);

    println!("econtoy - mini economy simulation (educational toy, not a real model)");
    if let Some(name) = scenario.and_then(|s| s.name.as_deref()) {
        println!("scenario: {name}");
    }
    if let Some(description) = scenario.and_then(|s| s.description.as_deref()) {
        println!("  {description}");
    }
    println!(
        "params: ticks={} seed={} households={} firms={} rate={}% money-growth={}% mpc={} reserve={}",
        params.ticks,
        params.seed,
        params.households,
        params.firms,
        params.interest_rate,
        params.money_growth,
        params.mpc,
        params.reserve_ratio,
    );
    if params.has_government() {
        println!(
            "government: income-tax={}% profit-tax={}% gov-spending={}%",
            params.income_tax, params.profit_tax, params.gov_spending,
        );
    }
    if !params.shocks.is_empty() {
        println!("shocks: {}", describe_shocks(&params.shocks));
    }
    println!();

    println!("final indicators");
    println!("  CPI (base 100)      {:>10.2}", summary.cpi);
    println!("  avg inflation       {:>9.2}%/yr", summary.avg_inflation);
    println!("  real output (GDP)   {:>10.2}", summary.gdp);
    println!(
        "  unemployment        {:>9.2}%",
        summary.unemployment * 100.0
    );
    println!("  avg household money  {:>9.2}", summary.avg_money);
    println!("  credit outstanding  {:>10.2}", summary.credit);
    println!("  broad money         {:>10.2}", summary.broad_money);
    println!("  base money          {:>10.2}", summary.base_money);
    if params.has_government() {
        println!("  tax revenue         {:>10.2}", summary.taxes);
        println!("  gov spending        {:>10.2}", summary.gov_spending);
        println!("  deficit             {:>10.2}", summary.deficit);
        println!("  public debt         {:>10.2}", summary.gov_debt);
        println!("  debt / annual GDP   {:>9.1}%", summary.debt_to_gdp);
    }
    println!();

    let chart_w = width.saturating_sub(6);
    println!("time series (t0 -> t{})", series.len().saturating_sub(1));
    print_chart("CPI  ", &series.cpi, chart_w);
    print_chart("GDP  ", &series.gdp, chart_w);
    print_chart("Unemp", &series.unemployment, chart_w);
    print_chart("Loans", &series.credit, chart_w);
    if params.has_government() {
        print_chart("Defic", &series.deficit, chart_w);
        print_chart("Debt ", &series.gov_debt, chart_w);
    }
}

fn print_chart(label: &str, data: &[f64], width: usize) {
    let spark = sparkline(data, width);
    let min = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    println!("{label} {spark}  [{min:.2} .. {max:.2}]");
}

fn describe_shocks(shocks: &[Shock]) -> String {
    shocks
        .iter()
        .map(|s| match *s {
            Shock::Money { tick, pct } => format!("money({pct:+}% @t{tick})"),
            Shock::Demand { tick, pct, dur } => format!("demand({pct:+}% @t{tick} for {dur})"),
            Shock::Policy { tick, knob, value } => format!("{}={value} @t{tick}", knob.name()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Best-effort terminal width from the `COLUMNS` environment variable.
fn term_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(80)
}
