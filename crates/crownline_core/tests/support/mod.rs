use crownline_core::{
    Action, MatchState, TransitionEvent, apply_action, is_in_check, legal_mandatory_choice_actions,
    legal_moves,
    rules::MoveKind,
    scenario::{Coord, PieceKind, Player, ScenarioDefinition},
    state::{MandatoryChoice, OutcomeReason, TurnPhase},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_PROBE_PLIES: u32 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flank {
    West,
    East,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideTurns {
    pub north: Option<u64>,
    pub south: Option<u64>,
}

impl SideTurns {
    fn set_once(&mut self, player: Player, turn: u64) {
        let slot = match player {
            Player::North => &mut self.north,
            Player::South => &mut self.south,
        };
        slot.get_or_insert(turn);
    }
}

#[derive(Debug, Serialize)]
struct ProbeStep<'a> {
    pub ply: u32,
    pub turn: u64,
    pub player: Player,
    pub action: &'a Action,
    pub events: &'a [TransitionEvent],
    pub state_hash: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningProbe {
    pub scenario_id: String,
    pub scenario_hash: String,
    pub flank: Flank,
    pub north_target_settlement: String,
    pub south_target_settlement: String,
    pub first_crossing_turn: SideTurns,
    pub first_claim_turn: SideTurns,
    pub first_contact_turn: Option<u64>,
    pub immediate_mate_moves: u32,
    pub completed_plies: u32,
    pub final_turn: u64,
    pub outcome: Option<OutcomeReason>,
    pub final_state_hash: String,
    pub trace_digest: String,
}

pub fn shipped_scenarios() -> Vec<ScenarioDefinition> {
    [
        include_str!("../../../../assets/scenarios/introductory.ron"),
        include_str!("../../../../assets/scenarios/standard.ron"),
        include_str!("../../../../assets/scenarios/large.ron"),
    ]
    .into_iter()
    .map(|source| ron::from_str(source).expect("shipped scenario must decode"))
    .collect()
}

pub fn all_opening_probes() -> Vec<OpeningProbe> {
    shipped_scenarios()
        .iter()
        .flat_map(|scenario| {
            [Flank::West, Flank::East]
                .into_iter()
                .map(|flank| run_opening_probe(scenario, flank))
        })
        .collect()
}

pub fn run_opening_probe(scenario: &ScenarioDefinition, flank: Flank) -> OpeningProbe {
    let north_target = target_settlement(scenario, Player::North, flank);
    let south_target = target_settlement(scenario, Player::South, flank);
    let mut state = MatchState::from_scenario(scenario).expect("validated scenario state");
    let immediate_mate_moves = legal_moves(scenario, &state)
        .expect("initial moves")
        .into_iter()
        .filter(|candidate| {
            apply_action(
                scenario,
                &state,
                &Action::Move {
                    player: state.active_player,
                    piece: candidate.piece,
                    to: candidate.to,
                },
            )
            .is_ok_and(|transition| {
                transition
                    .state
                    .outcome
                    .is_some_and(|outcome| outcome.reason == OutcomeReason::Checkmate)
            })
        })
        .count()
        .try_into()
        .expect("initial move count fits u32");
    let mut probe = OpeningProbe {
        scenario_id: scenario.id.clone(),
        scenario_hash: scenario.canonical_hash().expect("validated scenario hash"),
        flank,
        north_target_settlement: north_target.id.clone(),
        south_target_settlement: south_target.id.clone(),
        first_crossing_turn: SideTurns::default(),
        first_claim_turn: SideTurns::default(),
        first_contact_turn: None,
        immediate_mate_moves,
        completed_plies: 0,
        final_turn: state.turn_number,
        outcome: None,
        final_state_hash: state.canonical_hash().expect("initial state hash"),
        trace_digest: String::new(),
    };
    let mut trace_hasher = Sha256::new();

    for ply in 1..=MAX_PROBE_PLIES {
        if state.outcome.is_some() {
            break;
        }
        let player = state.active_player;
        let turn = state.turn_number;
        let target = match player {
            Player::North => north_target.at,
            Player::South => south_target.at,
        };
        let action = choose_action(scenario, &state, target);
        let transition = apply_action(scenario, &state, &action).expect("probe action is legal");
        collect_milestones(
            &mut probe,
            scenario,
            player,
            turn,
            &transition.events,
            &transition.state,
        );
        state = transition.state;
        let state_hash = state.canonical_hash().expect("probe state hash");
        let step = ProbeStep {
            ply,
            turn,
            player,
            action: &action,
            events: &transition.events,
            state_hash: &state_hash,
        };
        trace_hasher.update(serde_json::to_vec(&step).expect("probe step must serialize"));
        probe.completed_plies = ply;
        if probe.first_crossing_turn.north.is_some()
            && probe.first_crossing_turn.south.is_some()
            && probe.first_contact_turn.is_some()
        {
            break;
        }
    }
    probe.final_turn = state.turn_number;
    probe.outcome = state.outcome.map(|outcome| outcome.reason);
    probe.final_state_hash = state.canonical_hash().expect("final probe state hash");
    probe.trace_digest = format!("{:x}", trace_hasher.finalize());
    probe
}

fn target_settlement(
    scenario: &ScenarioDefinition,
    player: Player,
    flank: Flank,
) -> &crownline_core::scenario::SettlementSite {
    let midpoint = scenario.board.height / 2;
    let mut candidates = scenario
        .settlements
        .iter()
        .filter(|settlement| match player {
            Player::North => settlement.at.y >= midpoint,
            Player::South => settlement.at.y < midpoint,
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|settlement| match flank {
        Flank::West => (settlement.at.x, settlement.at.y.abs_diff(midpoint)),
        Flank::East => (
            u16::MAX.saturating_sub(settlement.at.x),
            settlement.at.y.abs_diff(midpoint),
        ),
    });
    candidates
        .into_iter()
        .next()
        .expect("each side has an opposing-half settlement")
}

fn choose_action(scenario: &ScenarioDefinition, state: &MatchState, target: Coord) -> Action {
    if let TurnPhase::ResolvingChoices { queue } = &state.phase {
        return match queue.first().expect("choice queue is non-empty") {
            MandatoryChoice::Promote { .. } => legal_mandatory_choice_actions(state)
                .into_iter()
                .next()
                .expect("promotion has at least the Knight action"),
            MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares,
            } => Action::PlacePawn {
                player: state.active_player,
                settlement_index: *settlement_index,
                at: *legal_squares
                    .iter()
                    .min_by_key(|at| distance(**at, target))
                    .expect("placement choice has a legal square"),
            },
        };
    }
    legal_moves(scenario, state)
        .expect("probe legal moves")
        .into_iter()
        .max_by_key(|candidate| {
            let piece = &state.pieces[&candidate.piece];
            let pawn_priority = u32::from(piece.kind == PieceKind::Pawn) * 100_000;
            let target_bonus = u32::from(candidate.to == target) * 1_000_000;
            let capture_bonus = u32::from(matches!(
                candidate.kind,
                MoveKind::Capture { .. } | MoveKind::EnPassant { .. }
            )) * 500_000;
            let progress = 10_000_u32.saturating_sub(distance(candidate.to, target));
            (
                target_bonus + capture_bonus + pawn_priority + progress,
                candidate.to,
                candidate.piece,
            )
        })
        .map_or(
            Action::Hold {
                player: state.active_player,
            },
            |candidate| Action::Move {
                player: state.active_player,
                piece: candidate.piece,
                to: candidate.to,
            },
        )
}

fn collect_milestones(
    probe: &mut OpeningProbe,
    scenario: &ScenarioDefinition,
    player: Player,
    turn: u64,
    events: &[TransitionEvent],
    state: &MatchState,
) {
    let midpoint = scenario.board.height / 2;
    for event in events {
        match *event {
            TransitionEvent::PieceMoved { from, to, .. }
                if (from.y < midpoint && to.y >= midpoint)
                    || (from.y >= midpoint && to.y < midpoint) =>
            {
                probe.first_crossing_turn.set_once(player, turn);
            }
            TransitionEvent::SettlementClaimed { owner, .. }
            | TransitionEvent::SettlementTransferred { owner, .. } => {
                probe.first_claim_turn.set_once(owner, turn);
                probe.first_contact_turn.get_or_insert(turn);
            }
            TransitionEvent::PieceCaptured { .. } | TransitionEvent::SettlementContested { .. } => {
                probe.first_contact_turn.get_or_insert(turn);
            }
            _ => {}
        }
    }
    if state.outcome.is_none() && is_in_check(scenario, state, state.active_player).unwrap_or(false)
    {
        probe.first_contact_turn.get_or_insert(turn);
    }
}

fn distance(first: Coord, second: Coord) -> u32 {
    u32::from(first.x.abs_diff(second.x)) + u32::from(first.y.abs_diff(second.y))
}
