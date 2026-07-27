use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use crownline_core::{
    ClockSettings, MatchState, ScenarioDefinition, scenario::Player, start_clocks,
};
use crownline_protocol::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, PROTOCOL_VERSION,
    ReconnectToken, validate_create_room, validate_join_room,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const ROOM_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const MAX_CODE_ATTEMPTS: usize = 32;
const TOKEN_PARTS: usize = 2;
const MAX_INVALID_TOKEN_ATTEMPTS: u8 = 8;
const TOKEN_ATTEMPT_WINDOW: Duration = Duration::from_mins(1);

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

    #[cfg(test)]
    pub(crate) fn from_scenarios(
        definitions: impl IntoIterator<Item = ScenarioDefinition>,
    ) -> Self {
        Self(
            definitions
                .into_iter()
                .map(|definition| {
                    definition.validate().expect("test scenario must validate");
                    let hash = definition
                        .canonical_hash()
                        .expect("test scenario must hash");
                    (
                        definition.id.clone(),
                        InstalledScenario { definition, hash },
                    )
                })
                .collect(),
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TokenHash([u8; 32]);

impl std::fmt::Debug for TokenHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TokenHash([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedSeatRecord {
    pub player: Player,
    pub display_name: String,
    pub token_hash: [u8; 32],
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedRoomRecord {
    pub code: String,
    pub match_id: Uuid,
    pub scenario_id: String,
    pub scenario_hash: String,
    pub clock: Option<ClockSettings>,
    pub phase: RoomPhase,
    pub seats: [PersistedSeatRecord; 2],
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
    token_hash: TokenHash,
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
    created_at: Instant,
    last_activity: Instant,
    ever_started: bool,
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

    fn player_for_token(&self, token: &str) -> Option<Player> {
        let candidate = hash_token(token);
        if constant_time_eq(&self.north.token_hash.0, &candidate.0) {
            Some(Player::North)
        } else if self
            .south
            .as_ref()
            .is_some_and(|seat| constant_time_eq(&seat.token_hash.0, &candidate.0))
        {
            Some(Player::South)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct CreatedRoom {
    pub response: CreateRoomResponse,
}

#[derive(Debug)]
pub struct JoinedRoom {
    pub response: JoinRoomResponse,
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
    #[error("too many credential attempts")]
    RateLimited,
}

#[derive(Debug, Clone, Copy)]
struct FailedTokenWindow {
    started: Instant,
    attempts: u8,
}

pub struct RoomService {
    catalog: ScenarioCatalog,
    rooms: BTreeMap<String, Room>,
    failed_token_attempts: BTreeMap<String, FailedTokenWindow>,
    code_seed: u64,
    max_rooms: usize,
}

impl RoomService {
    pub fn new(catalog: ScenarioCatalog) -> Self {
        Self {
            catalog,
            rooms: BTreeMap::new(),
            failed_token_attempts: BTreeMap::new(),
            code_seed: 0,
            max_rooms: crate::limits::ServerLimits::default().max_rooms,
        }
    }

    #[must_use]
    pub fn with_max_rooms(mut self, maximum: usize) -> Self {
        self.max_rooms = maximum;
        self
    }

    /// Creates a lobby after validating host configuration against installed content.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid input, unknown content, or code exhaustion.
    #[allow(clippy::needless_pass_by_value)]
    pub fn create(&mut self, request: CreateRoomRequest) -> Result<CreatedRoom, RoomError> {
        validate_create_room(&request).map_err(|_| RoomError::InvalidRequest)?;
        if self.rooms.len() >= self.max_rooms {
            return Err(RoomError::RateLimited);
        }
        let installed = self
            .catalog
            .get(&request.scenario_id)
            .ok_or(RoomError::UnknownScenario)?;
        let scenario_id = installed.definition.id.clone();
        let scenario_hash = installed.hash.clone();
        let code = self.allocate_code()?;
        let match_id = Uuid::new_v4();
        let raw_token = issue_token();
        let now = Instant::now();
        let room = Room {
            code: code.clone(),
            match_id,
            scenario_id,
            scenario_hash,
            clock: request.clock,
            phase: RoomPhase::WaitingForOpponent,
            state: None,
            created_at: now,
            last_activity: now,
            ever_started: false,
            north: Seat {
                token_hash: hash_token(&raw_token),
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
                reconnect_token: ReconnectToken::issued(raw_token),
            },
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
        let raw_token = issue_token();
        room.south = Some(Seat {
            token_hash: hash_token(&raw_token),
            name: request.player_name.trim().to_owned(),
            ready: false,
        });
        room.phase = RoomPhase::WaitingForReady;
        room.last_activity = Instant::now();
        Ok(JoinedRoom {
            response: JoinRoomResponse {
                protocol_version: PROTOCOL_VERSION,
                match_id: room.match_id,
                seat: Player::South,
                reconnect_token: ReconnectToken::issued(raw_token),
            },
        })
    }

    /// Marks an authenticated seat ready and starts once both seats are ready.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential/phase, or invalid state.
    pub fn ready(&mut self, code: &str, token: &str) -> Result<RoomPhase, RoomError> {
        let code = normalize_code(code);
        let player = self.authenticate(&code, token)?;
        let room = self.rooms.get_mut(&code).ok_or(RoomError::Unauthorized)?;
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
            room.ever_started = true;
            room.last_activity = Instant::now();
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
        room.last_activity = Instant::now();
        Ok(())
    }

    /// Applies a newly committed canonical state to the room lifecycle.
    ///
    /// Results for an earlier match are rejected so a late response cannot overwrite a rematch.
    ///
    /// # Errors
    ///
    /// Returns an error when the room or match does not correspond to the committed state.
    pub fn sync_committed_state(
        &mut self,
        code: &str,
        match_id: Uuid,
        state: MatchState,
    ) -> Result<RoomPhase, RoomError> {
        let room = self.room_mut(code)?;
        if room.match_id != match_id || room.scenario_id != state.scenario_id {
            return Err(RoomError::InvalidRequest);
        }
        room.phase = if state.outcome.is_some() {
            RoomPhase::Finished
        } else {
            RoomPhase::Playing
        };
        room.state = Some(state);
        room.last_activity = Instant::now();
        Ok(room.phase)
    }

    /// Records acceptance and creates a fresh match once both seats accept.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential/phase, or invalid state.
    pub fn accept_rematch(&mut self, code: &str, token: &str) -> Result<RoomPhase, RoomError> {
        let code = normalize_code(code);
        let player = self.authenticate(&code, token)?;
        let room = self.rooms.get_mut(&code).ok_or(RoomError::Unauthorized)?;
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
            room.ever_started = true;
            room.last_activity = Instant::now();
            room.rematch_acceptances.clear();
        }
        Ok(room.phase)
    }

    /// Clears an outstanding rematch negotiation for an authenticated seat.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential, or non-terminal room.
    pub fn decline_rematch(&mut self, code: &str, token: &str) -> Result<(), RoomError> {
        let code = normalize_code(code);
        self.authenticate(&code, token)?;
        let room = self.rooms.get_mut(&code).ok_or(RoomError::Unauthorized)?;
        if room.phase != RoomPhase::Finished {
            return Err(RoomError::WrongPhase);
        }
        room.rematch_acceptances.clear();
        room.last_activity = Instant::now();
        Ok(())
    }

    /// Removes a lobby seat; a host departure removes the room.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing room, bad credential, or active match.
    pub fn leave_lobby(&mut self, code: &str, token: &str) -> Result<bool, RoomError> {
        let code = normalize_code(code);
        let player = self.authenticate(&code, token)?;
        let room = self.rooms.get_mut(&code).ok_or(RoomError::Unauthorized)?;
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
        room.last_activity = Instant::now();
        Ok(false)
    }

    pub fn room(&self, code: &str) -> Option<&Room> {
        self.rooms.get(&normalize_code(code))
    }

    pub fn persisted_room(&self, code: &str) -> Option<PersistedRoomRecord> {
        let room = self.room(code)?;
        let south = room.south.as_ref()?;
        Some(PersistedRoomRecord {
            code: room.code.clone(),
            match_id: room.match_id,
            scenario_id: room.scenario_id.clone(),
            scenario_hash: room.scenario_hash.clone(),
            clock: room.clock,
            phase: room.phase,
            seats: [
                PersistedSeatRecord {
                    player: Player::North,
                    display_name: room.north.name.clone(),
                    token_hash: room.north.token_hash.0,
                    ready: room.north.ready,
                },
                PersistedSeatRecord {
                    player: Player::South,
                    display_name: south.name.clone(),
                    token_hash: south.token_hash.0,
                    ready: south.ready,
                },
            ],
        })
    }

    pub fn installed_scenario_for_room(&self, code: &str) -> Option<InstalledScenario> {
        let room = self.room(code)?;
        self.catalog.get(&room.scenario_id).cloned()
    }

    /// Authenticates a seat secret without exposing whether a room or seat exists.
    ///
    /// # Errors
    ///
    /// Returns unauthorized or rate-limited errors for invalid credentials.
    pub fn authenticate_seat(
        &mut self,
        code: &str,
        token: &str,
    ) -> Result<(Uuid, Player, RoomPhase), RoomError> {
        let code = normalize_code(code);
        let player = self.authenticate(&code, token)?;
        let room = self.rooms.get(&code).ok_or(RoomError::Unauthorized)?;
        Ok((room.match_id, player, room.phase))
    }

    /// Reconstructs a started room from validated database records.
    ///
    /// # Errors
    ///
    /// Returns an invalid-request error for inconsistent seats, phase, scenario, or state.
    pub fn restore_started_room(
        &mut self,
        record: PersistedRoomRecord,
        state: MatchState,
    ) -> Result<(), RoomError> {
        if self.rooms.contains_key(&record.code)
            || state.scenario_id != record.scenario_id
            || !matches!(record.phase, RoomPhase::Playing | RoomPhase::Finished)
            || record.seats[0].player != Player::North
            || record.seats[1].player != Player::South
            || record.seats[0].token_hash == record.seats[1].token_hash
        {
            return Err(RoomError::InvalidRequest);
        }
        let now = Instant::now();
        let north = &record.seats[0];
        let south = &record.seats[1];
        self.rooms.insert(
            record.code.clone(),
            Room {
                code: record.code,
                match_id: record.match_id,
                scenario_id: record.scenario_id,
                scenario_hash: record.scenario_hash,
                clock: record.clock,
                phase: record.phase,
                state: Some(state),
                created_at: now,
                last_activity: now,
                ever_started: true,
                north: Seat {
                    token_hash: TokenHash(north.token_hash),
                    name: north.display_name.clone(),
                    ready: north.ready,
                },
                south: Some(Seat {
                    token_hash: TokenHash(south.token_hash),
                    name: south.display_name.clone(),
                    ready: south.ready,
                }),
                rematch_acceptances: BTreeSet::new(),
            },
        );
        Ok(())
    }

    /// Removes only never-started rooms whose last activity exceeded the limit.
    pub fn expire_idle_pregame(&mut self, now: Instant, maximum_idle: Duration) -> usize {
        let before = self.rooms.len();
        self.rooms.retain(|code, room| {
            let expired = !room.ever_started
                && matches!(room.phase, RoomPhase::WaitingForOpponent | RoomPhase::WaitingForReady)
                && now.duration_since(room.last_activity) >= maximum_idle;
            if expired {
                tracing::info!(%code, age_seconds = now.duration_since(room.created_at).as_secs(), "expired idle pre-game room");
            }
            !expired
        });
        let removed = before - self.rooms.len();
        self.failed_token_attempts
            .retain(|code, _| self.rooms.contains_key(code));
        removed
    }

    fn room_mut(&mut self, code: &str) -> Result<&mut Room, RoomError> {
        self.rooms
            .get_mut(&normalize_code(code))
            .ok_or(RoomError::NotFound)
    }

    fn authenticate(&mut self, code: &str, token: &str) -> Result<Player, RoomError> {
        let now = Instant::now();
        if let Some(window) = self.failed_token_attempts.get_mut(code) {
            if now.duration_since(window.started) >= TOKEN_ATTEMPT_WINDOW {
                *window = FailedTokenWindow {
                    started: now,
                    attempts: 0,
                };
            } else if window.attempts >= MAX_INVALID_TOKEN_ATTEMPTS {
                return Err(RoomError::RateLimited);
            }
        }
        if let Some(player) = self
            .rooms
            .get(code)
            .and_then(|room| room.player_for_token(token))
        {
            self.failed_token_attempts.remove(code);
            return Ok(player);
        }
        let window =
            self.failed_token_attempts
                .entry(code.to_owned())
                .or_insert(FailedTokenWindow {
                    started: now,
                    attempts: 0,
                });
        window.attempts = window.attempts.saturating_add(1);
        Err(RoomError::Unauthorized)
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

fn issue_token() -> String {
    (0..TOKEN_PARTS)
        .map(|_| Uuid::new_v4().simple().to_string())
        .collect()
}

fn hash_token(token: &str) -> TokenHash {
    TokenHash(Sha256::digest(token.as_bytes()).into())
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
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
        let host_token = created.response.reconnect_token.expose().to_owned();
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
        let guest_token = joined.response.reconnect_token.expose().to_owned();
        assert_eq!(
            service.ready(&created.response.room_code, &host_token),
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
            service.ready(&created.response.room_code, &guest_token),
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
        let host_token = created.response.reconnect_token.expose().to_owned();
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        let guest_token = joined.response.reconnect_token.expose().to_owned();
        assert_eq!(
            service
                .join(join_request(&created.response.room_code))
                .unwrap_err(),
            RoomError::Full
        );
        assert_eq!(
            service.ready(&created.response.room_code, "invalid-token"),
            Err(RoomError::Unauthorized)
        );
        assert_ne!(host_token, guest_token);
    }

    #[test]
    fn lobby_leave_and_terminal_rematch_are_deterministic() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let created = service.create(create_request()).unwrap();
        let host_token = created.response.reconnect_token.expose().to_owned();
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        let first_guest_token = joined.response.reconnect_token.expose().to_owned();
        assert_eq!(
            service.leave_lobby(&created.response.room_code, &first_guest_token),
            Ok(false)
        );
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        let guest_token = joined.response.reconnect_token.expose().to_owned();
        service
            .ready(&created.response.room_code, &host_token)
            .unwrap();
        service
            .ready(&created.response.room_code, &guest_token)
            .unwrap();
        let old_match = service.room(&created.response.room_code).unwrap().match_id;
        let mut terminal = service
            .room(&created.response.room_code)
            .unwrap()
            .state
            .clone()
            .unwrap();
        terminal.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        assert_eq!(
            service.sync_committed_state(&created.response.room_code, old_match, terminal),
            Ok(RoomPhase::Finished)
        );
        assert_eq!(
            service.accept_rematch(&created.response.room_code, &host_token),
            Ok(RoomPhase::Finished)
        );
        assert_eq!(
            service.accept_rematch(&created.response.room_code, &guest_token),
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

    #[test]
    fn issued_tokens_are_distinct_high_entropy_and_redacted_at_rest() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let created = service.create(create_request()).unwrap();
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        let host_token = created.response.reconnect_token.expose();
        let guest_token = joined.response.reconnect_token.expose();
        assert_eq!(host_token.len(), TOKEN_PARTS * 32);
        assert_eq!(guest_token.len(), TOKEN_PARTS * 32);
        assert_ne!(host_token, guest_token);
        assert_eq!(
            format!("{:?}", created.response.reconnect_token),
            "ReconnectToken([REDACTED])"
        );
        let stored = format!("{:?}", service.room(&created.response.room_code).unwrap());
        assert!(!stored.contains(host_token));
        assert!(!stored.contains(guest_token));
        assert!(stored.contains("TokenHash([REDACTED])"));
    }

    #[test]
    fn invalid_tokens_are_indistinguishable_and_rate_limited_per_room_locator() {
        let mut service = RoomService::new(ScenarioCatalog::installed());
        let created = service.create(create_request()).unwrap();
        assert_eq!(
            service.ready(&created.response.room_code, "wrong"),
            Err(RoomError::Unauthorized)
        );
        assert_eq!(
            service.ready("ZZZZZZ", "wrong"),
            Err(RoomError::Unauthorized)
        );
        for _ in 1..MAX_INVALID_TOKEN_ATTEMPTS {
            assert_eq!(
                service.ready(&created.response.room_code, "wrong"),
                Err(RoomError::Unauthorized)
            );
        }
        assert_eq!(
            service.ready(&created.response.room_code, "wrong"),
            Err(RoomError::RateLimited)
        );
    }

    #[test]
    fn cleanup_expires_only_idle_never_started_rooms_and_room_count_is_bounded() {
        let mut service = RoomService::new(ScenarioCatalog::installed()).with_max_rooms(2);
        let idle = service.create(create_request()).unwrap();
        let active = service.create(create_request()).unwrap();
        let joined = service
            .join(join_request(&active.response.room_code))
            .unwrap();
        let host_token = active.response.reconnect_token.expose().to_owned();
        let guest_token = joined.response.reconnect_token.expose().to_owned();
        service
            .ready(&active.response.room_code, &host_token)
            .unwrap();
        service
            .ready(&active.response.room_code, &guest_token)
            .unwrap();
        assert_eq!(
            service.create(create_request()).unwrap_err(),
            RoomError::RateLimited
        );

        let now = Instant::now();
        service
            .room_mut(&idle.response.room_code)
            .unwrap()
            .last_activity = now.checked_sub(Duration::from_mins(31)).unwrap();
        service
            .room_mut(&active.response.room_code)
            .unwrap()
            .last_activity = now.checked_sub(Duration::from_mins(31)).unwrap();
        assert_eq!(service.expire_idle_pregame(now, Duration::from_mins(30)), 1);
        assert!(service.room(&idle.response.room_code).is_none());
        assert!(service.room(&active.response.room_code).is_some());
        assert!(service.create(create_request()).is_ok());
    }

    #[test]
    fn persisted_hashes_reconstruct_both_authenticated_seats_without_raw_tokens() {
        let catalog = ScenarioCatalog::installed();
        let mut service = RoomService::new(catalog.clone());
        let created = service.create(create_request()).unwrap();
        let joined = service
            .join(join_request(&created.response.room_code))
            .unwrap();
        let host_token = created.response.reconnect_token.expose().to_owned();
        let guest_token = joined.response.reconnect_token.expose().to_owned();
        service
            .ready(&created.response.room_code, &host_token)
            .unwrap();
        service
            .ready(&created.response.room_code, &guest_token)
            .unwrap();
        let record = service.persisted_room(&created.response.room_code).unwrap();
        let state = service
            .room(&created.response.room_code)
            .unwrap()
            .state
            .clone()
            .unwrap();

        let mut restored = RoomService::new(catalog);
        restored.restore_started_room(record, state).unwrap();
        assert_eq!(
            restored
                .authenticate_seat(&created.response.room_code, &host_token)
                .unwrap()
                .1,
            Player::North
        );
        assert_eq!(
            restored
                .authenticate_seat(&created.response.room_code, &guest_token)
                .unwrap()
                .1,
            Player::South
        );
    }
}
