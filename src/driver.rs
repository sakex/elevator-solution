//! The driver controls when and where passengers arrive.

use crate::building::{Building, BuildingEvent, DriverCommand, FloorId};
use rand::Rng;
use tokio::sync::{broadcast, mpsc};

/// Create a new building to be driven by this driver.
pub fn make_building() -> Building {
    // Set num elevators to more than 1 for question 2.
    Building::new(10, 2)
}

/// Simulate people arriving at the ground floor and going to the first floor, one by one.
// ----------- Solution 1 -----------
pub async fn driver(
    num_floors: usize,
    passengers_count: usize,
    mut events_rx: broadcast::Receiver<BuildingEvent>,
    driver_cmd_tx: mpsc::Sender<DriverCommand>,
) {
    for _ in 0..passengers_count {
        let (at, destination, wait_time_ms) = {
            let mut rng = rand::thread_rng();
            let at = rng.gen_range(0..num_floors);
            let destination = rng.gen_range(0..num_floors);
            let wait_time_ms = rng.gen_range(1..=1000);
            (at, destination, wait_time_ms)
        };
        tokio::time::sleep(tokio::time::Duration::from_millis(wait_time_ms)).await;
        // ----------- End solution 1 -----------
        // A passenger has arrived..
        driver_cmd_tx
            .send(DriverCommand::PassengerArrived { at, destination })
            .await
            .unwrap();
    }
    let mut delivered_count = 0;
    // Wait until they are delivered..
    while let Ok(evt) = events_rx.recv().await {
        if let BuildingEvent::PassengerDelivered(_) = evt {
            delivered_count += 1;
            if delivered_count == passengers_count {
                println!("DONE");
                break;
            }
        }
    }
    driver_cmd_tx.send(DriverCommand::Halt).await.unwrap();
}
