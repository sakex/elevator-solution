//! The driver controls when and where passengers arrive.

use crate::building::{Building, BuildingEvent, DriverCommand};
use rand::Rng;
use tokio::sync::{broadcast, mpsc};

/// Create a new building to be driven by this driver.
pub fn make_building() -> Building {
    // Set num elevators to more than 1 for question 2.
    Building::new(30, 10)
}

/// Simulate people arriving at the ground floor and going to the first floor, one by one.
// ----------- Solution 1 -----------
pub async fn driver(
    num_floors: usize,
    passengers_count: usize,
    mut events_rx: broadcast::Receiver<BuildingEvent>,
    driver_cmd_tx: mpsc::Sender<DriverCommand>,
) {
    let sender = driver_cmd_tx.clone();
    tokio::spawn(async move {
        let mut idx = 0;
        while idx < passengers_count {
            let (at, destination, wait_time_ms, high_traffic) = {
                let mut rng = rand::thread_rng();
                let high_traffic = rng.gen_range(0..100) >= 95; // 5% chance of high traffic
                let at = rng.gen_range(0..num_floors);
                let destination = rng.gen_range(0..num_floors);
                let wait_time_ms = rng.gen_range(1..=300);
                (at, destination, wait_time_ms, high_traffic)
            };
            tokio::time::sleep(tokio::time::Duration::from_millis(wait_time_ms)).await;
            // ----------- End solution 1 -----------
            // A passenger has arrived..
            let send_amount = if high_traffic {
                10.min(passengers_count - idx)
            } else {
                1
            };
            for _ in 0..send_amount {
                idx += 1;
                sender
                    .send(DriverCommand::PassengerArrived { at, destination })
                    .await
                    .unwrap();
            }
        }
    });
    // Wait until they are delivered..
    let mut delivered_count = 0;
    while let Ok(evt) = events_rx.recv().await {
        if let BuildingEvent::PassengerDelivered(_) = evt {
            delivered_count += 1;
            if delivered_count == passengers_count {
                break;
            }
        }
    }
    driver_cmd_tx.send(DriverCommand::Halt).await.unwrap();
}
