use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, Duration};
use rand::{Rng, rngs::OsRng};
use rand::rngs::adapter::ReseedingRng;
use rand_chacha::ChaCha12Core;
use rand::SeedableRng;
use std::sync::Arc;
use prettytable::{Table, Row, Cell};

#[derive(Debug, Clone)]
pub struct Stock {
    pub id: String,
    pub name: String,
    pub sell_price: f64,
    pub buy_price: f64,
    pub available_stock: u32,
}

#[derive(Debug, Clone)]
pub struct StockMarket {
    pub stocks: Vec<Stock>,
    pub transactions: Vec<String>,
    pub usd_price: f64,
    pub gold_price: f64,
    pub petrol_price: f64,
}

impl StockMarket {
    pub fn new() -> Self {
        StockMarket {
            stocks: vec![],
            transactions: vec![],
            usd_price: 1.0,
            gold_price: 1800.0,
            petrol_price: 100.0,
        }
    }

    pub fn add_stock(&mut self, stock: Stock) {
        self.stocks.push(stock);
    }

    pub fn simulate_price_changes(&mut self, rng: &mut impl Rng) {
        let usd_fluctuation = rng.gen_range(-0.02..0.02);
        self.usd_price += usd_fluctuation;
        println!(
            "US Dollar has {} by {:.2}%",
            if usd_fluctuation > 0.0 { "increased" } else { "decreased" },
            usd_fluctuation * 100.0
        );

        let gold_fluctuation = rng.gen_range(-0.05..0.05);
        self.gold_price += self.gold_price * gold_fluctuation;
        println!(
            "Gold price has {} by {:.2}%",
            if gold_fluctuation > 0.0 { "increased" } else { "decreased" },
            gold_fluctuation * 100.0
        );

        let petrol_event = rng.gen_range(0..3);
        match petrol_event {
            0 => {
                let petrol_fluctuation = rng.gen_range(-0.03..0.03);
                self.petrol_price += self.petrol_price * petrol_fluctuation;
                println!(
                    "Petroleum price has {} due to news on electric cars, fluctuation: {:.2}%",
                    if petrol_fluctuation > 0.0 { "increased" } else { "decreased" },
                    petrol_fluctuation * 100.0
                );
            }
            1 => {
                let petrol_fluctuation = rng.gen_range(0.01..0.05);
                self.petrol_price += self.petrol_price * petrol_fluctuation;
                println!(
                    "Petroleum price has increased due to rising demand, fluctuation: {:.2}%",
                    petrol_fluctuation * 100.0
                );
            }
            _ => {
                let petrol_fluctuation = rng.gen_range(-0.01..-0.03);
                self.petrol_price += self.petrol_price * petrol_fluctuation;
                println!(
                    "Petroleum price has decreased due to oversupply, fluctuation: {:.2}%",
                    petrol_fluctuation * 100.0
                );
            }
        }

        for stock in &mut self.stocks {
            let fluctuation = rng.gen_range(-0.05..0.05);
            let change = stock.sell_price * fluctuation;
            stock.sell_price += change;
            stock.buy_price = stock.sell_price * 1.20;
        }
    }

    pub fn list_stocks(&self) {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Stock ID"),
            Cell::new("Name"),
            Cell::new("Sell Price"),
            Cell::new("Buy Price"),
            Cell::new("Available Stock"),
        ]));

        for stock in &self.stocks {
            table.add_row(Row::new(vec![
                Cell::new(&stock.id),
                Cell::new(&stock.name),
                Cell::new(&stock.sell_price.to_string()),
                Cell::new(&stock.buy_price.to_string()),
                Cell::new(&stock.available_stock.to_string()),
            ]));
        }

        print!("\x1b[2J\x1b[H");
        table.printstd();
    }
}

async fn simulate_stock_updates(
    stock_market: Arc<Mutex<StockMarket>>,
    stock_tx: mpsc::Sender<Option<Stock>>,
    rng: Arc<Mutex<ReseedingRng<ChaCha12Core, OsRng>>>,
) {
    let stock_ids = vec!["AAPL".to_string(), "GOOGL".to_string(), "AMZN".to_string()];

    loop {
        for stock_id in &stock_ids {
            let mut rng_locked = rng.lock().await;
            let rng_ref = &mut *rng_locked;

            let stock = Stock {
                id: stock_id.clone(),
                name: stock_id.clone(),
                sell_price: rng_ref.gen_range(100.0..500.0),
                buy_price: rng_ref.gen_range(100.0..500.0) * 1.20,
                available_stock: rng_ref.gen_range(50..200),
            };

            // Clone the stock before sending to avoid moving it
            stock_tx.send(Some(stock.clone())).await.unwrap(); // Clone here
            println!("New stock update: {:?}", stock);
        }
        time::sleep(Duration::from_secs(5)).await;
    }
}

pub async fn handle_stock_updates(
    mut stock_rx: mpsc::Receiver<Option<Stock>>,  // Receiver expecting Option<Stock>
    stock_market: Arc<Mutex<StockMarket>>,
    rng: Arc<Mutex<ReseedingRng<ChaCha12Core, OsRng>>>,
) {
    loop {
        match stock_rx.recv().await {
            // Correctly handle the Option<Stock>
            Some(Some(stock)) => {
                // Stock exists, we can process it
                let stock_clone = stock.clone();
                
                // Process the cloned stock
                let mut stock_market_locked = stock_market.lock().await;
                stock_market_locked.add_stock(stock_clone);  // Use the clone here
                stock_market_locked.simulate_price_changes(&mut *rng.lock().await);
                stock_market_locked.list_stocks();
            }
            Some(None) => {
                // Handle the case where `None` was sent, i.e., the channel is closed.
                println!("Received None, exiting stock update handling.");
                break;
            }
            None => {
                // If receiving from channel fails
                println!("Error receiving stock update.");
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let stock_market = Arc::new(Mutex::new(StockMarket::new()));
    let rng = Arc::new(Mutex::new(ReseedingRng::new(
        ChaCha12Core::from_entropy(),
        0,
        OsRng,
    )));

    let (stock_tx, stock_rx) = mpsc::channel::<Option<Stock>>(32); // Correct channel type

    tokio::spawn({
        let stock_market = stock_market.clone();
        let rng = rng.clone();
        let stock_tx = stock_tx.clone();
        async move {
            simulate_stock_updates(stock_market, stock_tx, rng).await;
        }
    });

    tokio::spawn({
        let stock_market = stock_market.clone();
        let rng = rng.clone();
        async move {
            handle_stock_updates(stock_rx, stock_market, rng).await;
        }
    });

    loop {
        time::sleep(Duration::from_secs(10)).await;
    }
}
