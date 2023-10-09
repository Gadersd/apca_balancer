use apca::ApiInfo;
use apca::Client;

use apca::api::v2::{account, calendar, order, position, positions};
use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::US::Eastern;
use num_decimal::Num;
use std::hash::Hash;
use std::str::FromStr;
use std::time;

use std::collections::HashMap;
use std::thread::current;

use serde::{Deserialize, Serialize};
use serde_json;

fn mean(x: impl Iterator<Item = f64>) -> Option<f64> {
    let (i, sum) = x.fold((0, 0.0), |(i, sum), v| (i + 1, sum + v));

    if i > 0 {
        Some(sum / i as f64)
    } else {
        None
    }
}

fn error(
    stock_fractions: impl Iterator<Item = f64>,
    ideal_fractions: impl Iterator<Item = f64>,
) -> Option<f64> {
    mean(
        stock_fractions
            .zip(ideal_fractions)
            .map(|(v1, v2)| v1 - v2)
            .map(|v| v * v),
    )
}

fn best_asset_to_fund(
    stock_equities: impl Iterator<Item = f64> + Clone,
    stock_prices: impl Iterator<Item = f64>,
    ideal_allocations: impl Iterator<Item = f64> + Clone,
    max_fund: f64,
) -> Option<(usize, f64)> {
    let total_stock_equity: f64 = stock_equities.clone().sum();

    min_by_key_f64(
        stock_prices
            .enumerate()
            .filter(|&(_, p)| p <= max_fund)
            .filter_map(|(i, p)| {
                let stock_fractions = stock_equities
                    .clone()
                    .enumerate()
                    .map(|(se_id, se)| if se_id != i { se } else { se + p })
                    .map(|se| se / (total_stock_equity + p));
                let err = error(stock_fractions, ideal_allocations.clone())?;

                Some((i, err))
            }),
        |&(_, e)| e,
    )
}

fn min_by_key_f64<B>(x: impl Iterator<Item = B>, key: impl Fn(&B) -> f64) -> Option<B> {
    x.fold((f64::INFINITY, None), |(min, min_item), item| {
        let k = key(&item);
        if k < min {
            (k, Some(item))
        } else {
            (min, min_item)
        }
    })
    .1
}

use std::ops::ControlFlow;

fn generate_orders(
    stock_equities: impl Iterator<Item = f64>,
    stock_prices: impl Iterator<Item = f64> + Clone,
    ideal_allocations: impl Iterator<Item = f64> + Clone,
    max_fund: f64,
) -> (Vec<(usize, f64)>, Vec<f64>) {
    let stock_equities: Vec<_> = stock_equities.collect();
    let orders = Vec::new();

    let r = (0..).into_iter().try_fold(
        (orders, stock_equities, max_fund),
        |(orders, stock_equities, max_fund), _| {
            if stock_prices.clone().any(|p| p <= max_fund) {
                if let Some((idx, err)) = best_asset_to_fund(
                    stock_equities.iter().cloned(),
                    stock_prices.clone(),
                    ideal_allocations.clone(),
                    max_fund,
                ) {
                    let order_amount = stock_prices.clone().nth(idx).unwrap();

                    let mut new_orders = orders;
                    new_orders.push((idx, order_amount));

                    let mut new_stock_equities = stock_equities;
                    new_stock_equities[idx] += order_amount;

                    ControlFlow::Continue((new_orders, new_stock_equities, max_fund - order_amount))
                } else {
                    ControlFlow::Break((orders, stock_equities, max_fund))
                }
            } else {
                ControlFlow::Break((orders, stock_equities, max_fund))
            }
        },
    );

    match r {
        ControlFlow::Break((orders, stock_equities, _)) => (orders, stock_equities),
        _ => panic!("Impossible path!"),
    }
}

async fn submit_order(client: &Client, sym: &str, price: f64, funds: f64) -> Result<order::Order> {
    assert!(funds > 0.0);

    let limit_price = price * 0.999;

    let qty = (funds / limit_price) as usize;

    let request = order::OrderReqInit {
        type_: order::Type::Limit,
        limit_price: Some(Num::from_str(&format!("{:.2}", limit_price)).unwrap()),
        time_in_force: order::TimeInForce::Day,
        ..Default::default()
    }
    .init(
        sym,
        order::Side::Buy,
        order::Amount::quantity(Num::from(qty)),
    );

    Ok(client.issue::<order::Post>(&request).await?)
}

/*async fn submit_order(client: &Client, sym: &str, price: f64, funds: f64) -> Result<()> {
    println!("Order for {} with size ${}", sym, funds);

    Ok( () )
}*/

#[derive(Serialize, Deserialize)]
struct Order {}

#[derive(Serialize, Deserialize)]
struct State {
    last_funding_date: Option<DateTime<Utc>>, 
    reference_equities: HashMap<String, f64>, 
    ideal_allocations: HashMap<String, f64>,
    target_investment_equity_ratio: f64,
    finish_date: DateTime<Utc>,
}

async fn wait_until_datetime(dt: DateTime<Utc>, granularity: Duration) {
    while Utc::now() < dt {
        tokio::time::sleep(granularity.to_std().unwrap()).await;
    }
}

use anyhow::Result;
use std::fs;

fn load_state(filename: &str) -> Result<State> {
    let data = fs::read_to_string(filename)?;
    Ok(serde_json::from_str(&data)?)
}

fn save_state(filename: &str, state: &State) -> Result<()> {
    let str = serde_json::to_string(state)?;
    fs::write(filename, str)?;
    Ok(())
}

async fn generate_default_state(client: &Client) -> Result<State> {
    let pos: Vec<_> = client.issue::<positions::Get>(&()).await?;
    let stock_equities: Vec<_> = pos
        .iter()
        .map(|pos| pos.market_value.as_ref().unwrap().to_f64().unwrap())
        .collect();

    let total_invested: f64 = stock_equities.iter().cloned().sum();
    let ideal_allocs: Vec<_> = stock_equities
        .iter()
        .cloned()
        .map(|e| e / total_invested)
        .collect();
    let syms = pos.iter().map(|pos| pos.symbol.clone());

    Ok( State {
        last_funding_date: None,
        reference_equities: HashMap::from_iter(syms.clone().zip(stock_equities)),
        ideal_allocations: HashMap::from_iter(syms.zip(ideal_allocs)),
        target_investment_equity_ratio: 1.0,
        finish_date: Utc::now() + Duration::days(365),
    } )
}

use tokio;

#[tokio::main]
async fn main() -> Result<()> {
    // Assumes credentials to be present in the `APCA_API_KEY_ID` and
    // `APCA_API_SECRET_KEY` environment variables.
    let api_info = ApiInfo::from_env()?;
    let client = Client::new(api_info);

    let state_filename = "state.json";

    loop {
        let mut state = match load_state(state_filename) {
            Ok(state) => state,
            _ => {
                let state = generate_default_state(&client).await?;
                save_state(state_filename, &state)?;
                state
            }, 
        };

        let current_dt = Utc::now();

        // wait until next trading time
        let earliest_next_trading_dt = if let Some(dt) = state.last_funding_date {
            current_dt.max( dt + Duration::days(1) )
        } else {
            current_dt
        };

        {
            let earliest_next_trading_date_eastern = earliest_next_trading_dt.with_timezone(&Eastern).date_naive();
            let calendar_req = calendar::CalendarReq {
                start: earliest_next_trading_date_eastern,
                end: earliest_next_trading_date_eastern + Duration::days(7),
            };

            let open_close = client.issue::<calendar::Get>(&calendar_req).await?;
            let (next_trading_date, next_trading_time) =
                open_close.first().map(|oc| (oc.date, oc.open)).unwrap();
            let next_trading_dt = Eastern
                .from_local_datetime(
                    &next_trading_date.and_time(next_trading_time + Duration::hours(1)),
                )
                .unwrap();
            let next_trading_dt = next_trading_dt.with_timezone(&Utc);

            println!("Waiting until next trading time {:?}", next_trading_dt);
            wait_until_datetime(next_trading_dt, Duration::seconds(10)).await;
        }

        let account = client.issue::<account::Get>(&()).await?;

        let equity = account.equity.to_f64().unwrap(); println!("Account equity = {}", equity);
        let cash = account.cash.to_f64().unwrap(); println!("Account cash = {}", cash);
        let buying_power = account.buying_power.to_f64().unwrap(); println!("Account buying power = {}", buying_power);

        let total_invested = equity - cash;

        let days_until_finished = (state.finish_date - current_dt).num_days();

        let total_additional_funding =
            equity * state.target_investment_equity_ratio - total_invested;
        let daily_funding = (total_additional_funding / days_until_finished as f64).max(0.0);

        println!("Daily funding = {}", daily_funding);

        assert!(days_until_finished > 0);
        assert!(daily_funding >= 0.0);
        assert!(buying_power >= daily_funding);

        let days_since_last_funding = state
            .last_funding_date
            .map(|dt| (current_dt - dt).num_days());

        let funding_today = match days_since_last_funding {
            Some(d) => daily_funding * d as f64,
            None => daily_funding,
        };

        println!("Funding today = {}", funding_today);

        if funding_today > 0.0 {
            let pos: Vec<_> = client.issue::<positions::Get>(&()).await?;

            let virtual_equities: Vec<_> = pos
                .iter()
                .map(|pos| {
                    let e = pos.market_value.as_ref().unwrap().to_f64().unwrap();
                    let ref_e = state
                        .ideal_allocations
                        .get(&pos.symbol)
                        .cloned()
                        .unwrap_or(0.0);

                    e - ref_e

                }).collect();
            let stock_prices = pos
                .iter()
                .map(|pos| pos.current_price.as_ref().unwrap().to_f64().unwrap());

            let ideal_allocations: Vec<_> = pos
                .iter()
                .map(|pos| {
                    state
                        .ideal_allocations
                        .get(&pos.symbol)
                        .cloned()
                        .unwrap_or(0.0)
                })
                .collect();
            let ideal_allocations_itr = ideal_allocations.iter().cloned();

            let (orders, new_virtual_equities) = generate_orders(
                virtual_equities.into_iter(),
                stock_prices.clone(),
                ideal_allocations_itr,
                funding_today,
            );
            println!("Orders: {:?}", orders);
            for (idx, funding) in orders {
                let price = stock_prices.clone().nth(idx).unwrap();
                let sym = &pos[idx].symbol;

                submit_order(&client, sym, price, funding).await?;
            }

            state.last_funding_date = Some(Utc::now());
            save_state(state_filename, &state)?;
        }
    }
}
