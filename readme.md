# Auto Dollar Cost Averaging Bot

This Rust program automates dollar cost averaging into a target portfolio allocation on an Alpaca trading account. 

## Features

- Automatically invests a calculated amount daily to reach a target portfolio equity by a finish date 
- Invests based on minimizing the difference between current and target allocation percentages
- Does not perform fractional share trades
- Stores state in a JSON file to enable persistence across runs
- Assumes Alpaca API credentials are set in environment variables

## Usage

The program requires:

- Alpaca API key and secret set in `APCA_API_KEY_ID` and `APCA_API_SECRET_KEY` environment variables

It will automatically generate a state file that stores the configuration and should be modified by the user to meet his or her objectives.

An example `state.json`:

```json
{
  "last_funding_date": "2023-10-09T15:29:07.294605323Z",
  "reference_equities": {
    "PEP": 641.16,
    "UNH": 2624.05, 
    "DG": 207.34,
    "CVX": 3082.37,
    "AAPL": 3017.33,
    "XOM": 3536.61, 
    "GIS": 2062.17,
    "HD": 292.82,
    "ABT": 10753.68,
    "GOOGL": 6879.0
  },
  "ideal_allocations": {
    "GIS": 0.062307740418708554,
    "DG": 0.0062647050914401,
    "CVX": 0.09313272418588896,
    "XOM": 0.10685742583890216,
    "PEP": 0.01937242363474358,
    "UNH": 0.07928474676952539,
    "HD": 0.008847453192222871, 
    "GOOGL": 0.20784656276654984,
    "AAPL": 0.0911675634877735,
    "ABT": 0.3249186546142451
  },
  "target_investment_equity_ratio": 1.8,
  "finish_date": "2024-10-06T15:57:19.656310857Z" 
}
```

The `reference_equities` fields track the reference allocation exclude the program's investments. This ensures `ideal_allocations` represents only the investments made by this program.

The `target_investment_equity_ratio` controls margin trading. Values above 1 use margin to reach the target equity.

To run, execute:

```
cargo run
```

On each run, the program will:

- Calculate amount to invest daily to reach target equity by finish date 
- Compare current vs target allocations, excluding original reference equity
- Invest by purchasing stocks that most closely minimize allocation error
- Update state.json with new state

## License

This project is licensed under the MIT license. See LICENSE for details.
