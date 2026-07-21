use std::{fs, io::Write, path::PathBuf};

use bevy::prelude::*;
use crownline_core::{
    AtomicSaveStorage, MAX_PERSISTED_BYTES, SaveEnvelope, SaveReader,
    scenario::{SCENARIO_SCHEMA_VERSION, ScenarioDefinition},
    write_bytes_atomically,
};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::{
    lifecycle::{ClientFlow, LocalClockRuntime, LocalSetup},
    rendering::{
        DisplayedGame, LocalTransitionEventQueue, LocalTransitionNoticeLog, OverlaySelection,
    },
};

const LOCAL_SAVE_FORMAT_VERSION: u16 = 1;
const SLOT_COUNT: u8 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalSaveDocument {
    format_version: u16,
    application_version: String,
    scenario_schema_version: u16,
    scenario_ron: String,
    core: SaveEnvelope,
    history: Vec<String>,
    selected_scenario: usize,
    session_id: u64,
    north_name: String,
    south_name: String,
    clock: Option<crownline_core::ClockSettings>,
}

#[derive(Resource)]
pub(crate) struct LocalPersistenceStatus {
    pub(crate) slot: u8,
    pub(crate) message: String,
}

impl Default for LocalPersistenceStatus {
    fn default() -> Self {
        Self {
            slot: 1,
            message: "F5 save · F9 load · F6 change slot".to_owned(),
        }
    }
}

pub struct LocalPersistencePlugin;

impl Plugin for LocalPersistencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalPersistenceStatus>()
            .add_systems(Update, handle_save_load_keys);
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn handle_save_load_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut status: ResMut<LocalPersistenceStatus>,
    mut flow: ResMut<ClientFlow>,
    mut setup: ResMut<LocalSetup>,
    mut runtime: ResMut<LocalClockRuntime>,
    mut game: ResMut<DisplayedGame>,
    mut history: ResMut<LocalTransitionNoticeLog>,
    mut events: ResMut<LocalTransitionEventQueue>,
    mut selection: ResMut<OverlaySelection>,
) {
    if keys.just_pressed(KeyCode::F6) {
        status.slot = status.slot % SLOT_COUNT + 1;
        status.message = format!("Selected save slot {}.", status.slot);
    }
    if keys.just_pressed(KeyCode::F5) && !matches!(*flow, ClientFlow::Setup) {
        status.message = match save_slot(status.slot, &game, &setup, &history.entries) {
            Ok(path) => format!("Saved slot {} safely to {}.", status.slot, path.display()),
            Err(error) => format!(
                "Save failed; the previous slot was preserved. {error} Check storage permissions and free space."
            ),
        };
    }
    if keys.just_pressed(KeyCode::F9) {
        match load_slot(status.slot) {
            Ok(document) => {
                game.scenario = ron::from_str(&document.scenario_ron)
                    .expect("decoded save scenario was already validated");
                game.state = document.core.state;
                history.entries = document.history;
                setup.selected_scenario = document.selected_scenario;
                setup.session_id = document.session_id;
                setup.north_name = document.north_name;
                setup.south_name = document.south_name;
                setup.clock = document.clock;
                setup.error.clear();
                runtime.sub_millisecond_nanos = 0;
                selection.piece = None;
                events.clear();
                *flow = if game.state.outcome.is_some() {
                    ClientFlow::Outcome
                } else {
                    ClientFlow::Playing
                };
                status.message = format!(
                    "Loaded slot {}. Offline time was not charged; canonical revision {} restored.",
                    status.slot, game.state.revision
                );
            }
            Err(error) => {
                status.message = format!(
                    "Load failed; the slot file was left unchanged. {error} Try another slot or restore a known-good copy."
                );
            }
        }
    }
}

fn save_slot(
    slot: u8,
    game: &DisplayedGame,
    setup: &LocalSetup,
    history: &[String],
) -> Result<PathBuf, String> {
    let document = LocalSaveDocument {
        format_version: LOCAL_SAVE_FORMAT_VERSION,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        scenario_schema_version: SCENARIO_SCHEMA_VERSION,
        scenario_ron: ron::ser::to_string(&game.scenario).map_err(|error| error.to_string())?,
        core: SaveEnvelope::new(env!("CARGO_PKG_VERSION"), game.state.clone())
            .map_err(|error| error.to_string())?,
        history: history.to_vec(),
        selected_scenario: setup.selected_scenario,
        session_id: setup.session_id,
        north_name: setup.north_name.clone(),
        south_name: setup.south_name.clone(),
        clock: setup.clock,
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    let path = slot_path(slot)?;
    let mut storage = FileSaveStorage::new(path.clone());
    write_bytes_atomically(&mut storage, &bytes, |temporary| {
        decode_document(temporary)
            .map(|_| ())
            .map_err(crownline_core::PersistenceError::MalformedJson)
    })
    .map_err(|error| error.to_string())?;
    Ok(path)
}

fn load_slot(slot: u8) -> Result<LocalSaveDocument, String> {
    let path = slot_path(slot)?;
    let metadata = fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() > u64::try_from(MAX_PERSISTED_BYTES).unwrap() {
        return Err(format!(
            "slot exceeds the {MAX_PERSISTED_BYTES} byte safety limit"
        ));
    }
    let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    decode_document(&bytes)
}

fn decode_document(bytes: &[u8]) -> Result<LocalSaveDocument, String> {
    if bytes.len() > MAX_PERSISTED_BYTES {
        return Err(format!(
            "save exceeds the {MAX_PERSISTED_BYTES} byte safety limit"
        ));
    }
    let document: LocalSaveDocument =
        serde_json::from_slice(bytes).map_err(|error| format!("save JSON is invalid: {error}"))?;
    if document.format_version != LOCAL_SAVE_FORMAT_VERSION {
        return Err(format!(
            "local save format {} is unsupported (current {})",
            document.format_version, LOCAL_SAVE_FORMAT_VERSION
        ));
    }
    if document.application_version.trim().is_empty() {
        return Err("application version is missing".to_owned());
    }
    let scenario: ScenarioDefinition = ron::from_str(&document.scenario_ron)
        .map_err(|error| format!("authored scenario RON is invalid: {error}"))?;
    if document.scenario_schema_version != SCENARIO_SCHEMA_VERSION
        || scenario.schema_version != SCENARIO_SCHEMA_VERSION
    {
        return Err("scenario schema version is unsupported".to_owned());
    }
    scenario
        .validate()
        .map_err(|errors| format!("scenario is invalid: {errors:?}"))?;
    if scenario.id != document.core.scenario_id {
        return Err("authored scenario does not match the canonical save".to_owned());
    }
    let core_bytes = document.core.to_json().map_err(|error| error.to_string())?;
    SaveReader::new()
        .read(&core_bytes)
        .map_err(|error| error.to_string())?;
    if document.north_name.trim().is_empty()
        || document.south_name.trim().is_empty()
        || document.north_name.chars().count() > 24
        || document.south_name.chars().count() > 24
        || document
            .north_name
            .eq_ignore_ascii_case(&document.south_name)
    {
        return Err("saved player names are invalid".to_owned());
    }
    if document.selected_scenario >= 3 {
        return Err("saved scenario selection is invalid".to_owned());
    }
    Ok(document)
}

fn slot_path(slot: u8) -> Result<PathBuf, String> {
    if !(1..=SLOT_COUNT).contains(&slot) {
        return Err(format!("save slot {slot} is outside 1–{SLOT_COUNT}"));
    }
    ProjectDirs::from("org", "Crownlines", "Crownlines")
        .map(|dirs| {
            dirs.data_local_dir()
                .join("saves")
                .join(format!("slot-{slot}.json"))
        })
        .ok_or_else(|| "platform save directory is unavailable".to_owned())
}

struct FileSaveStorage {
    current: PathBuf,
    temporary: PathBuf,
}

impl FileSaveStorage {
    fn new(current: PathBuf) -> Self {
        let temporary = current.with_extension("json.tmp");
        Self { current, temporary }
    }
}

impl AtomicSaveStorage for FileSaveStorage {
    fn write_temporary(&mut self, bytes: &[u8]) -> Result<(), String> {
        let parent = self
            .current
            .parent()
            .ok_or_else(|| "save path has no parent directory".to_owned())?;
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
        is_in_check,
        scenario::{Coord, PieceKind, Player},
        state::{ClockState, MandatoryChoice, MatchOutcome, OutcomeReason, TurnPhase},
    };

    use super::*;

    fn fixture_document() -> LocalSaveDocument {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let state = crownline_core::MatchState::from_scenario(&scenario).unwrap();
        LocalSaveDocument {
            format_version: LOCAL_SAVE_FORMAT_VERSION,
            application_version: "test".to_owned(),
            scenario_schema_version: SCENARIO_SCHEMA_VERSION,
            scenario_ron: ron::ser::to_string(&scenario).unwrap(),
            core: SaveEnvelope::new("test", state).unwrap(),
            history: vec!["Move a to b".to_owned()],
            selected_scenario: 1,
            session_id: 7,
            north_name: "North".to_owned(),
            south_name: "South".to_owned(),
            clock: None,
        }
    }

    #[test]
    fn wrapper_round_trip_preserves_hash_scenario_history_and_versions() {
        let document = fixture_document();
        let bytes = serde_json::to_vec(&document).unwrap();
        let decoded = decode_document(&bytes).unwrap();
        assert_eq!(decoded.core.state_hash, document.core.state_hash);
        assert_eq!(decoded.scenario_ron, document.scenario_ron);
        assert_eq!(decoded.history, document.history);
        assert_eq!(decoded.application_version, "test");
        assert_eq!(decoded.scenario_schema_version, SCENARIO_SCHEMA_VERSION);
    }

    #[test]
    fn pending_choice_clock_draw_and_terminal_variants_decode() {
        let mut document = fixture_document();
        document.core.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::PlacePawn {
                settlement_index: 0,
                legal_squares: [scenario_from_document(&document).settlements[0].at]
                    .into_iter()
                    .collect(),
            }],
        };
        document.core = SaveEnvelope::new("test", document.core.state.clone()).unwrap();
        assert!(decode_document(&serde_json::to_vec(&document).unwrap()).is_ok());

        let mut document = fixture_document();
        let pawn = document
            .core
            .state
            .pieces
            .values()
            .find(|piece| piece.kind == PieceKind::Pawn)
            .unwrap()
            .id;
        document.core.state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::Promote {
                pawn,
                site_index: 0,
            }],
        };
        document.core.state.clocks = Some(ClockState {
            north_millis: 12_345,
            south_millis: 54_321,
            increment_millis: 2_000,
        });
        document.core = SaveEnvelope::new("test", document.core.state.clone()).unwrap();
        assert!(decode_document(&serde_json::to_vec(&document).unwrap()).is_ok());

        let mut document = fixture_document();
        document.core.state.outstanding_draw_offer = Some(Player::North);
        document.core = SaveEnvelope::new("test", document.core.state.clone()).unwrap();
        assert!(decode_document(&serde_json::to_vec(&document).unwrap()).is_ok());

        let mut document = fixture_document();
        document.core.state.outcome = Some(MatchOutcome {
            winner: None,
            reason: OutcomeReason::AgreedDraw,
        });
        document.core = SaveEnvelope::new("test", document.core.state.clone()).unwrap();
        assert!(decode_document(&serde_json::to_vec(&document).unwrap()).is_ok());

        let mut document = fixture_document();
        let active = document.core.state.active_player;
        let king = document
            .core
            .state
            .pieces
            .values()
            .find(|piece| piece.owner == active && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let opposing_king = document
            .core
            .state
            .pieces
            .values()
            .find(|piece| piece.owner != active && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        let opposing_rook = document
            .core
            .state
            .pieces
            .values()
            .find(|piece| piece.owner != active && piece.kind == PieceKind::Rook)
            .unwrap()
            .id;
        document
            .core
            .state
            .pieces
            .retain(|id, _| [king, opposing_king, opposing_rook].contains(id));
        document.core.state.pieces.get_mut(&king).unwrap().at = Coord::new(0, 5);
        document
            .core
            .state
            .pieces
            .get_mut(&opposing_rook)
            .unwrap()
            .at = Coord::new(0, 6);
        document
            .core
            .state
            .pieces
            .get_mut(&opposing_king)
            .unwrap()
            .at = Coord::new(7, 7);
        assert!(
            is_in_check(
                &scenario_from_document(&document),
                &document.core.state,
                active
            )
            .unwrap()
        );
        document.core = SaveEnvelope::new("test", document.core.state.clone()).unwrap();
        assert!(decode_document(&serde_json::to_vec(&document).unwrap()).is_ok());
    }

    fn scenario_from_document(document: &LocalSaveDocument) -> ScenarioDefinition {
        ron::from_str(&document.scenario_ron).unwrap()
    }

    #[test]
    fn corrupt_wrapper_is_recoverable_and_does_not_require_mutation() {
        assert!(decode_document(b"{ truncated").is_err());
        let mut document = fixture_document();
        document.core.state.revision += 1;
        assert!(decode_document(&serde_json::to_vec(&document).unwrap()).is_err());
    }

    #[derive(Default)]
    struct MemoryStorage {
        current: Vec<u8>,
        temporary: Vec<u8>,
        replaced: usize,
    }

    impl AtomicSaveStorage for MemoryStorage {
        fn write_temporary(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.temporary = bytes.to_vec();
            Ok(())
        }

        fn read_temporary(&mut self) -> Result<Vec<u8>, String> {
            Ok(self.temporary.clone())
        }

        fn replace_with_temporary(&mut self) -> Result<(), String> {
            self.current.clone_from(&self.temporary);
            self.replaced += 1;
            Ok(())
        }

        fn discard_temporary(&mut self) {
            self.temporary.clear();
        }
    }

    #[test]
    fn wrapper_validation_happens_before_atomic_replacement() {
        let valid = serde_json::to_vec(&fixture_document()).unwrap();
        let mut storage = MemoryStorage {
            current: b"known good".to_vec(),
            ..default()
        };
        write_bytes_atomically(&mut storage, &valid, |bytes| {
            decode_document(bytes)
                .map(|_| ())
                .map_err(crownline_core::PersistenceError::MalformedJson)
        })
        .unwrap();
        assert_eq!(storage.replaced, 1);
        let known_good = storage.current.clone();

        let result = write_bytes_atomically(&mut storage, b"{broken", |bytes| {
            decode_document(bytes)
                .map(|_| ())
                .map_err(crownline_core::PersistenceError::MalformedJson)
        });
        assert!(result.is_err());
        assert_eq!(storage.current, known_good);
        assert_eq!(storage.replaced, 1);
    }
}
