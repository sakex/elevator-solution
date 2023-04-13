//! The controller directs the elevators to operate so that passengers
//! get to their destinations.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ops::Range,
};

use crate::building::{BuildingCommand, BuildingEvent, Direction, ElevatorId, FloorId};
use tokio::sync::{broadcast, mpsc};

#[derive(Default)]
struct ElevatorButtonsInfo {
    position: FloorId,
    should_visit: BTreeSet<FloorId>,
    direction: Option<Direction>,
}

impl ElevatorButtonsInfo {
    fn is_idle(&self) -> bool {
        self.should_visit.is_empty()
    }

    fn current_trip(&self) -> Option<Range<FloorId>> {
        let direction = self.direction?;
        let first = *self.should_visit.first()?;
        let last = *self.should_visit.last()?;
        match direction {
            Direction::Up => Some(self.position..last),
            Direction::Down => Some(self.position..first),
        }
    }

    fn next_step(&self) -> Option<FloorId> {
        let direction = self.direction?;
        match direction {
            Direction::Up => self.should_visit.range(self.position..).next().copied(),
            Direction::Down => self.should_visit.range(0..self.position).rev().next().copied(),
        }
    }

    fn distance_to(&self, floor: FloorId) -> i32 {
        (self.position as i32 - floor as i32).abs()
    }

    fn swap_direction(&mut self) {
        self.direction = self.direction.map(|dir| {
            if dir == Direction::Up {
                Direction::Down
            } else {
                Direction::Up
            }
        });
        if self.direction.is_none() {
            let first = *self.should_visit.first().unwrap();
            self.direction = if self.position < first {
                Some(Direction::Up)
            } else {
                Some(Direction::Down)
            };
        }
    }
}

fn find_best_elevator_match(
    floor: FloorId,
    direction: Direction,
    should_visit_by_elevator: &HashMap<ElevatorId, ElevatorButtonsInfo>,
) -> Option<ElevatorId> {
    let mut lowest_distance = std::i32::MAX;
    let mut result = None;
    for (id, elevator) in should_visit_by_elevator {
        if elevator.is_idle()
            || (elevator.current_trip().unwrap().contains(&floor)
                && elevator.direction.unwrap() == direction)
        {
            let distance = elevator.distance_to(floor);
            if distance < lowest_distance {
                lowest_distance = distance;
                result = Some(*id);
            }
        }
    }
    result
}

async fn process_waiting_list(
    should_visit_by_elevator: &mut HashMap<ElevatorId, ElevatorButtonsInfo>,
    call_button_pressed_by_floor: &mut HashSet<(FloorId, Direction)>,
    building_cmd_tx: &mpsc::Sender<BuildingCommand>,
) {
    let mut waiters_to_remove = Vec::new();
    for &(floor, direction) in &*call_button_pressed_by_floor {
        if let Some(elevator_id) =
            find_best_elevator_match(floor, direction, should_visit_by_elevator)
        {
            let elevator_info = should_visit_by_elevator.get_mut(&elevator_id).unwrap();
            elevator_info.should_visit.insert(floor);
            if elevator_info.next_step().is_none() {
                elevator_info.swap_direction();
            }
            building_cmd_tx
                .send(BuildingCommand::GoToFloor(
                    elevator_id,
                    elevator_info.next_step().unwrap(),
                ))
                .await
                .unwrap();
            waiters_to_remove.push((floor, direction));
        }
    }
    for (floor, direction) in waiters_to_remove {
        call_button_pressed_by_floor.remove(&(floor, direction));
    }
}

pub async fn controller(
    elevator_count: usize,
    mut events_rx: broadcast::Receiver<BuildingEvent>,
    building_cmd_tx: mpsc::Sender<BuildingCommand>,
) {
    let mut should_visit_by_elevator: HashMap<ElevatorId, ElevatorButtonsInfo> = (0
        ..elevator_count)
        .map(|id| (id, ElevatorButtonsInfo::default()))
        .collect();
    let mut call_button_pressed_by_floor: HashSet<(FloorId, Direction)> = HashSet::new();

    while let Ok(evt) = events_rx.recv().await {
        match evt {
            BuildingEvent::CallButtonPressed(at, direction) => {
                call_button_pressed_by_floor.insert((at, direction));
                process_waiting_list(
                    &mut should_visit_by_elevator,
                    &mut call_button_pressed_by_floor,
                    &building_cmd_tx,
                )
                .await;
            }
            BuildingEvent::FloorButtonPressed(elevator_id, destination) => {
                let elevator = should_visit_by_elevator.get_mut(&elevator_id).unwrap();
                elevator.should_visit.insert(destination);

                process_waiting_list(
                    &mut should_visit_by_elevator,
                    &mut call_button_pressed_by_floor,
                    &building_cmd_tx,
                )
                .await;
                let elevator = should_visit_by_elevator.get_mut(&elevator_id).unwrap();
                if elevator.next_step().is_none() {
                    elevator.swap_direction();
                }
                building_cmd_tx
                    .send(BuildingCommand::GoToFloor(
                        elevator_id,
                        elevator.next_step().unwrap(),
                    ))
                    .await
                    .unwrap();
            }
            BuildingEvent::AtFloor(elevator_id, floor) => {
                let elevator = should_visit_by_elevator.get_mut(&elevator_id).unwrap();
                elevator.should_visit.remove(&floor);
                elevator.position = floor;
                process_waiting_list(
                    &mut should_visit_by_elevator,
                    &mut call_button_pressed_by_floor,
                    &building_cmd_tx,
                )
                .await;

                let elevator = should_visit_by_elevator.get_mut(&elevator_id).unwrap();
                if elevator.next_step().is_none() && !elevator.is_idle() {
                    elevator.swap_direction();
                }
                if !elevator.is_idle() {
                    building_cmd_tx
                        .send(BuildingCommand::GoToFloor(
                            elevator_id,
                            elevator.next_step().unwrap(),
                        ))
                        .await
                        .unwrap();
                } else {
                    elevator.direction = None;
                }
            }
            _ => {}
        }
    }
}
