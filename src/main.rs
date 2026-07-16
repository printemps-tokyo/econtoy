//! Command-line entry point for econtoy.
//!
//! The argument parser is hand-rolled (zero dependencies, matching the crate's
//! no-crate policy) and supports `--help` / `--version` plus the simulation
//! knobs and output modes.

use std::process::ExitCode;

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
    --shock <SPEC>         Add a shock (repeatable). Specs:
                             money:TICK:PCT        inject PCT% of base money at TICK
                             demand:TICK:PCT:DUR   raise MPC by PCT% for DUR ticks
    --json                 Emit the full time series and summary as JSON
    --csv                  Emit one CSV row per tick
    --quiet                Emit only the final summary line
    -h, --help             Print help
    -V, --version          Print version

EXAMPLES:
    econtoy
    econtoy --interest-rate 12 --money-growth 6 --ticks 300
    econtoy --shock money:120:25 --json
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
            "--json" => mode = OutputMode::Json,
            "--csv" => mode = OutputMode::Csv,
            "--quiet" => mode = OutputMode::Quiet,
            "--ticks" => params.ticks = parse_next(&mut args, "--ticks")?,
            "--seed" => params.seed = parse_next(&mut args, "--seed")?,
            "--households" => params.households = parse_next(&mut args, "--households")?,
            "--firms" => params.firms = parse_next(&mut args, "--firms")?,
            "--interest-rate" => params.interest_rate = parse_next(&mut args, "--interest-rate")?,
            "--money-growth" => params.money_growth = parse_next(&mut args, "--money-growth")?,
            "--mpc" => params.mpc = parse_next(&mut args, "--mpc")?,
            "--reserve-ratio" => params.reserve_ratio = parse_next(&mut args, "--reserve-ratio")?,
            "--shock" => {
                let spec = args
                    .next()
                    .ok_or_else(|| "--shock requires a value".to_string())?;
                params.shocks.push(Shock::parse(&spec)?);
            }
            other => {
                return Err(format!("unknown argument '{other}' (try --help)"));
            }
        }
    }

    params.validate()?;

    let mut sim = Simulation::new(&params);
    let summary = sim.run();
    let series = sim.series();

    match mode {
        OutputMode::Json => print!("{}", to_json(&params, series, &summary)),
        OutputMode::Csv => print!("{}", to_csv(series)),
        OutputMode::Quiet => println!("{}", summary_line(&summary)),
        OutputMode::Human => print_human(&params, series, &summary),
    }

    Ok(())
}

fn parse_next<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<T, String> {
    let raw = args
        .next()
        .ok_or_else(|| format!("{flag} requires a value"))?;
    raw.parse::<T>()
        .map_err(|_| format!("invalid value for {flag}: '{raw}'"))
}

fn summary_line(s: &Summary) -> String {
    format!(
        "CPI={:.1} inflation={:.2}%/yr GDP={:.1} unemployment={:.1}% avg_money={:.2} credit={:.1}",
        s.cpi,
        s.avg_inflation,
        s.gdp,
        s.unemployment * 100.0,
        s.avg_money,
        s.credit,
    )
}

fn print_human(params: &Params, series: &Series, summary: &Summary) {
    let width = term_width().clamp(20, 80);

    println!("econtoy - mini economy simulation (educational toy, not a real model)");
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
    println!();

    let chart_w = width.saturating_sub(6);
    println!("time series (t0 -> t{})", series.len().saturating_sub(1));
    print_chart("CPI  ", &series.cpi, chart_w);
    print_chart("GDP  ", &series.gdp, chart_w);
    print_chart("Unemp", &series.unemployment, chart_w);
    print_chart("Loans", &series.credit, chart_w);
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
            Shock::Money { tick, pct } => format!("money(+{pct}% @t{tick})"),
            Shock::Demand { tick, pct, dur } => format!("demand(+{pct}% @t{tick} for {dur})"),
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
