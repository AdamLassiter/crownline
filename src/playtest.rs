use std::{fs, io::Write as _, path::PathBuf};

use bevy::prelude::*;
use crownline_core::{
    AtomicSaveStorage, TransitionEvent, is_in_check,
    scenario::{Player, SCENARIO_SCHEMA_VERSION, ScenarioDefinition},
    state::{Action, ClockState, MatchOutcome, MatchState, TurnPhase},
    write_bytes_atomically,
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    lifecycle::{ClientFlow, LocalSetup},
    rendering::{DisplayedGame, LocalTransitionEventQueue},
};

const PLAYTEST_FORMAT_VERSION: u16 = 1;
const MAX_PLAYTEST_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SideCounts {
    pub(crate) north: u32,
    pub(crate) south: u32,
}

impl SideCounts {
    fn increment(&mut self, player: Player) {
        match player {
            Player::North => self.north = self.north.saturating_add(1),
            Player::South => self.south = self.south.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GeographicCounts {
    pub(crate) north_west: u32,
    pub(crate) north_center: u32,
    pub(crate) north_east: u32,
    pub(crate) south_west: u32,
    pub(crate) south_center: u32,
    pub(crate) south_east: u32,
}

impl GeographicCounts {
    fn record(&mut self, player: Player, x: u16, width: u16) {
        let ordering = (u32::from(x) * 2).cmp(&u32::from(width.saturating_sub(1)));
        match (player, ordering) {
            (Player::North, std::cmp::Ordering::Less) => {
                self.north_west = self.north_west.saturating_add(1);
            }
            (Player::North, std::cmp::Ordering::Equal) => {
                self.north_center = self.north_center.saturating_add(1);
            }
            (Player::North, std::cmp::Ordering::Greater) => {
                self.north_east = self.north_east.saturating_add(1);
            }
            (Player::South, std::cmp::Ordering::Less) => {
                self.south_west = self.south_west.saturating_add(1);
            }
            (Player::South, std::cmp::Ordering::Equal) => {
                self.south_center = self.south_center.saturating_add(1);
            }
            (Player::South, std::cmp::Ordering::Greater) => {
                self.south_east = self.south_east.saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContactKind {
    Capture,
    Check,
    SettlementClaim,
    SettlementContest,
    SettlementTransfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContactMilestone {
    pub(crate) turn: u64,
    pub(crate) revision: u64,
    pub(crate) kind: ContactKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlaytestActionRecord {
    pub(crate) turn: u64,
    pub(crate) revision: u64,
    pub(crate) player: Option<Player>,
    pub(crate) action: Option<Action>,
    pub(crate) events: Vec<TransitionEvent>,
    pub(crate) state_hash: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QualitativeReview {
    pub(crate) governance_clarity: Option<String>,
    pub(crate) growth_speed: Option<String>,
    pub(crate) economic_conflict: Option<String>,
    pub(crate) promotion_pressure: Option<String>,
    pub(crate) downtime: Option<String>,
    pub(crate) major_piece_overload: Option<String>,
    pub(crate) checkmate_viability: Option<String>,
    pub(crate) observations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlaytestReport {
    pub(crate) format_version: u16,
    pub(crate) application_version: String,
    pub(crate) build_revision: String,
    pub(crate) scenario_id: String,
    pub(crate) scenario_schema_version: u16,
    pub(crate) scenario_hash: String,
    pub(crate) board_width: u16,
    pub(crate) board_height: u16,
    pub(crate) expected_minutes: (u16, u16),
    pub(crate) initial_clock: Option<ClockState>,
    pub(crate) session_id: u64,
    pub(crate) partial_record: bool,
    pub(crate) start_revision: u64,
    pub(crate) final_revision: u64,
    pub(crate) turn_count: u64,
    pub(crate) active_duration_millis: u64,
    pub(crate) paused_duration_millis: u64,
    pub(crate) mandatory_choice_duration_millis: u64,
    pub(crate) first_contact: Option<ContactMilestone>,
    pub(crate) claims: SideCounts,
    pub(crate) produced_pawns: SideCounts,
    pub(crate) promoted_pawns: SideCounts,
    pub(crate) checks_delivered: SideCounts,
    pub(crate) last_check_against: Option<(u64, Player)>,
    pub(crate) geographic_moves: GeographicCounts,
    pub(crate) outcome: Option<MatchOutcome>,
    pub(crate) actions: Vec<PlaytestActionRecord>,
    pub(crate) qualitative_review: QualitativeReview,
}

impl PlaytestReport {
    fn new(
        scenario: &ScenarioDefinition,
        state: &MatchState,
        session_id: u64,
    ) -> Result<Self, String> {
        Ok(Self {
            format_version: PLAYTEST_FORMAT_VERSION,
            application_version: env!("CARGO_PKG_VERSION").to_owned(),
            build_revision: option_env!("CROWNLINE_BUILD_REVISION")
                .unwrap_or("development")
                .to_owned(),
            scenario_id: scenario.id.clone(),
            scenario_schema_version: SCENARIO_SCHEMA_VERSION,
            scenario_hash: scenario
                .canonical_hash()
                .map_err(|error| error.to_string())?,
            board_width: scenario.board.width,
            board_height: scenario.board.height,
            expected_minutes: scenario.metadata.expected_minutes,
            initial_clock: state.clocks,
            session_id,
            partial_record: state.revision != 0,
            start_revision: state.revision,
            final_revision: state.revision,
            turn_count: state.turn_number,
            active_duration_millis: 0,
            paused_duration_millis: 0,
            mandatory_choice_duration_millis: 0,
            first_contact: None,
            claims: SideCounts::default(),
            produced_pawns: SideCounts::default(),
            promoted_pawns: SideCounts::default(),
            checks_delivered: SideCounts::default(),
            last_check_against: None,
            geographic_moves: GeographicCounts::default(),
            outcome: state.outcome,
            actions: Vec::new(),
            qualitative_review: QualitativeReview::default(),
        })
    }

    fn record(
        &mut self,
        scenario: &ScenarioDefinition,
        record: crate::rendering::LocalTransitionRecord,
    ) {
        let player = record.action.as_ref().map(action_player);
        let turn = record
            .events
            .iter()
            .find_map(|event| {
                if let TransitionEvent::TurnStarted { turn_number, .. } = event {
                    Some(turn_number.saturating_sub(1))
                } else {
                    None
                }
            })
            .unwrap_or(record.state.turn_number);
        let revision = record.state.revision;
        if let Some(Action::Move { player, to, .. }) = record.action.as_ref() {
            self.geographic_moves
                .record(*player, to.x, scenario.board.width);
        }
        for event in &record.events {
            match *event {
                TransitionEvent::PieceCaptured { .. } => {
                    self.note_contact(turn, revision, ContactKind::Capture);
                }
                TransitionEvent::SettlementClaimed { owner, .. } => {
                    self.claims.increment(owner);
                    self.note_contact(turn, revision, ContactKind::SettlementClaim);
                }
                TransitionEvent::SettlementContested { .. } => {
                    self.note_contact(turn, revision, ContactKind::SettlementContest);
                }
                TransitionEvent::SettlementTransferred { owner, .. } => {
                    self.claims.increment(owner);
                    self.note_contact(turn, revision, ContactKind::SettlementTransfer);
                }
                TransitionEvent::PawnProduced { .. } => {
                    if let Some(player) = player {
                        self.produced_pawns.increment(player);
                    }
                }
                TransitionEvent::PiecePromoted { .. } => {
                    if let Some(player) = player {
                        self.promoted_pawns.increment(player);
                    }
                }
                _ => {}
            }
        }
        if record.state.outcome.is_none() {
            let checked_player = record.state.active_player;
            if is_in_check(scenario, &record.state, checked_player).unwrap_or(false) {
                let checking_player = checked_player.opponent();
                if self.last_check_against != Some((turn, checked_player)) {
                    self.last_check_against = Some((turn, checked_player));
                    self.checks_delivered.increment(checking_player);
                    self.note_contact(turn, revision, ContactKind::Check);
                }
            } else {
                self.last_check_against = None;
            }
        } else if record.state.outcome.is_some_and(|outcome| {
            outcome.reason == crownline_core::state::OutcomeReason::Checkmate
        }) {
            let checking_player = record.state.active_player.opponent();
            if self.last_check_against != Some((turn, record.state.active_player)) {
                self.last_check_against = Some((turn, record.state.active_player));
                self.checks_delivered.increment(checking_player);
                self.note_contact(turn, revision, ContactKind::Check);
            }
        }
        self.final_revision = revision;
        self.turn_count = record.state.turn_number;
        self.outcome = record.state.outcome;
        self.actions.push(PlaytestActionRecord {
            turn,
            revision,
            player,
            action: record.action,
            events: record.events,
            state_hash: record
                .state
                .canonical_hash()
                .unwrap_or_else(|error| format!("unavailable:{error}")),
        });
    }

    fn note_contact(&mut self, turn: u64, revision: u64, kind: ContactKind) {
        self.first_contact.get_or_insert(ContactMilestone {
            turn,
            revision,
            kind,
        });
    }
}

fn action_player(action: &Action) -> Player {
    match *action {
        Action::Move { player, .. }
        | Action::Hold { player }
        | Action::ChoosePromotion { player, .. }
        | Action::PlacePawn { player, .. }
        | Action::Resign { player }
        | Action::OfferDraw { player }
        | Action::RespondToDraw { player, .. } => player,
    }
}

#[derive(Resource, Default)]
pub(crate) struct PlaytestRecorder {
    report: Option<PlaytestReport>,
    observed_session_id: u64,
    observed_revision: u64,
}

#[derive(Resource)]
pub(crate) struct PlaytestStatus {
    pub(crate) message: String,
}

impl Default for PlaytestStatus {
    fn default() -> Self {
        Self {
            message: "F8 exports a local, name-free playtest record.".to_owned(),
        }
    }
}

pub struct PlaytestPlugin;

impl Plugin for PlaytestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlaytestRecorder>()
            .init_resource::<PlaytestStatus>()
            .add_systems(
                PostUpdate,
                (sync_and_capture_playtest, export_playtest).chain(),
            );
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn sync_and_capture_playtest(
    time: Option<Res<Time<Real>>>,
    flow: Res<ClientFlow>,
    setup: Res<LocalSetup>,
    game: Res<DisplayedGame>,
    mut transitions: ResMut<LocalTransitionEventQueue>,
    mut recorder: ResMut<PlaytestRecorder>,
    mut status: ResMut<PlaytestStatus>,
) {
    if matches!(*flow, ClientFlow::OnlineLobby | ClientFlow::OnlinePlaying) {
        transitions.drain_local_records().for_each(drop);
        return;
    }
    let discontinuity = transitions.take_local_discontinuity();
    let revision_jump = game.state.revision < recorder.observed_revision
        || game.state.revision > recorder.observed_revision.saturating_add(1);
    let needs_report = setup.session_id > 0
        && (recorder.report.is_none()
            || recorder.observed_session_id != setup.session_id
            || recorder
                .report
                .as_ref()
                .is_some_and(|report| report.scenario_id != game.scenario.id)
            || revision_jump
            || discontinuity);
    if needs_report {
        match PlaytestReport::new(&game.scenario, &game.state, setup.session_id) {
            Ok(report) => {
                recorder.report = Some(report);
                status.message = if game.state.revision == 0 {
                    "Playtest record started in memory; F8 exports locally.".to_owned()
                } else {
                    "Partial playtest record started from a loaded revision; F8 exports locally."
                        .to_owned()
                };
            }
            Err(error) => status.message = format!("Playtest recorder unavailable: {error}"),
        }
    }
    if let (Some(delta), Some(report)) = (
        time.as_deref()
            .map(Time::delta)
            .map(|delta| delta.as_millis()),
        recorder.report.as_mut(),
    ) {
        let delta = u64::try_from(delta).unwrap_or(u64::MAX);
        match *flow {
            ClientFlow::Playing => {
                report.active_duration_millis = report.active_duration_millis.saturating_add(delta);
                if matches!(game.state.phase, TurnPhase::ResolvingChoices { .. }) {
                    report.mandatory_choice_duration_millis = report
                        .mandatory_choice_duration_millis
                        .saturating_add(delta);
                }
            }
            ClientFlow::Paused | ClientFlow::ConfirmResign => {
                report.paused_duration_millis = report.paused_duration_millis.saturating_add(delta);
            }
            _ => {}
        }
    }
    let records = transitions.drain_local_records().collect::<Vec<_>>();
    if let Some(report) = recorder.report.as_mut() {
        for record in records {
            report.record(&game.scenario, record);
        }
    }
    recorder.observed_session_id = setup.session_id;
    recorder.observed_revision = game.state.revision;
}

#[allow(clippy::needless_pass_by_value)]
fn export_playtest(
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<ClientFlow>,
    game: Res<DisplayedGame>,
    recorder: Res<PlaytestRecorder>,
    mut status: ResMut<PlaytestStatus>,
) {
    if !keys.just_pressed(KeyCode::F8)
        || matches!(
            *flow,
            ClientFlow::Setup | ClientFlow::OnlineLobby | ClientFlow::OnlinePlaying
        )
    {
        return;
    }
    if active_fog_export_is_locked(&game) {
        "Fog playtest export is locked until the match ends; active hidden truth remains private."
            .clone_into(&mut status.message);
        return;
    }
    let Some(report) = recorder.report.as_ref() else {
        "No local playtest session is available to export.".clone_into(&mut status.message);
        return;
    };
    status.message = match write_report(report) {
        Ok(path) => format!(
            "Explicit local playtest export written to {}. Add consented qualitative review before submission.",
            path.display()
        ),
        Err(error) => format!("Playtest export failed without upload: {error}"),
    };
}

fn active_fog_export_is_locked(game: &DisplayedGame) -> bool {
    game.scenario.rules.fog.is_some() && game.state.outcome.is_none()
}

fn write_report(report: &PlaytestReport) -> Result<PathBuf, String> {
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_PLAYTEST_BYTES {
        return Err(format!(
            "record exceeds the {MAX_PLAYTEST_BYTES} byte local export limit"
        ));
    }
    serde_json::from_slice::<PlaytestReport>(&bytes).map_err(|error| error.to_string())?;
    let path = playtest_path(report)?;
    let mut storage = FilePlaytestStorage::new(path.clone());
    write_bytes_atomically(&mut storage, &bytes, |temporary| {
        serde_json::from_slice::<PlaytestReport>(temporary)
            .map(|_| ())
            .map_err(|error| crownline_core::PersistenceError::MalformedJson(error.to_string()))
    })
    .map_err(|error| error.to_string())?;
    Ok(path)
}

fn playtest_path(report: &PlaytestReport) -> Result<PathBuf, String> {
    let scenario = report
        .scenario_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    ProjectDirs::from("org", "Crownlines", "Crownlines")
        .map(|dirs| {
            dirs.data_local_dir().join("playtests").join(format!(
                "{scenario}-session-{}-{}.json",
                report.session_id,
                Uuid::new_v4()
            ))
        })
        .ok_or_else(|| "platform playtest directory is unavailable".to_owned())
}

struct FilePlaytestStorage {
    current: PathBuf,
    temporary: PathBuf,
}

impl FilePlaytestStorage {
    fn new(current: PathBuf) -> Self {
        let temporary = current.with_extension("json.tmp");
        Self { current, temporary }
    }
}

impl AtomicSaveStorage for FilePlaytestStorage {
    fn write_temporary(&mut self, bytes: &[u8]) -> Result<(), String> {
        let parent = self
            .current
            .parent()
            .ok_or_else(|| "playtest path has no parent directory".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let mut file = fs::File::create(&self.temporary).map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())
    }

    fn read_temporary(&mut self) -> Result<Vec<u8>, String> {
        fs::read(&self.temporary).map_err(|error| error.to_string())
    }

    fn replace_with_temporary(&mut self) -> Result<(), String> {
        fs::rename(&self.temporary, &self.current).map_err(|error| error.to_string())
    }

    fn discard_temporary(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}

#[cfg(test)]
mod tests {
    use crownline_core::{
        rules::TransitionEvent,
        scenario::{Coord, Player},
        state::{MatchOutcome, OutcomeReason, PieceId},
    };

    use super::*;
    use crate::rendering::LocalTransitionRecord;

    fn scenario() -> ScenarioDefinition {
        ron::from_str(include_str!("../assets/scenarios/introductory.ron")).unwrap()
    }

    #[test]
    fn fog_full_truth_export_is_terminal_only() {
        let mut scenario = scenario();
        scenario.rules.fog = Some(crownline_core::FogRules {
            schema_version: crownline_core::FOG_RULES_SCHEMA_VERSION,
            vision_radius: 3,
        });
        let mut game = DisplayedGame {
            state: MatchState::from_scenario(&scenario).unwrap(),
            scenario,
        };
        assert!(active_fog_export_is_locked(&game));
        game.state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        assert!(!active_fog_export_is_locked(&game));
    }

    #[test]
    fn accepted_transitions_produce_required_balance_metrics() {
        let scenario = scenario();
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let mut report = PlaytestReport::new(&scenario, &state, 7).unwrap();
        state.revision = 1;
        state.turn_number = 2;
        state.outcome = Some(MatchOutcome {
            winner: Some(Player::South),
            reason: OutcomeReason::Checkmate,
        });
        let action = Action::Move {
            player: Player::South,
            piece: PieceId(0),
            to: Coord::new(3, 4),
        };
        report.record(
            &scenario,
            LocalTransitionRecord {
                action: Some(action),
                state,
                events: vec![
                    TransitionEvent::PieceCaptured {
                        piece: PieceId(1),
                        at: Coord::new(3, 4),
                    },
                    TransitionEvent::SettlementClaimed {
                        settlement_index: 0,
                        owner: Player::South,
                        founder: PieceId(0),
                    },
                    TransitionEvent::PawnProduced {
                        settlement_index: 0,
                        pawn: PieceId(99),
                        at: Coord::new(2, 4),
                    },
                    TransitionEvent::PiecePromoted {
                        pawn: PieceId(99),
                        promoted: PieceId(100),
                        kind: crownline_core::scenario::PieceKind::Queen,
                        at: Coord::new(2, 0),
                    },
                ],
            },
        );
        assert_eq!(report.first_contact.unwrap().kind, ContactKind::Capture);
        assert_eq!(report.claims.south, 1);
        assert_eq!(report.produced_pawns.south, 1);
        assert_eq!(report.promoted_pawns.south, 1);
        assert_eq!(report.turn_count, 2);
        assert_eq!(report.outcome.unwrap().reason, OutcomeReason::Checkmate);
        assert_eq!(report.actions.len(), 1);
    }

    #[test]
    fn report_is_name_free_versioned_and_round_trips() {
        let scenario = scenario();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let report = PlaytestReport::new(&scenario, &state, 3).unwrap();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("North Player"));
        assert!(!json.contains("South Player"));
        assert!(!json.contains("credential"));
        assert_eq!(
            serde_json::from_str::<PlaytestReport>(&json).unwrap(),
            report
        );
        assert_eq!(report.format_version, PLAYTEST_FORMAT_VERSION);
        assert!(!report.partial_record);
    }

    #[test]
    fn loaded_revision_is_explicitly_partial() {
        let scenario = scenario();
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.revision = 12;
        let report = PlaytestReport::new(&scenario, &state, 4).unwrap();
        assert!(report.partial_record);
        assert_eq!(report.start_revision, 12);
    }

    #[test]
    fn only_explicit_local_transitions_enter_the_recorder_queue() {
        let scenario = scenario();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let action = Action::Hold {
            player: state.active_player,
        };
        let transition = crownline_core::apply_action(&scenario, &state, &action).unwrap();
        let mut queue = LocalTransitionEventQueue::default();

        queue.push_transition(&transition);
        assert_eq!(queue.drain_local_records().count(), 0);

        queue.push_local_action(&action, &transition);
        let recorded = queue.drain_local_records().collect::<Vec<_>>();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].action.as_ref(), Some(&action));
        assert_eq!(recorded[0].state, transition.state);
    }

    #[test]
    fn local_load_discontinuity_is_consumed_once() {
        let mut queue = LocalTransitionEventQueue::default();
        queue.mark_local_discontinuity();
        assert!(queue.take_local_discontinuity());
        assert!(!queue.take_local_discontinuity());
    }

    #[test]
    fn plugin_captures_a_local_action_after_session_start() {
        let scenario = scenario();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let action = Action::Hold {
            player: state.active_player,
        };
        let transition = crownline_core::apply_action(&scenario, &state, &action).unwrap();
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ClientFlow::Playing)
            .insert_resource(LocalSetup {
                session_id: 1,
                ..LocalSetup::default()
            })
            .insert_resource(DisplayedGame {
                scenario,
                state: state.clone(),
            })
            .init_resource::<LocalTransitionEventQueue>()
            .add_plugins(PlaytestPlugin);
        app.update();
        app.world_mut()
            .resource_mut::<LocalTransitionEventQueue>()
            .push_local_action(&action, &transition);
        app.world_mut().resource_mut::<DisplayedGame>().state = transition.state;
        app.update();

        let recorder = app.world().resource::<PlaytestRecorder>();
        let report = recorder.report.as_ref().unwrap();
        assert_eq!(report.actions.len(), 1);
        assert_eq!(report.actions[0].action.as_ref(), Some(&action));
        assert!(!report.partial_record);
    }
}
