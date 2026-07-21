use std::collections::{BTreeMap, BTreeSet};

use crownline_core::{
    ClockSettings, MatchState, ScenarioDefinition, scenario::Player, start_clocks,
};
use crownline_protocol::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, PROTOCOL_VERSION,
    validate_create_room, validate_join_room,
};
use thiserror::Error;
use uuid::Uuid;

const ROOM_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const MAX_CODE_ATTEMPTS: usize = 32;

#[derive(Debug, Clone)]
pub struct InstalledScenario {
    pub definition: ScenarioDefinition,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct ScenarioCatalog(BTreeMap<String, InstalledScenario>);

impl ScenarioCatalog {
    /// Loads and verifies the scenarios installed with this server build.
    ///
    /// # Panics
    ///
    /// Panics only when a build-time scenario asset is malformed or invalid.
    pub fn installed() -> Self {
        let sources = [
            include_str!("../../../assets/scenarios/introductory.ron"),
            include_str!("../../../assets/scenarios/standard.ron"),
            include_str!("../../../assets/scenarios/large.ron"),
        ];
        let entries = sources.into_iter().map(|source| {
            let definition: ScenarioDefinition =
                ron::from_str(source).expect("installed scenario must parse");
            definition
                .validate()
                .expect("installed scenario must validate");
            let hash = definition
                .canonical_hash()
                .expect("installed scenario must hash");
            (
                definition.id.clone(),
                InstalledScenario { definition, hash },
            )
        });
        Self(entries.collect())
    }

    pub fn get(&self, id: &str) -> Option<&InstalledScenario> {
        self.0.get(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SeatKey(Uuid);

impl SeatKey {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn expose(self) -> String {
        self.0.to_string()
    }
}

impl Default for SeatKey {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomPhase {
    WaitingForOpponent,
    WaitingForReady,
    Playing,
    Finished,
}

#[derive(Debug, Clone)]
struct Seat {
    key: SeatKey,
    name: String,
    ready: bool,
}

#[derive(Debug, Clone)]
pub struct Room {
    pub code: String,
    pub match_id: Uuid,
    pub scenario_id: String,
    pub scenario_hash: String,
    pub clock: Option<ClockSettings>,
    pub phase: RoomPhase,
    pub state: Option<MatchState>,
    north: Seat,
    south: Option<Seat>,
    rematch_acceptances: BTreeSet<Player>,
}

impl Room {
    pub fn player_name(&self, player: Player) -> Option<&str> {
        match player {
            Player::North => Some(&self.north.name),
            Player::South => self.south.as_ref().map(|seat| seat.name.as_str()),
        }
    }

    fn player_for_key(&self, key: SeatKey) -> Option<Player> {
        if self.north.key == key {
            Some(Player::North)
        } else if self.south.as_ref().is_some_and(|seat| seat.key == key) {
            Some(Player::South)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct CreatedRoom {
    pub response: CreateRoomResponse,
    pub seat_key: SeatKey,
}

#[derive(Debug)]
pub struct JoinedRoom {
    pub response: JoinRoomResponse,
    pub seat_key: SeatKey,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum RoomError {
    #[error("invalid room request")]
    InvalidRequest,
    #[error("scenario is not installed")]
    UnknownScenario,
    #[error("room was not found")]
    NotFound,
    #[error("room is full")]
    Full,
    #[error("seat credential is invalid")]
    Unauthorized,
    #[error("room is not in the required phase")]
    WrongPhase,
    #[error("room code allocation failed")]
    CodeSpaceExhausted,
}

pub struct RoomService {
    catalog: ScenarioCatalog,
    rooms: BTreeMap<String, Room>,
    code_seed: u64,
}

impl RoomService {
    pub fn new(catalog: ScenarioCatalog) -> Self {
        Self {
            catalog,
            rooms: BTreeMap::new(),
            code_seed: 0,
        }
    }

    /// Creates a lobby after validating host configuration against installed content.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, unknown content, or code exhaustion.
    #[allow(clippy::needless_pass_by_value)]
    pub fn create(&mut self, request: CreateRoomRequest) -> Result<CreatedRoom, RoomError> {
        validate_create_room(&request).map_err(|_| RoomError::InvalidRequest)?;
        let installed = self
            .catalog
            .get(&request.scenario_id)
            .ok_or(RoomError::UnknownScenario)?;
        let scenario_id = installed.definition.id.clone();
        let scenario_hash = installed.hash.clone();
        let code = self.allocate_code()?;
        let match_id = Uuid::new_v4();
        let seat_key = SeatKey::new();
        let room = Room {
            code: code.clone(),
            match_id,
            scenario_id,
            scenario_hash,
            clock: request.clock,
            phase: RoomPhase::WaitingForOpponent,
            state: None,
            north: Seat {
                key: seat_key,
                name: request.player_name.trim().to_owned(),
                ready: false,
            },
            south: None,
            rematch_acceptances: BTreeSet::new(),
        };
        self.rooms.insert(code.clone(), room);
        Ok(CreatedRoom {
            response: CreateRoomResponse {
                protocol_version: PROTOCOL_VERSION,
                match_id,
                room_code: code,
                seat: Player::North,
                reconnect_token: seat_key.expose(),
            },
            seat_key,
        })
    }

    /// Occupies the second seat of a normalized room code.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, a missing/full room, or the wrong phase.
    pub fn join(&mut self, mut request: JoinRoomRequest) -> Result<JoinedRoom, RoomError> {
        request.room_code = normalize_code(&request.room_code);
        validate_join_room(&request).map_err(|_| RoomError::InvalidRequest)?;
        let room = self
            .rooms
            .get_mut(&request.room_code)
            .ok_or(RoomError::NotFound)?;
        if room.south.is_some() {
            return Err(RoomError::Full);
        }
        if room.phase != RoomPhase::WaitingForOpponent {
            return Err(RoomError::WrongPhase);
        }
        let seat_key = SeatKey::new();
        room.south = Some(Seat {
            key: seat_key,
            name: request.player_name.trim().to_owned(),
            ready: false,
        });
        room.phase = RoomPhase::WaitingForReady;
        Ok(JoinedRoom {
            response: JoinRoomResponse {
                protocol_version: PROTOCOL_VERSION,
                match_id: room.match_id,
                seat: Player::South,
                reconnect_token: seat_key.expose(),
            },
            seat_key,
        })
    }

    /// Marks an authenticated seat ready and starts once both seats are ready.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential/phase, or invalid state.
    pub fn ready(&mut self, code: &str, key: SeatKey) -> Result<RoomPhase, RoomError> {
        let code = normalize_code(code);
        let room = self.rooms.get_mut(&code).ok_or(RoomError::NotFound)?;
        let player = room.player_for_key(key).ok_or(RoomError::Unauthorized)?;
        if room.phase != RoomPhase::WaitingForReady {
            return Err(RoomError::WrongPhase);
        }
        match player {
            Player::North => room.north.ready = true,
            Player::South => room.south.as_mut().ok_or(RoomError::Unauthorized)?.ready = true,
        }
        if room.north.ready && room.south.as_ref().is_some_and(|seat| seat.ready) {
            let scenario = &self
                .catalog
                .get(&room.scenario_id)
                .ok_or(RoomError::UnknownScenario)?
                .definition;
            let mut state =
                MatchState::from_scenario(scenario).map_err(|_| RoomError::InvalidRequest)?;
            if let Some(clock) = room.clock {
                state = start_clocks(&state, clock).map_err(|_| RoomError::InvalidRequest)?;
            }
            room.state = Some(state);
            room.phase = RoomPhase::Playing;
        }
        Ok(room.phase)
    }

    /// Moves a canonically terminal room into its rematch phase.
    ///
    /// # Errors
    ///
    /// Returns an error unless the room exists and its state is terminal.
    pub fn mark_finished(&mut self, code: &str) -> Result<(), RoomError> {
        let room = self.room_mut(code)?;
        if room.phase != RoomPhase::Playing
            || room
                .state
                .as_ref()
                .is_none_or(|state| state.outcome.is_none())
        {
            return Err(RoomError::WrongPhase);
        }
        room.phase = RoomPhase::Finished;
        Ok(())
    }

    /// Records acceptance and creates a fresh match once both seats accept.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential/phase, or invalid state.
    pub fn accept_rematch(&mut self, code: &str, key: SeatKey) -> Result<RoomPhase, RoomError> {
        let code = normalize_code(code);
        let room = self.rooms.get_mut(&code).ok_or(RoomError::NotFound)?;
        let player = room.player_for_key(key).ok_or(RoomError::Unauthorized)?;
        if room.phase != RoomPhase::Finished {
            return Err(RoomError::WrongPhase);
        }
        room.rematch_acceptances.insert(player);
        if room.rematch_acceptances.len() == 2 {
            let scenario = &self
                .catalog
                .get(&room.scenario_id)
                .ok_or(RoomError::UnknownScenario)?
                .definition;
            let mut state =
                MatchState::from_scenario(scenario).map_err(|_| RoomError::InvalidRequest)?;
            if let Some(clock) = room.clock {
                state = start_clocks(&state, clock).map_err(|_| RoomError::InvalidRequest)?;
            }
            room.match_id = Uuid::new_v4();
            room.state = Some(state);
            room.phase = RoomPhase::Playing;
            room.rematch_acceptances.clear();
        }
        Ok(room.phase)
    }

    /// Removes a lobby seat; a host departure removes the room.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential, or active match.
    pub fn leave_lobby(&mut self, code: &str, key: SeatKey) -> Result<bool, RoomError> {
        let code = normalize_code(code);
        let room = self.rooms.get_mut(&code).ok_or(RoomError::NotFound)?;
        let player = room.player_for_key(key).ok_or(RoomError::Unauthorized)?;
        if !matches!(
            room.phase,
            RoomPhase::WaitingForOpponent | RoomPhase::WaitingForReady
        ) {
            return Err(RoomError::WrongPhase);
        }
        if player == Player::North {
            self.rooms.remove(&code);
            return Ok(true);
        }
        room.south = None;
        room.north.ready = false;
        room.phase = RoomPhase::WaitingForOpponent;
        Ok(false)
    }

    pub fn room(&self, code: &str) -> Option<&Room> {
        self.rooms.get(&normalize_code(code))
    }

    fn room_mut(&mut self, code: &str) -> Result<&mut Room, RoomError> {
        self.rooms
            .get_mut(&normalize_code(code))
            .ok_or(RoomError::NotFound)
    }

    fn allocate_code(&mut self) -> Result<String, RoomError> {
        for _ in 0..MAX_CODE_ATTEMPTS {
            self.code_seed = self.code_seed.wrapping_add(1);
            let candidate = code_from_uuid(Uuid::new_v4(), self.code_seed);
            if !self.rooms.contains_key(&candidate) {
                return Ok(candidate);
            }
        }
        Err(RoomError::CodeSpaceExhausted)
    }
}

pub fn normalize_code(code: &str) -> String {
    code.trim().to_ascii_uppercase()
}

fn code_from_uuid(uuid: Uuid, seed: u64) -> String {
    uuid.as_bytes()
        .iter()
        .take(crownline_protocol::ROOM_CODE_CHARS)
        .enumerate()
        .map(|(index, byte)| {
            let offset = seed.to_le_bytes()[index];
            char::from(
                ROOM_CODE_ALPHABET
                    [usize::from(byte.wrapping_add(offset)) % ROOM_CODE_ALPHABET.len()],
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crownline_core::state::{MatchOutcome, OutcomeReason};

    use super::*;

    fn create_request() -> CreateRoomRequest {
        CreateRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Host <North>".to_owned(),
            scenario_id: "crownlines-standard".to_owned(),
            clock: Some(ClockSettings {
                base_minutes: 15,
                increment_seconds: 3,
            }),
        }
    }

    fn join_request(code: &str) -> JoinRoomRequest {
        JoinRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Guest & South".to_owned(),
            room_code: format!("  {}  ", code.to_ascii_lowercase()),
        }
    }

    #[test]
    fn create_join_and_ready_starts_validated_scenario_and_clocks_once() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let created = service.create(create_request()).unwrap();
        assert_eq!(
            created.response.room_code.len(),
            crownline_protocol::ROOM_CODE_CHARS
        );
        assert!(
            created
                .response
                .room_code
                .bytes()
                .all(|byte| ROOM_CODE_ALPHABET.contains(&byte))
        );
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        assert_eq!(
            service.ready(&created.response.room_code, created.seat_key),
            Ok(RoomPhase::WaitingForReady)
        );
        assert!(
            service
                .room(&created.response.room_code)
                .unwrap()
                .state
                .is_none()
        );
        assert_eq!(
            service.ready(&created.response.room_code, joined.seat_key),
            Ok(RoomPhase::Playing)
        );
        let room = service.room(&created.response.room_code).unwrap();
        let state = room.state.as_ref().unwrap();
        assert_eq!(state.scenario_id, "crownlines-standard");
        assert_eq!(state.clocks.as_ref().unwrap().north_millis, 15 * 60_000);
        assert_eq!(
            room.scenario_hash,
            service.catalog.get("crownlines-standard").unwrap().hash
        );
    }

    #[test]
    fn full_room_and_wrong_seat_credentials_cannot_join_or_ready() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let created = service.create(create_request()).unwrap();
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        assert_eq!(
            service
                .join(join_request(&created.response.room_code))
                .unwrap_err(),
            RoomError::Full
        );
        assert_eq!(
            service.ready(&created.response.room_code, SeatKey::new()),
            Err(RoomError::Unauthorized)
        );
        assert_ne!(created.seat_key, joined.seat_key);
    }

    #[test]
    fn lobby_leave_and_terminal_rematch_are_deterministic() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let created = service.create(create_request()).unwrap();
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        assert_eq!(
            service.leave_lobby(&created.response.room_code, joined.seat_key),
            Ok(false)
        );
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        service
            .ready(&created.response.room_code, created.seat_key)
            .unwrap();
        service
            .ready(&created.response.room_code, joined.seat_key)
            .unwrap();
        let old_match = service.room(&created.response.room_code).unwrap().match_id;
        service
            .room_mut(&created.response.room_code)
            .unwrap()
            .state
            .as_mut()
            .unwrap()
            .outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        service.mark_finished(&created.response.room_code).unwrap();
        assert_eq!(
            service.accept_rematch(&created.response.room_code, created.seat_key),
            Ok(RoomPhase::Finished)
        );
        assert_eq!(
            service.accept_rematch(&created.response.room_code, joined.seat_key),
            Ok(RoomPhase::Playing)
        );
        let room = service.room(&created.response.room_code).unwrap();
        assert_ne!(room.match_id, old_match);
        assert!(room.state.as_ref().unwrap().outcome.is_none());
    }

    #[test]
    fn unknown_scenario_and_invalid_clock_are_rejected_before_room_creation() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let mut request = create_request();
        request.scenario_id = "not-installed".to_owned();
        assert_eq!(
            service.create(request).unwrap_err(),
            RoomError::UnknownScenario
        );
        let mut request = create_request();
        request.clock.as_mut().unwrap().base_minutes = 0;
        assert_eq!(
            service.create(request).unwrap_err(),
            RoomError::InvalidRequest
        );
    }
}
