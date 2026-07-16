# econtoy

A tiny agent-based economy simulator you run in the terminal to watch emergent
macro behavior (inflation, unemployment, credit-driven booms and busts) as you
turn knobs like the interest rate and money-supply growth.

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

# Machine-readable output for scripts and plots
econtoy --json > run.json
econtoy --csv  > run.csv
```

## The model

The economy has three kinds of agent.

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
| `--shock SPEC` | Add a shock (repeatable, see below) | none | |

Annualized rates are converted to per-tick rates by dividing by 12 (twelve ticks
per year). Money growth is applied as a helicopter drop split equally across
households each tick.

### Shocks

`--shock` may be passed more than once. Two kinds are supported:

- `money:TICK:PCT` injects `PCT`% of the current base money into households at
  tick `TICK` (a one-off money-supply shock). Good for demonstrating a bubble
  and its aftermath.
- `demand:TICK:PCT:DUR` raises the propensity to consume by `PCT`% for `DUR`
  ticks starting at `TICK` (a temporary confidence / demand shock).

```bash
econtoy --shock money:120:35
econtoy --shock demand:80:60:20 --shock money:150:20
```

### Output modes

- default: a human report (final indicators plus block sparklines that respect
  the terminal width, taken from `COLUMNS`).
- `--json`: the full time series and final summary as JSON.
- `--csv`: one row per tick with every tracked series.
- `--quiet`: only the final summary line.

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
  "params": {
    "ticks": 200, "seed": 42, "households": 200, "firms": 20,
    "interest_rate": 5, "money_growth": 2, "mpc": 0.3, "reserve_ratio": 0.1
  },
  "series": {
    "cpi":          [/* one value per tick */],
    "gdp":          [/* real output (goods sold) per tick */],
    "unemployment": [/* rate in [0,1] per tick */],
    "inflation":    [/* period-over-period CPI change, percent */],
    "avg_money":    [/* mean household deposits per tick */],
    "credit":       [/* loans outstanding (within-tick peak) per tick */],
    "broad_money":  [/* total deposits per tick */]
  },
  "summary": {
    "cpi": 145.07, "avg_inflation": 2.46, "gdp": 200.0, "unemployment": 0.0,
    "avg_money": 3.84, "credit": 200.0, "broad_money": 768.56, "base_money": 935.43
  }
}
```

`avg_inflation` is the mean per-tick inflation annualized (multiplied by 12).
All arrays have length `ticks`.

## CSV schema

One header row and one row per tick:

```
tick,cpi,gdp,unemployment,inflation,avg_money,credit,broad_money
0,96.57,262.62,0.0022,0,3.35,199.56,670.74
1,94.72,272.39,0,-1.92,3.36,200,671.02
...
```

## How the model works

Each tick runs the following ordered sequence:

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
   Production is added to inventory and wages are paid to households.
5. Goods market and price adjustment. Households spend `mpc` of their money.
   Demand is allocated across firms in proportion to inventory; each firm sells
   what it can, then moves its price up if buyers wanted more than it had and
   down if inventory was left over.
6. Interest accrual. Borrowers pay interest on their loans to the bank as
   equity, permanently removing that money from circulation.
7. Repayment and dividends. Firms repay loans from revenue and distribute any
   remaining profit to households as dividends, so they run lean and borrow again
   next tick.
8. Bookkeeping. All tracked series are recorded.

### Tracked series

- CPI (price index): the mean firm price, indexed to 100 at t0.
- Real output (GDP): total goods sold this tick.
- Unemployment: `1 - employment / labor force`, in `[0, 1]`.
- Average household money: mean deposits held by households.
- Credit outstanding: loans outstanding at their within-tick peak.
- Broad money: total deposits held by households and firms.
- Inflation: period-over-period percentage change in CPI.

### Accounting identity

Money is tracked as deposits (an inside/broad quantity that expands with credit)
plus a conserved base-money quantity. The following identity holds every tick, to
floating-point precision, regardless of growth or shocks:

```
base_money == total_deposits + bank_equity - total_loans
```

When `--money-growth 0` is set and there are no shocks, `base_money` itself is
constant for the entire run: nothing creates or destroys outside money, and every
other operation (wages, consumption, lending, repayment, interest, dividends) is
a transfer that preserves the identity. Both facts are asserted by the test
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
