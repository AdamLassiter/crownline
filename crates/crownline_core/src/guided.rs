//! Bounded declarative lesson and challenge content layered over canonical play.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    inspect_move, is_in_check,
    rules::{MoveInspection, TransitionEvent, governance_report},
    scenario::{Coord, Edge, EdgeKind, PieceKind, Player, ScenarioDefinition, TileTerrain},
    state::{Action, MatchOutcome, MatchState, OutcomeReason, PieceId, PieceOrigin, TurnPhase},
};

pub const GUIDED_SCHEMA_VERSION: u16 = 1;
const MAX_STAGES: usize = 64;
const MAX_PREDICATES: usize = 24;
const MAX_HINTS: usize = 8;
const MAX_KEY_CHARS: usize = 128;
const MAX_REPLY_NODES: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedKind {
    Tutorial,
    Challenge,
    OpenPractice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedContent {
    pub schema_version: u16,
    pub id: String,
    pub kind: GuidedKind,
    pub category_key: String,
    pub start: GuidedStart,
    pub stages: Vec<GuidedStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<GuidedAiConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<GuidedCompletion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reply_nodes: Vec<GuidedReplyNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedStart {
    pub state: MatchState,
    pub human_seat: Player,
    #[serde(default)]
    pub allow_clock: bool,
    #[serde(default)]
    pub allow_controller_changes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedStage {
    pub id: String,
    pub title_key: String,
    pub explanation_key: String,
    #[serde(default)]
    pub hint_keys: Vec<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    pub success: Vec<GuidedPredicate>,
    #[serde(default)]
    pub failure: Vec<GuidedPredicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_limit: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_limit: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedCompletion {
    pub completion_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_guided_id: Option<String>,
    #[serde(default)]
    pub records_best_actions: bool,
    #[serde(default)]
    pub records_best_turns: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedAiConfig {
    pub seat: Player,
    pub mode: GuidedAiMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_actions: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedAiMode {
    GeneralProfile { profile_id: String },
    RegisteredPolicy { policy_id: String },
    ReplyTree { root_node_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidedReplyNode {
    pub id: String,
    pub position_key: String,
    pub action: Action,
    #[serde(default)]
    pub child_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedPredicate {
    LegalMove {
        player: Player,
        piece: PieceId,
        to: Coord,
    },
    PieceAt {
        player: Player,
        kind: PieceKind,
        at: Coord,
    },
    PieceSurvives {
        piece: PieceId,
    },
    PieceOnTerrain {
        piece: PieceId,
        terrain: TileTerrain,
    },
    MaterialAtLeast {
        player: Player,
        kind: PieceKind,
        count: u16,
    },
    InCheck {
        player: Player,
        expected: bool,
    },
    TurnPhase {
        phase: GuidedTurnPhase,
    },
    SettlementOwned {
        settlement_index: u16,
        player: Player,
    },
    SettlementGoverned {
        settlement_index: u16,
        player: Player,
    },
    SettlementEstablished {
        settlement_index: u16,
        expected: bool,
    },
    SettlementProducedPawn {
        settlement_index: u16,
        expected: bool,
    },
    Outcome {
        winner: Option<Player>,
        reason: OutcomeReason,
    },
    Event(GuidedEventPredicate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedTurnPhase {
    Command,
    MandatoryChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidedEventPredicate {
    Move {
        piece: Option<PieceId>,
    },
    Capture {
        piece: Option<PieceId>,
    },
    CrossEdge {
        piece: Option<PieceId>,
        kind: EdgeKind,
    },
    EnterTerrain {
        piece: Option<PieceId>,
        terrain: TileTerrain,
    },
    SettlementClaimed {
        settlement_index: Option<u16>,
    },
    SettlementEstablished {
        settlement_index: Option<u16>,
    },
    PawnProduced {
        settlement_index: Option<u16>,
    },
    SettlementTransferred {
        settlement_index: Option<u16>,
    },
    Promotion {
        pawn: Option<PieceId>,
        kind: Option<PieceKind>,
    },
    MatchEnded,
}

pub struct GuidedPredicateContext<'a> {
    pub scenario: &'a ScenarioDefinition,
    pub state: &'a MatchState,
    pub events: &'a [TransitionEvent],
    pub actions_taken: u16,
    pub turns_elapsed: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveResult {
    InProgress,
    Succeeded,
    Failed,
}

impl GuidedContent {
    /// Validates bounded references, predicates, start state, and reply metadata.
    ///
    /// # Errors
    ///
    /// Returns a readable authoring error for the first invalid declaration.
    pub fn validate(&self, scenario: &ScenarioDefinition) -> Result<(), String> {
        if self.schema_version != GUIDED_SCHEMA_VERSION {
            return Err(format!(
                "guided schema {} is unsupported; expected {GUIDED_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        validate_key("guided id", &self.id)?;
        validate_key("category key", &self.category_key)?;
        self.start.validate(scenario)?;
        if self.stages.is_empty() || self.stages.len() > MAX_STAGES {
            return Err(format!(
                "guided content must contain 1..={MAX_STAGES} stages"
            ));
        }
        let ids: BTreeSet<_> = self.stages.iter().map(|stage| stage.id.as_str()).collect();
        if ids.len() != self.stages.len() {
            return Err("stage ids must be unique".to_owned());
        }
        for stage in &self.stages {
            stage.validate(scenario, &ids)?;
        }
        validate_acyclic(&self.stages)?;
        if let Some(ai) = &self.ai {
            ai.validate(&ids, &self.reply_nodes)?;
        }
        if let Some(completion) = &self.completion {
            validate_key("completion key", &completion.completion_key)?;
            if let Some(next) = &completion.next_guided_id {
                validate_key("next guided id", next)?;
            }
        }
        validate_reply_nodes(&self.reply_nodes)?;
        Ok(())
    }
}

impl GuidedStart {
    fn validate(&self, scenario: &ScenarioDefinition) -> Result<(), String> {
        if self.state.scenario_id != scenario.id {
            return Err("guided start scenario id does not match its scenario".to_owned());
        }
        if self.state.outcome.is_some() {
            return Err("guided start cannot already be terminal".to_owned());
        }
        self.state
            .validate_invariants()
            .map_err(|error| error.to_string())?;
        for piece in self.state.pieces.values() {
            if !piece.at.is_within(scenario.board) {
                return Err(format!("guided piece {:?} is outside the board", piece.id));
            }
            if scenario.terrain.get(&piece.at) == Some(&TileTerrain::Mountain) {
                return Err(format!("guided piece {:?} occupies a mountain", piece.id));
            }
        }
        if self.state.settlements.len() != scenario.settlements.len() {
            return Err("guided settlement state count does not match authored sites".to_owned());
        }
        if !self
            .state
            .available_castling_routes
            .iter()
            .all(|id| scenario.castling_routes.iter().any(|route| &route.id == id))
        {
            return Err("guided start contains an unknown castling right".to_owned());
        }
        if !self.allow_clock && self.state.clocks.is_some() {
            return Err("guided start includes a clock while clocks are disabled".to_owned());
        }
        validate_start_realm(&self.state)?;
        if self.state.promotion_candidates.iter().any(|(id, _)| {
            self.state
                .pieces
                .get(id)
                .is_none_or(|piece| piece.kind != PieceKind::Pawn)
        }) {
            return Err("guided promotion candidate is not a living Pawn".to_owned());
        }
        validate_start_choices(scenario, &self.state)?;
        crate::validate_promotion_eligibility(scenario, &self.state)
            .map_err(|error| error.to_string())?;
        crate::state::validate_exploration_unchecked(scenario, &self.state)
            .map_err(|error| error.to_string())
    }
}

fn validate_start_realm(state: &MatchState) -> Result<(), String> {
    for (index, settlement) in state.settlements.iter().enumerate() {
        if usize::from(settlement.site_index) != index {
            return Err("guided settlement indices must match authored site order".to_owned());
        }
        if let Some(owner) = settlement.owner {
            let founder = settlement
                .founder
                .and_then(|id| state.pieces.get(&id))
                .ok_or_else(|| "owned guided settlement needs a living founder".to_owned())?;
            if founder.owner != owner {
                return Err("guided settlement founder has the wrong owner".to_owned());
            }
        } else if settlement.founder.is_some() || settlement.established {
            return Err("neutral guided settlement contains owned lineage".to_owned());
        }
        if let Some(produced) = settlement
            .produced_pawn
            .and_then(|id| state.pieces.get(&id))
            && (produced.kind != PieceKind::Pawn
                || Some(produced.owner) != settlement.owner
                || produced.origin
                    != (PieceOrigin::Settlement {
                        settlement_index: settlement.site_index,
                    }))
        {
            return Err("guided produced Pawn lineage is inconsistent".to_owned());
        }
        if let Some(candidate) = settlement
            .transfer_candidate
            .and_then(|id| state.pieces.get(&id))
            && (candidate.kind != PieceKind::Pawn || settlement.owner == Some(candidate.owner))
        {
            return Err("guided transfer candidate is inconsistent".to_owned());
        }
    }
    Ok(())
}

fn validate_start_choices(scenario: &ScenarioDefinition, state: &MatchState) -> Result<(), String> {
    let TurnPhase::ResolvingChoices { queue } = &state.phase else {
        return Ok(());
    };
    for choice in queue {
        match choice {
            crate::state::MandatoryChoice::Promote { pawn, .. } => {
                if state
                    .pieces
                    .get(pawn)
                    .is_none_or(|piece| piece.kind != PieceKind::Pawn)
                {
                    return Err("guided promotion choice does not reference a Pawn".to_owned());
                }
            }
            crate::state::MandatoryChoice::PlacePawn {
                settlement_index,
                legal_squares,
            } => {
                if usize::from(*settlement_index) >= scenario.settlements.len()
                    || legal_squares.is_empty()
                    || legal_squares.iter().any(|at| {
                        !at.is_within(scenario.board)
                            || scenario.terrain.get(at) == Some(&TileTerrain::Mountain)
                    })
                {
                    return Err("guided Pawn-placement choice is inconsistent".to_owned());
                }
            }
        }
    }
    Ok(())
}

impl GuidedStage {
    fn validate(&self, scenario: &ScenarioDefinition, ids: &BTreeSet<&str>) -> Result<(), String> {
        validate_key("stage id", &self.id)?;
        validate_key("stage title key", &self.title_key)?;
        validate_key("stage explanation key", &self.explanation_key)?;
        if self.hint_keys.len() > MAX_HINTS {
            return Err(format!(
                "stage {:?} has more than {MAX_HINTS} hints",
                self.id
            ));
        }
        for hint in &self.hint_keys {
            validate_key("hint key", hint)?;
        }
        if self.success.is_empty()
            || self.success.len() > MAX_PREDICATES
            || self.failure.len() > MAX_PREDICATES
        {
            return Err(format!(
                "stage {:?} has an invalid predicate count",
                self.id
            ));
        }
        for prerequisite in &self.prerequisites {
            if prerequisite == &self.id || !ids.contains(prerequisite.as_str()) {
                return Err(format!("stage {:?} has an invalid prerequisite", self.id));
            }
        }
        for predicate in self.success.iter().chain(&self.failure) {
            predicate.validate(scenario)?;
        }
        if self.action_limit == Some(0) || self.turn_limit == Some(0) {
            return Err(format!("stage {:?} limits must be positive", self.id));
        }
        Ok(())
    }

    /// Evaluates failure first and then the complete success predicate set.
    ///
    /// # Errors
    ///
    /// Returns a canonical query error if the supplied state cannot be inspected.
    pub fn evaluate(
        &self,
        context: &GuidedPredicateContext<'_>,
    ) -> Result<ObjectiveResult, String> {
        if self
            .action_limit
            .is_some_and(|limit| context.actions_taken > limit)
            || self
                .turn_limit
                .is_some_and(|limit| context.turns_elapsed > limit)
        {
            return Ok(ObjectiveResult::Failed);
        }
        if self
            .failure
            .iter()
            .any(|predicate| predicate.evaluate(context).unwrap_or(false))
        {
            return Ok(ObjectiveResult::Failed);
        }
        if self
            .success
            .iter()
            .map(|predicate| predicate.evaluate(context))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .all(|matched| matched)
        {
            Ok(ObjectiveResult::Succeeded)
        } else {
            Ok(ObjectiveResult::InProgress)
        }
    }
}

impl GuidedPredicate {
    fn validate(&self, scenario: &ScenarioDefinition) -> Result<(), String> {
        let coordinate = match self {
            Self::LegalMove { to, .. } | Self::PieceAt { at: to, .. } => Some(*to),
            Self::PieceSurvives { .. }
            | Self::PieceOnTerrain { .. }
            | Self::MaterialAtLeast { .. }
            | Self::InCheck { .. }
            | Self::TurnPhase { .. }
            | Self::SettlementOwned { .. }
            | Self::SettlementGoverned { .. }
            | Self::SettlementEstablished { .. }
            | Self::SettlementProducedPawn { .. }
            | Self::Outcome { .. }
            | Self::Event(_) => None,
        };
        if coordinate.is_some_and(|at| !at.is_within(scenario.board)) {
            return Err("guided predicate coordinate is outside the board".to_owned());
        }
        let settlement = match self {
            Self::SettlementOwned {
                settlement_index, ..
            }
            | Self::SettlementGoverned {
                settlement_index, ..
            }
            | Self::SettlementEstablished {
                settlement_index, ..
            }
            | Self::SettlementProducedPawn {
                settlement_index, ..
            } => Some(*settlement_index),
            _ => None,
        };
        if settlement.is_some_and(|index| usize::from(index) >= scenario.settlements.len()) {
            return Err("guided predicate references an unknown settlement".to_owned());
        }
        Ok(())
    }

    /// Evaluates this read-only predicate against canonical state and accepted events.
    ///
    /// # Errors
    ///
    /// Returns a canonical query error if legality, check, or governance cannot be inspected.
    pub fn evaluate(&self, context: &GuidedPredicateContext<'_>) -> Result<bool, String> {
        let state = context.state;
        match *self {
            Self::LegalMove { player, piece, to } => {
                if state.active_player != player {
                    return Ok(false);
                }
                Ok(matches!(
                    inspect_move(context.scenario, state, piece, to).map_err(|e| e.to_string())?,
                    MoveInspection::Legal(_)
                ))
            }
            Self::PieceAt { player, kind, at } => Ok(state
                .pieces
                .values()
                .any(|piece| piece.owner == player && piece.kind == kind && piece.at == at)),
            Self::PieceSurvives { piece } => Ok(state.pieces.contains_key(&piece)),
            Self::PieceOnTerrain { piece, terrain } => {
                Ok(state.pieces.get(&piece).is_some_and(|piece| {
                    context
                        .scenario
                        .terrain
                        .get(&piece.at)
                        .copied()
                        .unwrap_or(TileTerrain::Open)
                        == terrain
                }))
            }
            Self::MaterialAtLeast {
                player,
                kind,
                count,
            } => Ok(state
                .pieces
                .values()
                .filter(|piece| piece.owner == player && piece.kind == kind)
                .count()
                >= usize::from(count)),
            Self::InCheck { player, expected } => Ok(is_in_check(context.scenario, state, player)
                .map_err(|e| e.to_string())?
                == expected),
            Self::TurnPhase { phase } => Ok(matches!(
                (phase, &state.phase),
                (GuidedTurnPhase::Command, TurnPhase::Command)
                    | (
                        GuidedTurnPhase::MandatoryChoice,
                        TurnPhase::ResolvingChoices { .. }
                    )
            )),
            Self::SettlementOwned {
                settlement_index,
                player,
            } => Ok(state
                .settlements
                .get(usize::from(settlement_index))
                .is_some_and(|settlement| settlement.owner == Some(player))),
            Self::SettlementGoverned {
                settlement_index,
                player,
            } => Ok(governance_report(context.scenario, state, settlement_index)
                .map_err(|e| e.to_string())?
                .governors
                .iter()
                .any(|line| {
                    state
                        .pieces
                        .get(&line.attacker)
                        .is_some_and(|piece| piece.owner == player)
                })),
            Self::SettlementEstablished {
                settlement_index,
                expected,
            } => Ok(state
                .settlements
                .get(usize::from(settlement_index))
                .is_some_and(|settlement| settlement.established == expected)),
            Self::SettlementProducedPawn {
                settlement_index,
                expected,
            } => Ok(state
                .settlements
                .get(usize::from(settlement_index))
                .is_some_and(|settlement| settlement.produced_pawn.is_some() == expected)),
            Self::Outcome { winner, reason } => {
                Ok(state.outcome == Some(MatchOutcome { winner, reason }))
            }
            Self::Event(ref event) => Ok(event.matches(context)),
        }
    }
}

impl GuidedEventPredicate {
    fn matches(&self, context: &GuidedPredicateContext<'_>) -> bool {
        context.events.iter().any(|event| match (self, event) {
            (Self::Move { piece }, TransitionEvent::PieceMoved { piece: found, .. }) => {
                piece.is_none_or(|id| id == *found)
            }
            (Self::Capture { piece }, TransitionEvent::PieceCaptured { piece: found, .. }) => {
                piece.is_none_or(|id| id == *found)
            }
            (
                Self::SettlementClaimed { settlement_index },
                TransitionEvent::SettlementClaimed {
                    settlement_index: found,
                    ..
                },
            )
            | (
                Self::SettlementEstablished { settlement_index },
                TransitionEvent::SettlementEstablished {
                    settlement_index: found,
                },
            )
            | (
                Self::PawnProduced { settlement_index },
                TransitionEvent::PawnProduced {
                    settlement_index: found,
                    ..
                },
            )
            | (
                Self::SettlementTransferred { settlement_index },
                TransitionEvent::SettlementTransferred {
                    settlement_index: found,
                    ..
                },
            ) => settlement_index.is_none_or(|index| index == *found),
            (
                Self::Promotion { pawn, kind },
                TransitionEvent::PiecePromoted {
                    pawn: found_pawn,
                    kind: found_kind,
                    ..
                },
            ) => {
                pawn.is_none_or(|id| id == *found_pawn)
                    && kind.is_none_or(|value| value == *found_kind)
            }
            (Self::MatchEnded, TransitionEvent::MatchEnded { .. }) => true,
            (
                Self::CrossEdge { piece, kind },
                TransitionEvent::PieceMoved {
                    piece: found,
                    from,
                    to,
                },
            ) => {
                piece.is_none_or(|id| id == *found)
                    && context.scenario.edges.get(&Edge::new(*from, *to)) == Some(kind)
            }
            (
                Self::EnterTerrain { piece, terrain },
                TransitionEvent::PieceMoved {
                    piece: found, to, ..
                },
            ) => {
                piece.is_none_or(|id| id == *found)
                    && context
                        .scenario
                        .terrain
                        .get(to)
                        .copied()
                        .unwrap_or(TileTerrain::Open)
                        == *terrain
            }
            _ => false,
        })
    }
}

impl GuidedAiConfig {
    fn validate(
        &self,
        _stage_ids: &BTreeSet<&str>,
        reply_nodes: &[GuidedReplyNode],
    ) -> Result<(), String> {
        match &self.mode {
            GuidedAiMode::GeneralProfile { profile_id } => {
                validate_key("AI profile id", profile_id)
            }
            GuidedAiMode::RegisteredPolicy { policy_id } => validate_key("AI policy id", policy_id),
            GuidedAiMode::ReplyTree { root_node_id } => {
                validate_key("reply-tree root", root_node_id)?;
                if !reply_nodes.iter().any(|node| &node.id == root_node_id) {
                    return Err("reply-tree root references an unknown node".to_owned());
                }
                Ok(())
            }
        }
    }
}

fn validate_reply_nodes(nodes: &[GuidedReplyNode]) -> Result<(), String> {
    if nodes.len() > MAX_REPLY_NODES {
        return Err(format!("reply tree exceeds {MAX_REPLY_NODES} nodes"));
    }
    let ids: BTreeSet<_> = nodes.iter().map(|node| node.id.as_str()).collect();
    if ids.len() != nodes.len() {
        return Err("reply-tree node ids must be unique".to_owned());
    }
    for node in nodes {
        validate_key("reply-tree node id", &node.id)?;
        validate_key("reply-tree position key", &node.position_key)?;
        if node
            .child_ids
            .iter()
            .any(|child| !ids.contains(child.as_str()))
        {
            return Err(format!(
                "reply node {:?} references an unknown child",
                node.id
            ));
        }
    }
    Ok(())
}

fn validate_acyclic(stages: &[GuidedStage]) -> Result<(), String> {
    fn visit<'a>(
        id: &'a str,
        graph: &BTreeMap<&'a str, &'a [String]>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> Result<(), String> {
        if visited.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(format!("stage prerequisite cycle includes {id:?}"));
        }
        for prerequisite in graph.get(id).copied().unwrap_or_default() {
            visit(prerequisite, graph, visiting, visited)?;
        }
        visiting.remove(id);
        visited.insert(id);
        Ok(())
    }
    let prerequisites: BTreeMap<_, _> = stages
        .iter()
        .map(|stage| (stage.id.as_str(), stage.prerequisites.as_slice()))
        .collect();
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in prerequisites.keys().copied() {
        visit(id, &prerequisites, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn validate_key(kind: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > MAX_KEY_CHARS {
        Err(format!(
            "{kind} must contain 1..={MAX_KEY_CHARS} characters"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ActionJournal, AppendOutcome, IdempotencyKey, SaveEnvelope, SaveReader, apply_action,
        legal_moves,
    };

    use super::*;

    fn fixture() -> (ScenarioDefinition, MatchState) {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        (scenario, state)
    }

    fn content(state: MatchState) -> GuidedContent {
        let king = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::King)
            .unwrap()
            .id;
        GuidedContent {
            schema_version: GUIDED_SCHEMA_VERSION,
            id: "lesson.movement.1".to_owned(),
            kind: GuidedKind::Tutorial,
            category_key: "guided.category.movement".to_owned(),
            start: GuidedStart {
                state,
                human_seat: Player::South,
                allow_clock: false,
                allow_controller_changes: false,
            },
            stages: vec![GuidedStage {
                id: "survive".to_owned(),
                title_key: "guided.movement.survive.title".to_owned(),
                explanation_key: "guided.movement.survive.explanation".to_owned(),
                hint_keys: vec!["guided.movement.survive.hint.1".to_owned()],
                prerequisites: Vec::new(),
                success: vec![GuidedPredicate::PieceSurvives { piece: king }],
                failure: vec![GuidedPredicate::Outcome {
                    winner: Some(Player::North),
                    reason: OutcomeReason::Checkmate,
                }],
                action_limit: Some(4),
                turn_limit: Some(2),
            }],
            ai: Some(GuidedAiConfig {
                seat: Player::North,
                mode: GuidedAiMode::GeneralProfile {
                    profile_id: "apprentice".to_owned(),
                },
                max_actions: Some(4),
            }),
            completion: Some(GuidedCompletion {
                completion_key: "guided.movement.complete".to_owned(),
                next_guided_id: None,
                records_best_actions: true,
                records_best_turns: true,
            }),
            reply_nodes: Vec::new(),
        }
    }

    #[test]
    fn guided_start_constructs_exact_deterministic_canonical_state() {
        let (mut scenario, mut state) = fixture();
        state.active_player = Player::North;
        state.turn_number = 7;
        state.revision = 12;
        let expected_hash = state.canonical_hash().unwrap();
        scenario.guided = Some(content(state));
        assert_eq!(scenario.validate(), Ok(()));
        let first = MatchState::from_scenario(&scenario).unwrap();
        let second = MatchState::from_scenario(&scenario).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.canonical_hash().unwrap(), expected_hash);
    }

    #[test]
    fn predicate_results_follow_canonical_transition_events() {
        let (scenario, state) = fixture();
        let candidate = legal_moves(&scenario, &state).unwrap()[0];
        let action = Action::Move {
            player: state.active_player,
            piece: candidate.piece,
            to: candidate.to,
        };
        let transition = apply_action(&scenario, &state, &action).unwrap();
        let context = GuidedPredicateContext {
            scenario: &scenario,
            state: &transition.state,
            events: &transition.events,
            actions_taken: 1,
            turns_elapsed: 1,
        };
        assert!(
            GuidedPredicate::Event(GuidedEventPredicate::Move {
                piece: Some(candidate.piece),
            })
            .evaluate(&context)
            .unwrap()
        );
        assert!(
            GuidedPredicate::PieceAt {
                player: state.active_player,
                kind: state.pieces[&candidate.piece].kind,
                at: candidate.to,
            }
            .evaluate(&context)
            .unwrap()
        );
    }

    #[test]
    fn validation_rejects_invalid_start_cycle_reference_and_schema() {
        let (mut scenario, state) = fixture();
        let mut guided = content(state);
        guided.start.state.pieces.values_mut().next().unwrap().at = Coord::new(99, 99);
        scenario.guided = Some(guided);
        assert!(matches!(
            scenario.validate(),
            Err(errors) if errors.iter().any(|error| matches!(error, crate::scenario::ScenarioError::InvalidGuidedContent(_)))
        ));

        let (mut scenario, state) = fixture();
        let mut guided = content(state);
        guided.stages.push(GuidedStage {
            id: "cycle".to_owned(),
            title_key: "cycle.title".to_owned(),
            explanation_key: "cycle.explanation".to_owned(),
            hint_keys: Vec::new(),
            prerequisites: vec!["survive".to_owned()],
            success: vec![GuidedPredicate::TurnPhase {
                phase: GuidedTurnPhase::Command,
            }],
            failure: Vec::new(),
            action_limit: None,
            turn_limit: None,
        });
        guided.stages[0].prerequisites = vec!["cycle".to_owned()];
        scenario.guided = Some(guided);
        assert!(scenario.validate().is_err());

        let (mut scenario, state) = fixture();
        let mut guided = content(state);
        guided.schema_version += 1;
        scenario.guided = Some(guided);
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn absent_guided_block_round_trips_without_changing_scenario_hash() {
        let (scenario, _) = fixture();
        let hash = scenario.canonical_hash().unwrap();
        let encoded = ron::to_string(&scenario).unwrap();
        assert!(!encoded.contains("guided"));
        let decoded: ScenarioDefinition = ron::from_str(&encoded).unwrap();
        assert_eq!(decoded.guided, None);
        assert_eq!(decoded.canonical_hash().unwrap(), hash);
    }

    #[test]
    fn objective_result_matches_live_save_and_replay_states() {
        let (mut scenario, state) = fixture();
        let candidate = legal_moves(&scenario, &state).unwrap()[0];
        let action = Action::Move {
            player: state.active_player,
            piece: candidate.piece,
            to: candidate.to,
        };
        let mut guided = content(state.clone());
        guided.stages[0].success = vec![GuidedPredicate::Event(GuidedEventPredicate::Move {
            piece: Some(candidate.piece),
        })];
        scenario.guided = Some(guided);

        let transition = apply_action(&scenario, &state, &action).unwrap();
        let live = GuidedPredicateContext {
            scenario: &scenario,
            state: &transition.state,
            events: &transition.events,
            actions_taken: 1,
            turns_elapsed: 1,
        };
        let expected = scenario.guided.as_ref().unwrap().stages[0]
            .evaluate(&live)
            .unwrap();

        let envelope = SaveEnvelope::new("guided-test", transition.state.clone()).unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        let loaded = SaveReader::new()
            .read_with_scenario(&bytes, &scenario)
            .unwrap()
            .state;
        let loaded_context = GuidedPredicateContext {
            scenario: &scenario,
            state: &loaded,
            events: &transition.events,
            actions_taken: 1,
            turns_elapsed: 1,
        };

        let mut journal = ActionJournal::new("guided-test", &scenario).unwrap();
        let appended = journal
            .append(&scenario, &state, IdempotencyKey([9; 16]), &action)
            .unwrap();
        assert!(matches!(appended, AppendOutcome::Accepted(_)));
        let replayed = journal.replay(&scenario).unwrap();
        let replay_context = GuidedPredicateContext {
            scenario: &scenario,
            state: &replayed,
            events: &journal.records[0].events,
            actions_taken: 1,
            turns_elapsed: 1,
        };
        let objective = &scenario.guided.as_ref().unwrap().stages[0];
        assert_eq!(objective.evaluate(&loaded_context).unwrap(), expected);
        assert_eq!(objective.evaluate(&replay_context).unwrap(), expected);
        assert_eq!(expected, ObjectiveResult::Succeeded);
    }
}
