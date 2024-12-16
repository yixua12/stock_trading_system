use futures::{StreamExt, TryStreamExt};
use lapin::{
    options::*, types::FieldTable, BasicProperties, Channel, Connection, ConnectionProperties,
};
use prettytable::{Cell, Row, Table};
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{self, Duration};

// Structs for Stock and StockTransaction
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub silver_price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockTransaction {
    pub action: String, // "buy" or "sell"
    pub id: String,
    pub name: String,
    pub sell_price: f64, // the price at which the stock is being sold
    pub buy_price: f64,  // the price at which the stock is being bought
    pub quantity: u32,  
}

impl StockMarket {
    // Generate a table representation of the stock list as a string
    pub fn generate_stock_table(&self) -> String {
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

        let mut table_string = Vec::new();
        table
            .print(&mut table_string)
            .expect("Failed to generate table");
        String::from_utf8(table_string).expect("Failed to convert table to String")
    }

    // Publish the stock table to RabbitMQ
    pub async fn publish_stock_table(
        &self,
        rabbitmq_channel: Arc<Mutex<Channel>>,
        exchange: &str,
        routing_key: &str,
        properties: &BasicProperties,
    ) {
        let table_string = self.generate_stock_table();
        let payload = table_string.into_bytes();

        let mut channel_locked = rabbitmq_channel.lock().await;

        // Publish the table
        if let Err(e) = channel_locked
            .basic_publish(
                exchange,
                routing_key,
                BasicPublishOptions::default(),
                payload,
                properties.clone(),
            )
            .await
        {
            eprintln!("Failed to publish stock table: {:?}", e);
        } else {
            println!("Published stock table.");
        }
    }

    // Simulate price changes and periodically publish the stock list
    pub async fn simulate_price_changes(
        &mut self,
        rng: &mut impl Rng,
        rabbitmq_channel: Arc<Mutex<Channel>>,
        exchange: &str,
        routing_key: &str,
        properties: &BasicProperties,
    ) {
        loop {
            // Generate and print the stock table locally
            // Simulate price fluctuations
            println!("\n--------Latest Stock ---------:\n");
            for stock in &mut self.stocks {
                let price_fluctuation = rng.gen_range(-0.05_f64..0.05_f64);
                stock.sell_price += stock.sell_price * price_fluctuation;
                stock.buy_price = stock.sell_price * 1.20;

                println!(
                    "{}: Updated price to {:.2}, available stock: {}",
                    stock.name, stock.sell_price, stock.available_stock
                );
            }
            let table_string = self.generate_stock_table();
            println!("\nUpdated Stock Table:\n{}", table_string);

            // Publish the updated stock list to RabbitMQ
            self.publish_stock_table(rabbitmq_channel.clone(), exchange, routing_key, properties)
                .await;

            time::sleep(Duration::from_secs(5)).await;
        }
    }

    // Function to publish stock updates to RabbitMQ
    pub async fn publish_stock_updates(
        &self,
        rabbitmq_channel: Arc<Mutex<Channel>>,
        exchange: &str,
        routing_key: &str,
        properties: &BasicProperties,
    ) {
        let mut channel_locked = rabbitmq_channel.lock().await;

        for stock in &self.stocks {
            let stock_json = match serde_json::to_string(stock) {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("Failed to serialize stock details: {}", e);
                    continue;
                }
            };

            let payload = stock_json.into_bytes();

            if let Err(e) = channel_locked
                .basic_publish(
                    exchange,
                    routing_key,
                    BasicPublishOptions::default(),
                    payload,
                    properties.clone(),
                )
                .await
            {
                eprintln!("Failed to publish stock update: {:?}", e);
            } else {
                println!("Published stock update: {}", stock.name);
            }
        }
    }

    pub async fn consume_actions(
        &mut self,
        rabbitmq_channel: Arc<Mutex<Channel>>,
        response_exchange: &str,
        response_routing_key: &str,
    ) {
        let mut channel_locked = rabbitmq_channel.lock().await;

        let consumer = channel_locked
            .basic_consume(
                "broker_action_queue",
                "stockmarket_consumer_tag",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .expect("Failed to start consuming actions");

        let mut consumer_stream = consumer.into_stream();

        while let Some(delivery) = consumer_stream.next().await {
            match delivery {
                Ok(delivery) => {
                    let action_json = String::from_utf8_lossy(&delivery.1.data);
                    match serde_json::from_str::<StockTransaction>(&action_json) {
                        Ok(action) => {
                            println!("StockMarket received action: {:?}", action);

                            // Process the action
                            let response = self.process_transaction(action);

                            // Send response back to broker
                            self.send_response(
                                rabbitmq_channel.clone(),
                                response_exchange,
                                response_routing_key,
                                response,
                            )
                            .await;
                        }
                        Err(e) => eprintln!("Failed to deserialize action: {}", e),
                    }
                }
                Err(e) => eprintln!("Error receiving action: {}", e),
            }
        }
    }

    fn process_transaction(&mut self, transaction: StockTransaction) -> String {
        if let Some(stock) = self.stocks.iter_mut().find(|s| s.id == transaction.id) {
            match transaction.action.as_str() {
                "buy" => {
                    if stock.available_stock >= transaction.quantity {
                        stock.available_stock -= transaction.quantity;
                        return format!(
                            "Buy successful: {} {} remaining: {}",
                            transaction.quantity, stock.name, stock.available_stock
                        );
                    } else {
                        return format!(
                            "Buy failed: Insufficient stock for {} (Available: {})",
                            stock.name, stock.available_stock
                        );
                    }
                }
                "sell" => {
                    stock.available_stock += transaction.quantity;
                    return format!(
                        "Sell successful: {} {} new total: {}",
                        transaction.quantity, stock.name, stock.available_stock
                    );
                }
                _ => return "Invalid action".to_string(),
            }
        } else {
            return format!("Stock with ID {} not found", transaction.id);
        }
    }

    async fn send_response(
        &self,
        rabbitmq_channel: Arc<Mutex<Channel>>,
        exchange: &str,
        routing_key: &str,
        response: String,
    ) {
        let mut channel_locked = rabbitmq_channel.lock().await;
        let response_clone = response.clone();

        if let Err(e) = channel_locked
            .basic_publish(
                exchange,
                routing_key,
                BasicPublishOptions::default(),
                response_clone.into_bytes(),
                BasicProperties::default(),
            )
            .await
        {
            eprintln!("Failed to send response: {:?}", e);
        } else {
            println!("Response sent: {}", response);
        }
    }
}

#[tokio::main]
async fn main() {
    let addr = std::env::var("AMQP_ADDR").unwrap_or_else(|_| "amqp://127.0.0.1:5672/%2f".into());
    let conn = Connection::connect(&addr, ConnectionProperties::default())
        .await
        .expect("Connection to RabbitMQ failed");

    let channel = conn
        .create_channel()
        .await
        .expect("Channel creation failed");

    // Declare exchange and queues
    channel
        .exchange_declare(
            "stocks_exchange",
            lapin::ExchangeKind::Direct,
            ExchangeDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare exchange");

    channel
        .queue_declare(
            "broker_stock_queue",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare broker_stock_queue");

    channel
        .queue_declare(
            "broker_action_queue",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare broker_action_queue");

    channel
        .queue_declare(
            "broker_response_queue",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to declare broker_response_queue");

    channel
        .queue_bind(
            "broker_stock_queue",
            "stocks_exchange",
            "stock_routing_key",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .expect("Failed to bind broker_stock_queue");

    let rabbitmq_channel = Arc::new(Mutex::new(channel));
    let stock_market = Arc::new(Mutex::new(StockMarket {
        stocks: vec![
            // Initialize stocks with random prices and fixed available stock
            Stock {
                id: "G1".to_string(),
                name: "Gold".to_string(),
                sell_price: rand::thread_rng().gen_range(1700.0..2000.0),
                buy_price: rand::thread_rng().gen_range(2040.0..2400.0),
                available_stock: rand::thread_rng().gen_range(50..150),
            },
            Stock {
                id: "S1".to_string(),
                name: "Silver".to_string(),
                sell_price: rand::thread_rng().gen_range(20.0..30.0),
                buy_price: rand::thread_rng().gen_range(24.0..36.0),
                available_stock: rand::thread_rng().gen_range(400..600),
            },
            Stock {
                id: "P1".to_string(),
                name: "Petrol".to_string(),
                sell_price: rand::thread_rng().gen_range(2.5..3.5),
                buy_price: rand::thread_rng().gen_range(3.0..4.0),
                available_stock: rand::thread_rng().gen_range(250..350),
            },
        ],
        transactions: vec![],
        usd_price: 1.0,
        gold_price: 1800.0,
        petrol_price: 3.0,
        silver_price: 25.0,
    }));

    // Task: Simulate stock price changes
    tokio::spawn({
        let stock_market_clone = stock_market.clone();
        let rabbitmq_channel_clone = rabbitmq_channel.clone();
        async move {
            let mut stock_market = stock_market_clone.lock().await;
            stock_market
                .simulate_price_changes(
                    &mut OsRng,
                    rabbitmq_channel_clone,
                    "stocks_exchange",
                    "stock_routing_key",
                    &BasicProperties::default(),
                )
                .await;
        }
    });

    // Task: Consume broker actions (buy/sell requests)
    tokio::spawn({
        let stock_market_clone = stock_market.clone();
        let rabbitmq_channel_clone = rabbitmq_channel.clone();
        async move {
            let mut stock_market = stock_market_clone.lock().await;
            stock_market
                .consume_actions(
                    rabbitmq_channel_clone,
                    "stocks_exchange",
                    "broker_response_routing_key",
                )
                .await;
        }
    });

    // Prevent the main function from exiting
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl+c");
}