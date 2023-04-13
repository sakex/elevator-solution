use building::BuildingEvent;
use tokio::sync::broadcast;

mod building;
mod controller;
mod driver;

#[tokio::main]
async fn main() {
    let building = driver::make_building();
    let num_floors = building.num_floors();
    let num_elevators = building.num_elevators();
    let (building_task, events_rx, building_cmd_tx, driver_cmd_tx) = building.start();

    tokio::spawn(print_events(events_rx.resubscribe()));
    let driver_handle = tokio::spawn(driver::driver(
        num_floors,
        100,
        events_rx.resubscribe(),
        driver_cmd_tx,
    ));
    tokio::spawn(controller::controller(
        num_elevators,
        events_rx,
        building_cmd_tx,
    ));
    building_task.await.unwrap();
    driver_handle.await;
}

async fn print_events(mut events_rx: broadcast::Receiver<BuildingEvent>) {
    while let Ok(evt) = events_rx.recv().await {
        println!("BuildingEvent::{:?}", evt);
    }
}
