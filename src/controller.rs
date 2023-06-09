//! The controller directs the elevators to operate so that passengers
//! get to their destinations.

use std::{
    collections::{BTreeSet, HashSet},
    ops::Range,
};

use crate::building::{BuildingCommand, BuildingEvent, Direction, ElevatorId, FloorId};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
#[derive(Default, Clone)]
struct ElevatorButtonsInfo {
    position: FloorId,
    passenger_count: usize,
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
            Direction::Down => self
                .should_visit
                .range(0..=self.position)
                .rev()
                .next()
                .copied(),
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
    should_visit_by_elevator: &[ElevatorButtonsInfo],
) -> Option<ElevatorId> {
    let mut lowest_distance = std::i32::MAX;
    let mut result = None;
    for (id, elevator) in should_visit_by_elevator.iter().enumerate() {
        if elevator.is_idle()
            || (elevator.current_trip().unwrap().contains(&floor)
                && elevator.direction.unwrap() == direction)
        {
            let distance = elevator.distance_to(floor);
            if distance < lowest_distance {
                lowest_distance = distance;
                result = Some(id);
            }
        }
    }
    result
}

async fn process_waiting_list(
    should_visit_by_elevator: &mut [ElevatorButtonsInfo],
    call_button_pressed_by_floor: &mut HashSet<(FloorId, Direction)>,
    building_cmd_tx: &mpsc::Sender<BuildingCommand>,
) {
    let mut waiters_to_remove = Vec::new();
    for &(floor, direction) in &*call_button_pressed_by_floor {
        if let Some(elevator_id) =
            find_best_elevator_match(floor, direction, should_visit_by_elevator)
        {
            waiters_to_remove.push((floor, direction));

            let elevator_info = should_visit_by_elevator.get_mut(elevator_id).unwrap();
            // Don't stop the elevator suddenly at the current floor if it is moving.
            if floor == elevator_info.position && !elevator_info.is_idle() {
                continue;
            }
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
        }
    }
    for (floor, direction) in waiters_to_remove {
        call_button_pressed_by_floor.remove(&(floor, direction));
    }
}

fn print_state(
    floors_count: usize,
    should_visit_by_elevator: &[ElevatorButtonsInfo],
    call_button_pressed_by_floor: &HashSet<(FloorId, Direction)>,
) {
    let mut print_matrix: Vec<Vec<bool>> =
        vec![vec![false; should_visit_by_elevator.len()]; floors_count];
    let called_floors: HashSet<FloorId> = call_button_pressed_by_floor
        .iter()
        .map(|(floor, _)| *floor)
        .collect();
    for (id, elevator) in should_visit_by_elevator.iter().enumerate() {
        print_matrix[elevator.position][id] = true;
    }

    let to_print = print_matrix
        .into_iter()
        .enumerate()
        .rev()
        .map(|(floor_level, floor)| {
            let button_press = if called_floors.contains(&floor_level) {
                "| . |"
            } else {
                "|   |"
            }
            .to_owned();
            button_press
                + &floor
                    .into_iter()
                    .map(|has_elevator| if has_elevator { " X " } else { "   " })
                    .collect::<Vec<_>>()
                    .join("|")
        })
        .collect::<Vec<_>>()
        .join("\n");
    println!("{}", to_print);
}

pub async fn controller(
    elevator_count: usize,
    floors_count: usize,
    mut events_rx: broadcast::Receiver<BuildingEvent>,
    building_cmd_tx: mpsc::Sender<BuildingCommand>,
) {
    let mut should_visit_by_elevator: Vec<ElevatorButtonsInfo> =
        vec![ElevatorButtonsInfo::default(); elevator_count];
    let mut call_button_pressed_by_floor: HashSet<(FloorId, Direction)> = HashSet::new();

    let sender = Arc::new(building_cmd_tx.clone());
    let send_go_to_floor = |elevator_id: ElevatorId, to: FloorId| {
        let sender = sender.clone();
        async move {
            sender
                .send(BuildingCommand::GoToFloor(elevator_id, to))
                .await
                .unwrap();
        }
    };

    while let Ok(evt) = events_rx.recv().await {
        match evt {
            BuildingEvent::CallButtonPressed(at, direction) => {
                call_button_pressed_by_floor.insert((at, direction));
            }
            BuildingEvent::FloorButtonPressed(elevator_id, destination) => {
                let elevator = should_visit_by_elevator.get_mut(elevator_id).unwrap();
                elevator.should_visit.insert(destination);
                elevator.passenger_count += 1;
                let elevator = should_visit_by_elevator.get_mut(elevator_id).unwrap();
                if elevator.next_step().is_none() {
                    elevator.swap_direction();
                }
                send_go_to_floor(elevator_id, elevator.next_step().unwrap()).await;
            }
            BuildingEvent::AtFloor(elevator_id, floor) => {
                let elevator = should_visit_by_elevator.get_mut(elevator_id).unwrap();
                elevator.should_visit.remove(&floor);
                elevator.position = floor;

                if elevator.next_step().is_none() && !elevator.is_idle() {
                    elevator.swap_direction();
                }

                let elevator = should_visit_by_elevator.get_mut(elevator_id).unwrap();
                if !elevator.is_idle() {
                    send_go_to_floor(elevator_id, elevator.next_step().unwrap()).await;
                } else {
                    elevator.direction = None;
                }
            }
            _ => {}
        }
        process_waiting_list(
            &mut should_visit_by_elevator,
            &mut call_button_pressed_by_floor,
            &building_cmd_tx,
        )
        .await;
        print_state(
            floors_count,
            &should_visit_by_elevator,
            &call_button_pressed_by_floor,
        );
    }
}
