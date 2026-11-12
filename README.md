# econtoy

A tiny agent-based economy simulator you run in the terminal to watch emergent
macro behavior (inflation, unemployment, credit-driven booms and busts) as you
turn knobs like the interest rate, money-supply growth, taxes, and government
spending, or replay a whole policy story from a scenario file.

econtoy is an educational emergence toy, not a real macroeconomic model. It
deliberately favors a small set of clear, legible rules over realism. Do not use
it for forecasting, policy analysis, or anything that matters. Its purpose is to
make a handful of textbook feedback loops visible and pokeable from the command
line.

- Zero external dependencies. The RNG, the charts, and the JSON/CSV writers are
  all implemented from scratch.
- Fully deterministic: the same seed and parameters always produce byte-for-byte
  identical output.
- Pure simulation logic lives in the library (`Simulation`), so you can embed it
  or test it directly; the binary only parses flags and renders.

## Install

Build from source with a stable Rust toolchain (https://rustup.rs):

```bash
git clone https://github.com/printemps-tokyo/econtoy
cd econtoy
cargo build --release
./target/release/econtoy --help
```

Prebuilt binaries are attached to tagged releases (as with the other
printemps-tokyo tools), so you can also grab one from the Releases page instead
of building.

## Quickstart

```bash
# Default 200-tick run with a human-readable report and charts
econtoy

# Tighten policy: higher interest rate cools inflation
econtoy --interest-rate 12 --money-growth 6 --ticks 300

# Blow a bubble: inject 35% extra money at tick 120
econtoy --interest-rate 3 --money-growth 3 --shock money:120:35

# Add a government: tax income and profit, buy a share of output
econtoy --income-tax 20 --profit-tax 20 --gov-spending 22

# Change policy mid-run: raise the policy rate to 15% at tick 100
econtoy --money-growth 4 --interest-rate 1 --shock set:100:interest-rate:15

# Replay a built-in scenario, or one of your own
econtoy --list-scenarios
econtoy --scenario stimulus
econtoy --scenario my-economy.scenario --ticks 400

# Machine-readable output for scripts and plots
econtoy --json > run.json
econtoy --csv  > run.csv
```

## The model

The economy has three kinds of agent, plus an optional government.

- Households supply labor, earn wages, receive firm dividends, and hold
  money as bank deposits. Each tick they try to consume a fraction (the marginal
  propensity to consume) of the money they hold.
- Firms hire workers, produce goods, hold inventory, and set a price that
  rises when buyers want more than is on the shelf and falls when inventory
  piles up. They plan output toward what they expect to sell, run lean on cash,
  borrow to cover the wage bill, and pay out profit as dividends.
- A bank takes deposits and extends loans at the policy interest rate. Loans
  create deposits (simple endogenous money), so the broad money stock expands
  and contracts with credit, capped by a reserve ratio (money multiplier
  `1 / reserve_ratio`). Interest paid by borrowers accrues to bank equity and
  leaves circulation, which is the toy's monetary-transmission channel.
- The government (active when any tax or spending setting is above zero)
  withholds a flat income tax from wages and dividends, takes a flat tax on
  firm profit, and buys goods alongside households. Its purchases are a share
  of full-employment wage income, indexed to last tick's price level, so the
  real program stays the same as prices move. The treasury keeps an account
  at the bank: a deficit is paid from an overdraft that stands in for selling
  bonds, the overdraft is the public debt, and it pays the policy rate on it.
  The overdraft is not counted against the bank's lending cap, so government
  borrowing neither crowds out nor adds to private credit. When spending runs
  ahead of what the taxes can raise, the deficit is paid with new deposits and
  prices rise.

### The knobs

| Flag | Meaning | Default | Range |
| --- | --- | --- | --- |
| `--ticks N` | Number of periods to simulate (one tick = one month) | 200 | >= 1 |
| `--seed N` | Seed for the deterministic RNG | 42 | any u64 |
| `--households N` | Number of household agents | 200 | >= 1 |
| `--firms N` | Number of firm agents | 20 | >= 1 |
| `--interest-rate PCT` | Annualized policy interest rate, percent | 5 | 0..=100 |
| `--money-growth PCT` | Annualized growth of base money, percent | 2 | >= -100 |
| `--mpc F` | Marginal propensity to consume out of money on hand | 0.3 | 0..=1 |
| `--reserve-ratio F` | Bank reserve ratio; multiplier is `1/ratio` | 0.1 | 0.001..=1 |
| `--income-tax PCT` | Tax on household wages and dividends, percent | 0 | 0..=100 |
| `--profit-tax PCT` | Tax on firm profit before dividends, percent | 0 | 0..=100 |
| `--gov-spending PCT` | Government purchases, percent of full-employment wage income | 0 | 0..=100 |
| `--shock SPEC` | Add a shock or policy change (repeatable, see below) | none | |
| `--scenario NAME\|FILE` | Load a scenario file or a built-in scenario | none | |
| `--list-scenarios` | List the built-in scenarios and exit | | |

Annualized rates are converted to per-tick rates by dividing by 12 (twelve ticks
per year). Money growth is applied as a helicopter drop split equally across
households each tick.

### Shocks

`--shock` may be passed more than once. Three kinds are supported:

- `money:TICK:PCT` injects `PCT`% of the current base money into households at
  tick `TICK` (a one-off money-supply shock). Good for demonstrating a bubble
  and its aftermath.
- `demand:TICK:PCT:DUR` raises the propensity to consume by `PCT`% for `DUR`
  ticks starting at `TICK` (a temporary confidence / demand shock). A
  negative `PCT` is a slump.
- `set:TICK:KNOB:VALUE` sets a policy knob to `VALUE` from tick `TICK` on.
  The knobs are `interest-rate`, `money-growth`, `income-tax`, `profit-tax`,
  and `gov-spending`, with the same ranges as their flags.

```bash
econtoy --shock money:120:35
econtoy --shock demand:80:60:20 --shock money:150:20
econtoy --gov-spending 22 --income-tax 20 \
  --shock demand:60:-50:36 --shock set:66:gov-spending:40 --shock set:102:gov-spending:22
```

### Scenario files

A scenario file bundles settings, shocks, and policy changes into one plain
text file, so a policy story can be saved, shared, and replayed:

```
# Households cut spending by half for three years; the government steps in.
name = Fiscal stimulus in a slump
description = A three-year demand slump met by a temporary rise in government purchases.
ticks = 200
income-tax = 20
profit-tax = 20
gov-spending = 22
shock = demand:60:-50:36
at 66: gov-spending = 40
at 102: gov-spending = 22
```

- `#` starts a comment (so `name` and `description` cannot contain `#`), and
  blank lines are ignored.
- `KEY = VALUE` takes any flag name without the leading `--` (`ticks`,
  `seed`, `households`, `firms`, `interest-rate`, `money-growth`, `mpc`,
  `reserve-ratio`, `income-tax`, `profit-tax`, `gov-spending`, `shock`),
  plus `name` and `description`. `_` and `-` are interchangeable in keys.
  `shock` may repeat; any other key may appear once.
- `at TICK: KNOB = VALUE` schedules a policy change, the same as
  `--shock set:TICK:KNOB:VALUE`.
- Errors name the line: `my.scenario: line 7: 'ticks' is set more than once`.

`--scenario` reads a file when the argument contains `/` or `.` or names an
existing file, and a built-in scenario otherwise. Flags given after
`--scenario` override the file's settings; shocks from both are kept.

Built-in scenarios (the files are in `scenarios/`):

| Name | Story |
| --- | --- |
| `balanced` | 20% taxes and spending of about the same size. Taxes scale with nominal income, so the budget settles near balance by itself. |
| `stimulus` | A three-year demand slump met by a temporary rise in government purchases. Unemployment stays lower than in the slump alone, at the cost of debt and a burst of inflation. |
| `austerity` | Ten years of deficits paid with new money, then higher taxes and lower spending at tick 120: debt and prices fall, unemployment rises. |
| `rate-hike` | 4% money growth at a 1% rate, then a hike to 15% at tick 100 pulls inflation from about 3% a year to between 1% and 2%. |
| `deficit-money` | Spending well beyond what a 10% tax can raise: the deficit drives inflation and compounding debt. |

### Output modes

- default: a human report (final indicators plus block sparklines that respect
  the terminal width, taken from `COLUMNS`). With a government it adds tax
  revenue, spending, the deficit, public debt, debt as a percent of annual
  nominal output, and deficit and debt charts.
- `--json`: the full time series and final summary as JSON.
- `--csv`: one row per tick with every tracked series.
- `--quiet`: only the final summary line (with deficit, debt, and debt/GDP
  when there is a government).

## Examples

A default run. The interest drain slightly outpaces 2% money growth early on,
then mild inflation sets in:

```
econtoy - mini economy simulation (educational toy, not a real model)
params: ticks=200 seed=42 households=200 firms=20 rate=5% money-growth=2% mpc=0.3 reserve=0.1

final indicators
  CPI (base 100)          145.07
  avg inflation            2.46%/yr
  real output (GDP)       200.00
  unemployment             0.00%
  avg household money       3.84
  credit outstanding      200.00
  broad money             768.56
  base money              935.43

time series (t0 -> t199)
CPI   ▁▂▃▄▅▅▅▆▆▆▆▆▆▆▆▆▆▆▆▆▆▆▆▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇▇█████████████  [94.72 .. 145.07]
GDP   █▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  [200.00 .. 272.39]
Unemp █▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  [0.00 .. 0.00]
Loans ▁█▆▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  [199.56 .. 215.58]
```

Raising the interest rate cools the price level. Holding money growth fixed at
4%/yr and sweeping the rate produces a clean, monotone response:

```
r= 1: CPI=216.1   r= 3: CPI=206.6   r= 6: CPI=192.3   r=10: CPI=173.1   r=15: CPI=149.2
```

A money shock at tick 120 spikes inflation, which then partially reverts as the
extra money is drained back out by interest and higher prices erode real
balances (`econtoy --shock money:120:35 --interest-rate 3 --money-growth 3`).

## JSON schema

```json
{
  "scenario": "Fiscal stimulus in a slump",
  "params": {
    "ticks": 200, "seed": 42, "households": 200, "firms": 20,
    "interest_rate": 5, "money_growth": 2, "mpc": 0.3, "reserve_ratio": 0.1,
    "income_tax": 20, "profit_tax": 20, "gov_spending": 22,
    "shocks": ["demand:60:-50:36", "set:66:gov-spending:40", "set:102:gov-spending:22"]
  },
  "series": {
    "cpi":          [/* one value per tick */],
    "gdp":          [/* real output (goods sold) per tick */],
    "unemployment": [/* rate in [0,1] per tick */],
    "inflation":    [/* period-over-period CPI change, percent */],
    "avg_money":    [/* mean household deposits per tick */],
    "credit":       [/* loans outstanding (within-tick peak) per tick */],
    "broad_money":  [/* household and firm deposits per tick */],
    "nominal_gdp":  [/* value of goods sold per tick */],
    "taxes":        [/* income plus profit tax collected per tick */],
    "gov_spending": [/* government purchases paid per tick */],
    "deficit":      [/* spending + debt interest - taxes, per tick */],
    "gov_debt":     [/* public debt at the end of each tick */]
  },
  "summary": {
    "cpi": 119.59, "avg_inflation": 1.46, "gdp": 200.66, "unemployment": 0.0,
    "avg_money": 2.29, "credit": 200.0, "broad_money": 458.4, "base_money": 935.43,
    "taxes": 53.75, "gov_spending": 52.65, "deficit": -1.09, "gov_debt": -201.19,
    "debt_to_gdp": -7.01
  }
}
```

`scenario` is present only when the run used a scenario file with a `name`.
`params` holds the starting settings; policy changes are listed in `shocks`.
`avg_inflation` is the mean per-tick inflation annualized (multiplied by 12).
`gov_debt` is negative when the government has saved more than it borrowed,
and `debt_to_gdp` divides it by 12 times the final tick's nominal output.
All arrays have length `ticks`.

## CSV schema

One header row and one row per tick:

```
tick,cpi,gdp,unemployment,inflation,avg_money,credit,broad_money,nominal_gdp,taxes,gov_spending,deficit,gov_debt
0,96.57,262.6,0.002182,0,3.354,199.6,670.7,261.3,0,0,0,0
1,94.72,272.4,0,-1.917,3.355,200,671,261.8,0,0,0,0
...
```

(Values rounded here; the real output prints full precision.)

## How the model works

Each tick runs the following ordered sequence:

0. Policy changes. Any `set:` change scheduled for this tick takes effect.
1. Money growth. New base money (`money_growth`/12 percent of the current
   base) is dropped equally on households.
2. Shocks. Any money shock scheduled for this tick is injected; any active
   demand shock scales the propensity to consume.
3. Output and hiring plan. Each firm targets output based on expected sales
   and a profitability signal (its price versus unit cost, where unit cost
   includes the interest on the wage advance). Labor demand is rationed against
   the fixed labor force.
4. Wage financing and payment. Firms cover the wage bill from cash, borrowing
   the shortfall from the bank subject to the reserve cap. Loans create deposits.
   Production is added to inventory and wages are paid to households, less
   the income tax.
5. Goods market and price adjustment. Households spend `mpc` of their money,
   and the government places its order.
   Demand is allocated across firms in proportion to inventory; each firm sells
   what it can, then moves its price up if buyers wanted more than it had and
   down if inventory was left over.
6. Interest accrual. Borrowers pay interest on their loans to the bank as
   equity, permanently removing that money from circulation. The treasury pays
   the same rate on its overdraft.
7. Repayment and dividends. Firms repay loans from revenue, pay the profit tax,
   and distribute the rest to households as dividends (less the income tax),
   so they run lean and borrow again next tick.
8. Bookkeeping. All tracked series are recorded.

### Tracked series

- CPI (price index): the mean firm price, indexed to 100 at t0.
- Real output (GDP): total goods sold this tick.
- Unemployment: `1 - employment / labor force`, in `[0, 1]`.
- Average household money: mean deposits held by households.
- Credit outstanding: loans outstanding at their within-tick peak.
- Broad money: total deposits held by households and firms.
- Inflation: period-over-period percentage change in CPI.
- Nominal output: the value of goods sold, to households and the government.
- Taxes, government spending, deficit (spending plus debt interest minus
  taxes), and public debt (the running sum of deficits).

### Accounting identity

Money is tracked as deposits (an inside/broad quantity that expands with credit)
plus a conserved base-money quantity. The following identity holds every tick, to
floating-point precision, regardless of growth or shocks:

```
base_money == total_deposits + treasury_balance + bank_equity - total_loans
```

`treasury_balance` is minus the public debt, and zero without a government.
When `--money-growth 0` is set and there are no shocks, `base_money` itself is
constant for the entire run: nothing creates or destroys outside money, and every
other operation (wages, consumption, lending, repayment, interest, dividends) is
a transfer that preserves the identity; so are taxes and government purchases.
Both facts are asserted by the test
suite on every tick.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo build --release
```

See `CONTRIBUTING.md` for details.

## License

MIT. See `LICENSE`.
