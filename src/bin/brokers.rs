
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TradePreferences {
    stock_id: String,
    max_price: f64,
    min_price: f64,
    order_amount: u32,
    target_profit: f64,
    stop_loss_limit: f64,
    interested_stocks: Vec<String>,
}

#[derive(Debug, Clone)]
struct Broker {
    id: String,
    preferences: TradePreferences,
}

impl Broker {
    fn new(id: &str, preferences: TradePreferences) -> Self {
        Broker {
            id: id.to_string(),
            preferences,
        }
    }

    async fn process_stock_update(&self, stock: &Stock, tx: mpsc::Sender<String>) {
        if self.preferences.interested_stocks.contains(&stock.id) {
            // identify whether the stock is interested or not
            if stock.price <= self.preferences.max_price && stock.price >= self.preferences.min_price {
                tx.send(format!(
                    "Broker {}: Placing order for stock {} at price {:.2}, order amount: {}",
                    self.id, stock.id, stock.price, self.preferences.order_amount
                ))
                .await
                .unwrap();
            } else {
                tx.send(format!(
                    "Broker {}: No action for stock {} at price {:.2}",
                    self.id, stock.id, stock.price
                ))
                .await
                .unwrap();
            }

            // handle target profit and cut loss limit
            if stock.price >= self.preferences.target_profit {
                tx.send(format!(
                    "Broker {}: Reached target profit for stock {} at price {:.2}, selling",
                    self.id, stock.id, stock.price
                ))
                .await
                .unwrap();
            } else if stock.price <= self.preferences.stop_loss_limit {
                tx.send(format!(
                    "Broker {}: Reached stop loss limit for stock {} at price {:.2}, selling",
                    self.id, stock.id, stock.price
                ))
                .await
                .unwrap();
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Stock {
    id: String,
    price: f64,
}

async fn stock_price_receiver(mut rx: mpsc::Receiver<Stock>, brokers: Vec<Arc<Broker>>, tx: mpsc::Sender<String>) {
    while let Some(stock) = rx.recv().await {
        for broker in &brokers {
            let broker_clone = broker.clone();
            let tx_clone = tx.clone();
            let stock_clone = stock.clone(); // Clone the stock for the async task
            tokio::spawn(async move {
                broker_clone.process_stock_update(&stock_clone, tx_clone).await;
            });
        }
    }
}

async fn simulate_stock_updates(tx: mpsc::Sender<Stock>, stock_ids: Vec<String>) {
    let mut rng = ChaCha8Rng::from_entropy(); // Thread-safe RNG
    loop {
        for stock_id in &stock_ids {
            let price = rng.gen_range(10.0..100.0);
            let stock = Stock {
                id: stock_id.clone(),
                price,
            };
            tx.send(stock).await.unwrap();
        }
        time::sleep(Duration::from_secs(5)).await;
    }
}

#[tokio::main]
async fn main() {
    let stock_ids = vec!["AAPL".to_string(), "GOOGL".to_string(), "AMZN".to_string()];

    let (stock_tx, stock_rx) = mpsc::channel(32);
    let (log_tx, mut log_rx) = mpsc::channel(32);

    let brokers = vec![
        Arc::new(Broker::new(
            "B1",
            TradePreferences {
                stock_id: "AAPL".to_string(),
                max_price: 50.0,
                min_price: 20.0,
                order_amount: 10,
                target_profit: 80.0,
                stop_loss_limit: 15.0,
                interested_stocks: vec!["AAPL".to_string(), "GOOGL".to_string()],
            },
        )),
        Arc::new(Broker::new(
            "B2",
            TradePreferences {
                stock_id: "GOOGL".to_string(),
                max_price: 70.0,
                min_price: 30.0,
                order_amount: 15,
                target_profit: 100.0,
                stop_loss_limit: 25.0,
                interested_stocks: vec!["GOOGL".to_string()],
            },
        )),
    ];

    let brokers_clone = brokers.clone();
    tokio::spawn(async move {
        stock_price_receiver(stock_rx, brokers_clone, log_tx).await;
    });

    tokio::spawn(async move {
        simulate_stock_updates(stock_tx, stock_ids).await;
    });

    while let Some(message) = log_rx.recv().await {
        println!("{}", message);
    }
}
