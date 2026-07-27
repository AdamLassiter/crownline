use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use bevy::prelude::*;
use crownline_ai::{
    AlphaBetaSearch, BaselineEvaluator, CancellationToken, DifficultyConfig, DifficultyProfile,
    RegisteredOpponentPolicy, SearchPolicy, SearchRequest, SearchResult, StableMoveOrderer,
    StopReason, legal_search_actions, registered_opponent_policy,
};
use crownline_core::{
    Action, GuidedAiMode, GuidedContent, MatchState, apply_timed_action,
    scenario::{Player, ScenarioDefinition},
};

use crate::{
    guided_play::GuidedRuntime,
    lifecycle::{ClientFlow, LocalSetup},
    local_interaction::BoardInteraction,
    rendering::{DisplayedGame, LocalTransitionEventQueue, OverlaySelection},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotKey {
    epoch: u64,
    session_id: u64,
    scenario_id: String,
    revision: u64,
    active_player: Player,
    position_key: String,
}

impl SnapshotKey {
    fn new(
        control: &AiCancellationEpoch,
        setup: &LocalSetup,
        game: &DisplayedGame,
    ) -> Result<Self, String> {
        Ok(Self {
            epoch: control.0,
            session_id: setup.session_id,
            scenario_id: game.scenario.id.clone(),
            revision: game.state.revision,
            active_player: game.state.active_player,
            position_key: game
                .state
                .repetition_key()
                .map_err(|error| error.to_string())?,
        })
    }
}

#[derive(Debug)]
struct WorkerResult {
    job_id: u64,
    key: SnapshotKey,
    profile: DifficultyProfile,
    label: String,
    guided: bool,
    elapsed_millis: u64,
    result: Result<SearchResult, String>,
}

#[derive(Debug, Clone)]
enum AiDecision {
    Search(DifficultyConfig),
    FirstLegal,
    ExactReply(Action),
}

#[derive(Debug, Clone)]
struct AiPlan {
    profile: DifficultyProfile,
    label: String,
    guided: bool,
    decision: AiDecision,
}

struct ActiveJob {
    id: u64,
    key: SnapshotKey,
    cancellation: Arc<CancellationToken>,
    handle: Option<JoinHandle<()>>,
    cancelled: bool,
}

#[derive(Resource, Default)]
pub(crate) struct AiCancellationEpoch(pub(crate) u64);

impl AiCancellationEpoch {
    pub(crate) fn cancel_pending(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}

#[derive(Resource)]
struct AiRuntime {
    sender: SyncSender<WorkerResult>,
    receiver: Mutex<Receiver<WorkerResult>>,
    active: Option<ActiveJob>,
    next_job_id: u64,
    failed_key: Option<SnapshotKey>,
}

impl Default for AiRuntime {
    fn default() -> Self {
        let (sender, receiver) = sync_channel(1);
        Self {
            sender,
            receiver: Mutex::new(receiver),
            active: None,
            next_job_id: 1,
            failed_key: None,
        }
    }
}

impl AiRuntime {
    fn cancel(&mut self) {
        if let Some(job) = &self.active {
            job.cancellation.cancel();
        }
        if let Some(job) = &mut self.active {
            job.cancelled = true;
        }
    }

    fn reap(&mut self) {
        if let Some(mut job) = self.active.take()
            && let Some(handle) = job.handle.take()
        {
            let _ = handle.join();
        }
    }
}

impl Drop for AiRuntime {
    fn drop(&mut self) {
        self.cancel();
        self.reap();
    }
}

pub struct LocalAiPlugin;

impl Plugin for LocalAiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiRuntime>()
            .init_resource::<AiCancellationEpoch>()
            .add_systems(
                Update,
                drive_local_ai
                    .after(crate::lifecycle::LifecycleInputSet)
                    .after(crate::guided_play::GuidedInputSet),
            );
    }
}

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn drive_local_ai(
    keys: Res<ButtonInput<KeyCode>>,
    flow: Res<ClientFlow>,
    setup: Res<LocalSetup>,
    control: Res<AiCancellationEpoch>,
    mut game: ResMut<DisplayedGame>,
    mut runtime: ResMut<AiRuntime>,
    mut transitions: ResMut<LocalTransitionEventQueue>,
    mut selection: ResMut<OverlaySelection>,
    mut interaction: ResMut<BoardInteraction>,
    guided: Option<Res<GuidedRuntime>>,
) {
    let current_key = SnapshotKey::new(&control, &setup, &game).ok();
    let plan = resolve_ai_plan(&setup, guided.as_deref(), &game);
    let eligible = *flow == ClientFlow::Playing
        && game.state.outcome.is_none()
        && game.scenario.rules.fog.is_none()
        && plan.as_ref().is_ok_and(Option::is_some);

    if runtime
        .active
        .as_ref()
        .is_some_and(|job| !eligible || current_key.as_ref() != Some(&job.key))
    {
        runtime.cancel();
    }

    let message = runtime
        .receiver
        .lock()
        .expect("AI result receiver lock poisoned")
        .try_recv();
    match message {
        Ok(message) => {
            let matching_job = result_matches_job(runtime.active.as_ref(), &message);
            runtime.reap();
            if matching_job && eligible && current_key.as_ref() == Some(&message.key) {
                handle_result(
                    message,
                    &mut runtime,
                    &mut game,
                    &mut transitions,
                    &mut selection,
                    &mut interaction,
                );
            }
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            runtime.cancel();
            runtime.reap();
            if let Some(key) = current_key.clone() {
                runtime.failed_key = Some(key);
            }
            interaction.set_status("AI worker disconnected. Press R to retry or F to forfeit.");
        }
    }

    if keys.just_pressed(KeyCode::KeyR) && runtime.failed_key.as_ref() == current_key.as_ref() {
        runtime.failed_key = None;
    }
    if keys.just_pressed(KeyCode::KeyF)
        && runtime.failed_key.as_ref() == current_key.as_ref()
        && let Ok(transition) = apply_timed_action(
            &game.scenario,
            &game.state,
            &Action::Resign {
                player: game.state.active_player,
            },
            0,
        )
    {
        transitions.push_local_action(
            &Action::Resign {
                player: game.state.active_player,
            },
            &transition,
        );
        game.state = transition.state;
        runtime.failed_key = None;
        return;
    }

    if eligible && runtime.active.is_none() && runtime.failed_key.as_ref() != current_key.as_ref() {
        if let Some(key) = current_key {
            let plan = plan
                .expect("eligible AI plan is valid")
                .expect("eligible AI plan is present");
            let thinking = thinking_message(&plan);
            spawn_decision(&mut runtime, key, plan, &game.scenario, &game.state);
            interaction.set_status(thinking);
        }
    } else if let Err(error) = plan
        && runtime.active.is_none()
    {
        interaction.set_status(error);
    }
}

fn result_matches_job(active: Option<&ActiveJob>, message: &WorkerResult) -> bool {
    active.is_some_and(|job| job.id == message.job_id && job.key == message.key && !job.cancelled)
}

pub(crate) fn validate_guided_ai_content(guided: &GuidedContent) -> Result<(), String> {
    let Some(ai) = &guided.ai else {
        return Ok(());
    };
    match &ai.mode {
        GuidedAiMode::GeneralProfile { profile_id } => DifficultyProfile::from_id(profile_id)
            .map(|_| ())
            .ok_or_else(|| format!("unknown registered AI profile {profile_id:?}")),
        GuidedAiMode::RegisteredPolicy { policy_id } => registered_opponent_policy(policy_id)
            .map(|_| ())
            .ok_or_else(|| format!("unknown registered AI policy {policy_id:?}")),
        GuidedAiMode::ReplyTree { root_node_id } => guided
            .reply_nodes
            .iter()
            .any(|node| &node.id == root_node_id)
            .then_some(())
            .ok_or_else(|| "guided reply tree has no registered root".to_owned()),
    }
}

fn resolve_ai_plan(
    setup: &LocalSetup,
    guided: Option<&GuidedRuntime>,
    game: &DisplayedGame,
) -> Result<Option<AiPlan>, String> {
    if let Some(guided_runtime) = guided
        && guided_runtime.is_active()
    {
        let Some((config, actions_taken)) = guided_runtime.ai_configuration(game) else {
            return Ok(None);
        };
        if config.seat != game.state.active_player {
            return Ok(None);
        }
        if config
            .max_actions
            .is_some_and(|limit| actions_taken >= limit)
        {
            return Err(
                "Guided opponent reached its authored action bound. Retry the lesson or leave."
                    .to_owned(),
            );
        }
        return resolve_guided_plan(game, &config.mode);
    }
    Ok(setup
        .controller(game.state.active_player)
        .profile()
        .map(|profile| AiPlan {
            profile,
            label: format!("AI {profile:?}"),
            guided: false,
            decision: AiDecision::Search(DifficultyConfig::for_profile(profile)),
        }))
}

fn resolve_guided_plan(
    game: &DisplayedGame,
    mode: &GuidedAiMode,
) -> Result<Option<AiPlan>, String> {
    let plan = match mode {
        GuidedAiMode::GeneralProfile { profile_id } => {
            let profile = DifficultyProfile::from_id(profile_id).ok_or_else(|| {
                format!("Unknown guided AI profile {profile_id:?}; content cannot start.")
            })?;
            AiPlan {
                profile,
                label: format!("guided profile {profile_id}"),
                guided: true,
                decision: AiDecision::Search(DifficultyConfig::for_profile(profile)),
            }
        }
        GuidedAiMode::RegisteredPolicy { policy_id } => {
            let policy = registered_opponent_policy(policy_id).ok_or_else(|| {
                format!("Unknown guided AI policy {policy_id:?}; content cannot start.")
            })?;
            match policy {
                RegisteredOpponentPolicy::Search(config) => AiPlan {
                    profile: config.profile,
                    label: format!("registered policy {policy_id}"),
                    guided: true,
                    decision: AiDecision::Search(config),
                },
                RegisteredOpponentPolicy::FirstLegal => AiPlan {
                    profile: DifficultyProfile::Apprentice,
                    label: format!("registered policy {policy_id}"),
                    guided: true,
                    decision: AiDecision::FirstLegal,
                },
            }
        }
        GuidedAiMode::ReplyTree { .. } => {
            let key = game
                .state
                .repetition_key()
                .map_err(|error| error.to_string())?;
            let guided = game
                .scenario
                .guided
                .as_ref()
                .ok_or_else(|| "guided AI lost its scenario content".to_owned())?;
            let node = guided
                .reply_nodes
                .iter()
                .find(|node| node.position_key == key)
                .ok_or_else(|| {
                    "No authored reply covers this canonical position. Retry or report content drift."
                        .to_owned()
                })?;
            AiPlan {
                profile: DifficultyProfile::Apprentice,
                label: format!("reply node {}", node.id),
                guided: true,
                decision: AiDecision::ExactReply(node.action.clone()),
            }
        }
    };
    Ok(Some(plan))
}

fn thinking_message(plan: &AiPlan) -> String {
    if plan.guided {
        return "Guided opponent thinking with a bounded registered policy. P pauses and cancels."
            .to_owned();
    }
    let AiDecision::Search(config) = plan.decision else {
        return "AI thinking. P pauses and cancels.".to_owned();
    };
    format!(
        "{} thinking: depth <= {}, nodes <= {}. P pauses and cancels.",
        plan.label, config.max_depth, config.max_nodes
    )
}

fn spawn_decision(
    runtime: &mut AiRuntime,
    key: SnapshotKey,
    plan: AiPlan,
    scenario: &ScenarioDefinition,
    state: &MatchState,
) {
    let job_id = runtime.next_job_id;
    runtime.next_job_id = runtime.next_job_id.saturating_add(1);
    let cancellation = Arc::new(CancellationToken::default());
    let worker_token = Arc::clone(&cancellation);
    let sender = runtime.sender.clone();
    let scenario = scenario.clone();
    let state = state.clone();
    let worker_key = key.clone();
    let handle = thread::Builder::new()
        .name(format!("crownline-ai-{job_id}"))
        .spawn(move || {
            let started = Instant::now();
            let result = decide(
                &scenario,
                &state,
                &plan.decision,
                worker_token.as_ref(),
                started,
            );
            let elapsed_millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let _ = sender.try_send(WorkerResult {
                job_id,
                key: worker_key,
                profile: plan.profile,
                label: plan.label,
                guided: plan.guided,
                elapsed_millis,
                result,
            });
        })
        .expect("failed to spawn bounded local AI worker");
    runtime.active = Some(ActiveJob {
        id: job_id,
        key,
        cancellation,
        handle: Some(handle),
        cancelled: false,
    });
}

fn decide(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    decision: &AiDecision,
    cancellation: &CancellationToken,
    started: Instant,
) -> Result<SearchResult, String> {
    match decision {
        AiDecision::Search(config) => {
            let evaluator = BaselineEvaluator::new(config.evaluation);
            AlphaBetaSearch
                .search(SearchRequest {
                    scenario,
                    state,
                    root: state.active_player,
                    evaluator: &evaluator,
                    orderer: &StableMoveOrderer,
                    limits: config.search_limits(started),
                    cancellation,
                })
                .map_err(|error| error.to_string())
        }
        AiDecision::FirstLegal => legal_search_actions(scenario, state)
            .map_err(|error| error.to_string())
            .map(|actions| synthetic_result(actions.into_iter().next())),
        AiDecision::ExactReply(action) => Ok(synthetic_result(Some(action.clone()))),
    }
}

fn synthetic_result(action: Option<Action>) -> SearchResult {
    SearchResult {
        stop_reason: if action.is_some() {
            StopReason::Completed
        } else {
            StopReason::NoLegalAction
        },
        action,
        score: 0,
        completed_depth: 0,
        principal_variation: Vec::new(),
        nodes: 0,
        quiescence_nodes: 0,
        cutoffs: 0,
        tie_break: None,
    }
}

fn handle_result(
    message: WorkerResult,
    runtime: &mut AiRuntime,
    game: &mut DisplayedGame,
    transitions: &mut LocalTransitionEventQueue,
    selection: &mut OverlaySelection,
    interaction: &mut BoardInteraction,
) {
    let Ok(result) = message.result else {
        runtime.failed_key = Some(message.key);
        interaction.set_status("AI search failed. Press R to retry or F to forfeit.");
        return;
    };
    let Some(action) = result.action else {
        runtime.failed_key = Some(message.key);
        interaction.set_status(format!(
            "{} found no complete action ({:?}). Press R to retry or F to forfeit.",
            message.label, result.stop_reason
        ));
        return;
    };
    match apply_timed_action(&game.scenario, &game.state, &action, 0) {
        Ok(transition) => {
            transitions.push_local_action(&action, &transition);
            game.state = transition.state;
            selection.piece = None;
            if message.guided {
                debug!(
                    target: "crownline::guided_ai",
                    policy = %message.label,
                    profile = ?message.profile,
                    depth = result.completed_depth,
                    nodes = result.nodes,
                    quiescence_nodes = result.quiescence_nodes,
                    elapsed_millis = message.elapsed_millis,
                    stop = ?result.stop_reason,
                    "guided AI decision accepted"
                );
                interaction.set_status("Guided opponent completed its registered reply.");
            } else {
                interaction.set_status(format!(
                    "AI {:?} completed depth {} in {} ms ({} + {} tactical nodes).",
                    message.profile,
                    result.completed_depth,
                    message.elapsed_millis,
                    result.nodes,
                    result.quiescence_nodes
                ));
            }
        }
        Err(error) => {
            runtime.failed_key = Some(message.key);
            interaction.set_status(format!(
                "AI result was rejected safely: {error}. Press R to retry or F to forfeit."
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crownline_ai::{SearchLimits, StopReason, legal_search_actions};
    use crownline_core::{
        ActionJournal, GUIDED_SCHEMA_VERSION, GuidedAiConfig, GuidedAiMode, GuidedContent,
        GuidedEventPredicate, GuidedKind, GuidedPredicate, GuidedReplyNode, GuidedStage,
        GuidedStart, scenario::ScenarioDefinition,
    };

    use super::*;

    fn standard_game() -> DisplayedGame {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../assets/scenarios/standard.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        DisplayedGame { scenario, state }
    }

    fn setup() -> LocalSetup {
        LocalSetup {
            south_controller: crate::lifecycle::SeatController::Ai(DifficultyProfile::Apprentice),
            ..LocalSetup::default()
        }
    }

    fn guided_content(state: MatchState, mode: GuidedAiMode) -> GuidedContent {
        let ai_seat = state.active_player;
        GuidedContent {
            schema_version: GUIDED_SCHEMA_VERSION,
            id: "guided.ai.test".to_owned(),
            kind: GuidedKind::Tutorial,
            category_key: "guided.test.category".to_owned(),
            start: GuidedStart {
                state,
                human_seat: ai_seat.opponent(),
                allow_clock: false,
                allow_controller_changes: false,
            },
            stages: vec![GuidedStage {
                id: "reply".to_owned(),
                title_key: "guided.test.title".to_owned(),
                explanation_key: "guided.test.explanation".to_owned(),
                hint_keys: Vec::new(),
                prerequisites: Vec::new(),
                success: vec![GuidedPredicate::Event(GuidedEventPredicate::Move {
                    piece: None,
                })],
                failure: Vec::new(),
                action_limit: Some(2),
                turn_limit: Some(2),
            }],
            ai: Some(GuidedAiConfig {
                seat: ai_seat,
                mode,
                max_actions: Some(2),
            }),
            completion: None,
            reply_nodes: Vec::new(),
        }
    }

    #[test]
    fn matching_result_submits_exactly_one_canonical_action() {
        let mut game = standard_game();
        let setup = setup();
        let key = SnapshotKey::new(&AiCancellationEpoch::default(), &setup, &game).unwrap();
        let action = legal_search_actions(&game.scenario, &game.state)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let message = WorkerResult {
            job_id: 1,
            key,
            profile: DifficultyProfile::Apprentice,
            label: "AI Apprentice".to_owned(),
            guided: false,
            elapsed_millis: 3,
            result: Ok(SearchResult {
                action: Some(action.clone()),
                score: 0,
                completed_depth: 1,
                principal_variation: vec![action],
                nodes: 2,
                quiescence_nodes: 1,
                cutoffs: 0,
                stop_reason: StopReason::DepthLimit,
                tie_break: None,
            }),
        };
        let before = game.state.revision;
        handle_result(
            message,
            &mut AiRuntime::default(),
            &mut game,
            &mut LocalTransitionEventQueue::default(),
            &mut OverlaySelection::default(),
            &mut BoardInteraction::default(),
        );
        assert_eq!(game.state.revision, before + 1);
    }

    #[test]
    fn guided_profiles_and_policies_resolve_only_registered_ids() {
        let game = standard_game();
        let general = guided_content(
            game.state.clone(),
            GuidedAiMode::GeneralProfile {
                profile_id: "warden".to_owned(),
            },
        );
        validate_guided_ai_content(&general).unwrap();
        let plan = resolve_guided_plan(&game, &general.ai.as_ref().unwrap().mode)
            .unwrap()
            .unwrap();
        assert_eq!(plan.profile, DifficultyProfile::Warden);

        let registered = guided_content(
            game.state.clone(),
            GuidedAiMode::RegisteredPolicy {
                policy_id: "teaching_first_legal".to_owned(),
            },
        );
        validate_guided_ai_content(&registered).unwrap();
        let plan = resolve_guided_plan(&game, &registered.ai.as_ref().unwrap().mode)
            .unwrap()
            .unwrap();
        assert!(matches!(plan.decision, AiDecision::FirstLegal));
        let result = decide(
            &game.scenario,
            &game.state,
            &plan.decision,
            &CancellationToken::default(),
            Instant::now(),
        )
        .unwrap();
        assert_eq!(
            result.action,
            legal_search_actions(&game.scenario, &game.state)
                .unwrap()
                .into_iter()
                .next()
        );

        let unknown = guided_content(
            game.state.clone(),
            GuidedAiMode::RegisteredPolicy {
                policy_id: "embedded-arbitrary-weights".to_owned(),
            },
        );
        assert!(validate_guided_ai_content(&unknown).is_err());
    }

    #[test]
    fn exact_reply_uses_the_action_pinned_to_the_current_position() {
        let mut game = standard_game();
        let action = legal_search_actions(&game.scenario, &game.state)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let key = game.state.repetition_key().unwrap();
        let mut guided = guided_content(
            game.state.clone(),
            GuidedAiMode::ReplyTree {
                root_node_id: "root".to_owned(),
            },
        );
        guided.reply_nodes.push(GuidedReplyNode {
            id: "root".to_owned(),
            state: game.state.clone(),
            position_key: key,
            action: action.clone(),
            child_ids: Vec::new(),
        });
        game.scenario.guided = Some(guided.clone());
        game.scenario.validate().unwrap();
        let plan = resolve_guided_plan(&game, &guided.ai.as_ref().unwrap().mode)
            .unwrap()
            .unwrap();
        assert!(matches!(plan.decision, AiDecision::ExactReply(_)));
        assert_eq!(
            decide(
                &game.scenario,
                &game.state,
                &plan.decision,
                &CancellationToken::default(),
                Instant::now(),
            )
            .unwrap()
            .action,
            Some(action)
        );
    }

    #[test]
    fn cancelled_job_cannot_match_even_when_position_identity_is_unchanged() {
        let game = standard_game();
        let setup = setup();
        let key = SnapshotKey::new(&AiCancellationEpoch::default(), &setup, &game).unwrap();
        let token = Arc::new(CancellationToken::default());
        let mut runtime = AiRuntime::default();
        runtime.active = Some(ActiveJob {
            id: 7,
            key: key.clone(),
            cancellation: token,
            handle: None,
            cancelled: false,
        });
        runtime.cancel();
        assert!(runtime.active.as_ref().unwrap().cancelled);
        let message = WorkerResult {
            job_id: 7,
            key,
            profile: DifficultyProfile::Apprentice,
            label: "AI Apprentice".to_owned(),
            guided: false,
            elapsed_millis: 0,
            result: Err("cancelled".to_owned()),
        };
        assert!(!result_matches_job(runtime.active.as_ref(), &message));
    }

    #[test]
    fn revision_and_epoch_changes_make_worker_results_stale() {
        let game = standard_game();
        let setup = setup();
        let key = SnapshotKey::new(&AiCancellationEpoch::default(), &setup, &game).unwrap();
        let job = ActiveJob {
            id: 3,
            key: key.clone(),
            cancellation: Arc::new(CancellationToken::default()),
            handle: None,
            cancelled: false,
        };
        let result = |key| WorkerResult {
            job_id: 3,
            key,
            profile: DifficultyProfile::Apprentice,
            label: "AI Apprentice".to_owned(),
            guided: false,
            elapsed_millis: 0,
            result: Err("fixture".to_owned()),
        };
        let mut stale_revision = key.clone();
        stale_revision.revision += 1;
        assert!(!result_matches_job(Some(&job), &result(stale_revision)));
        let mut stale_epoch = key;
        stale_epoch.epoch += 1;
        assert!(!result_matches_job(Some(&job), &result(stale_epoch)));
    }

    #[test]
    fn worker_returns_for_mandatory_choice_without_blocking_caller() {
        let scenario: ScenarioDefinition = ron::from_str(include_str!(
            "../crates/crownline_core/tests/fixtures/scenarios/combined-realms.ron"
        ))
        .unwrap();
        let mut journal: ActionJournal = serde_json::from_str(include_str!(
            "../crates/crownline_core/tests/fixtures/replays/combined-realms.json"
        ))
        .unwrap();
        journal.records.truncate(4);
        let state = journal.replay(&scenario).unwrap();
        let game = DisplayedGame { scenario, state };
        let setup = setup();
        let key = SnapshotKey::new(&AiCancellationEpoch::default(), &setup, &game).unwrap();
        let mut runtime = AiRuntime::default();
        spawn_decision(
            &mut runtime,
            key,
            AiPlan {
                profile: DifficultyProfile::Apprentice,
                label: "AI Apprentice".to_owned(),
                guided: false,
                decision: AiDecision::Search(DifficultyConfig::for_profile(
                    DifficultyProfile::Apprentice,
                )),
            },
            &game.scenario,
            &game.state,
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let message = loop {
            match runtime.receiver.lock().unwrap().try_recv() {
                Ok(message) => break message,
                Err(TryRecvError::Empty) if Instant::now() < deadline => thread::yield_now(),
                error => panic!("AI worker did not return: {error:?}"),
            }
        };
        runtime.reap();
        assert!(matches!(
            message.result.unwrap().action,
            Some(Action::ChoosePromotion { .. })
        ));
    }

    #[test]
    fn developer_ai_smoke_finds_and_applies_terminal_move() {
        let scenario: ScenarioDefinition = ron::from_str(include_str!(
            "../crates/crownline_core/tests/fixtures/scenarios/checkmate.ron"
        ))
        .unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let config = DifficultyConfig::for_profile(DifficultyProfile::Apprentice);
        let result = AlphaBetaSearch
            .search(SearchRequest {
                scenario: &scenario,
                state: &state,
                root: state.active_player,
                evaluator: &BaselineEvaluator::new(config.evaluation),
                orderer: &StableMoveOrderer,
                limits: SearchLimits {
                    deadline: None,
                    ..config.search_limits(Instant::now())
                },
                cancellation: &CancellationToken::default(),
            })
            .unwrap();
        let transition = apply_timed_action(&scenario, &state, &result.action.unwrap(), 0).unwrap();
        assert!(transition.state.outcome.is_some());
    }
}
