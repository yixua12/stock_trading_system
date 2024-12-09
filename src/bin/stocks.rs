use tokio::sync::mpsc;
use rand::Rng;
use serde::{Serialize, Deserialize};
use std::sync::{Arc, Mutex};
use tokio::task;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Stock {
    id: String,
    name: String,
    price: f64,
    available: u32,
    bought: u32,
    sold: u32,
}

impl Stock {
    fn new(id: &str, name: &str, initial_price: f64, initial_available: u32) -> Self {
        Stock {
            id: id.to_string(),
            name: name.to_string(),
            price: initial_price,
            available: initial_available,
            bought: 0,
            sold: 0,
        }
    }

    // Simulate stock price fluctuation (simple random price change)
    fn fluctuate(&mut self) {
        let mut rng = rand::thread_rng();
        let fluctuation = rng.gen_range(-0.05..0.05); // Price fluctuation between -5% to +5%
        self.price += self.price * fluctuation;
        if self.price < 0.1 {
            self.price = 0.1; // Prevent price from going below $0.1
        }
    }

    // Buying stock
    fn buy(&mut self, quantity: u32) -> Result<(), String> {
        if self.available >= quantity {
            self.available -= quantity;
            self.bought += quantity;
            Ok(())
        } else {
            Err("Not enough stock available".to_string())
        }
    }

    // Selling stock
    fn sell(&mut self, quantity: u32) -> Result<(), String> {
        if self.bought >= quantity {
            self.bought -= quantity;
            self.available += quantity;
            self.sold += quantity;
            Ok(())
        } else {
            Err("Not enough stock bought to sell".to_string())
        }
    }
}

// Worker for updating stock price
async fn update_stock_price(stock: Arc<Mutex<Stock>>) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let mut stock = stock.lock().unwrap();
        stock.fluctuate();
        println!("Updated Stock: {:?}", stock);
    }
}

// Worker for managing buy/sell orders
async fn manage_orders(stock: Arc<Mutex<Stock>>, tx: mpsc::Sender<String>) {
    let orders = vec![
        ("buy", 10),
        ("sell", 5),
        ("buy", 15),
        ("sell", 5),
    ];

    for order in orders {
        let mut stock = stock.lock().unwrap();
        match order {
            ("buy", qty) => {
                if let Err(e) = stock.buy(qty) {
                    tx.send(format!("Buy Error: {}", e)).await.unwrap();
                } else {
                    tx.send(format!("Successfully bought {} stocks", qty)).await.unwrap();
                }
            },
            ("sell", qty) => {
                if let Err(e) = stock.sell(qty) {
                    tx.send(format!("Sell Error: {}", e)).await.unwrap();
                } else {
                    tx.send(format!("Successfully sold {} stocks", qty)).await.unwrap();
                }
            },
            _ => {}
        }
    }
}

// Main function to run the system
#[tokio::main]
async fn main() {
    let stock = Arc::new(Mutex::new(Stock::new("AAPL", "Apple Inc.", 150.0, 100)));

    let (tx, mut rx) = mpsc::channel::<String>(32);

    let stock_clone = Arc::clone(&stock);
    tokio::spawn(async move {
        update_stock_price(stock_clone).await;
    });

    let stock_clone = Arc::clone(&stock);
    tokio::spawn(async move {
        manage_orders(stock_clone, tx).await;
    });

    // Listen for order updates and stock status
    while let Some(message) = rx.recv().await {
        println!("{}", message);
    }
}
