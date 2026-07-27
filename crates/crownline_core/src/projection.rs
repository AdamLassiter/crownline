//! Seat-explicit, privacy-preserving projections of canonical match state.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    rules::{Transition, TransitionEvent, apply_action, governance_report, is_in_check},
    scenario::{
        BoardSize, Coord, Edge, EdgeKind, PieceKind, Player, ScenarioDefinition, TileTerrain,
    },
    state::{
        Action, ClockState, EnPassantState, MandatoryChoice, MatchOutcome, MatchState, Piece,
        PieceId, PieceOrigin, PromotionEligibility, SettlementState, TransitionError, TurnPhase,
        validate_exploration, visible_coordinates,
    },
};

pub const PLAYER_VIEW_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerView {
    pub schema_version: u16,
    pub seat: Player,
    pub scenario_id: String,
    pub revision: u64,
    pub projection_hash: String,
    pub board: BoardSize,
    pub pawn_forward_y: i8,
    pub allow_pawn_double_step: bool,
    pub visible: BTreeSet<Coord>,
    pub squares: Vec<KnownSquare>,
    pub edges: Vec<KnownEdge>,
    pub pieces: BTreeMap<PieceId, ViewPiece>,
    pub settlements: BTreeMap<u16, SettlementView>,
    pub promotion_candidates: BTreeMap<PieceId, u8>,
    pub own_castling_routes: BTreeSet<String>,
    pub own_castling_destinations: BTreeSet<Coord>,
    pub en_passant: Option<EnPassantState>,
    pub active_player: Player,
    pub turn_number: u64,
    pub phase: ViewTurnPhase,
    pub checked_players: BTreeSet<Player>,
    pub clocks: Option<ClockState>,
    pub outstanding_draw_offer: Option<Player>,
    pub outcome: Option<MatchOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownSquare {
    pub at: Coord,
    pub terrain: TileTerrain,
    pub settlement: Option<StaticSiteView>,
    pub promotion_site: Option<StaticSiteView>,
    pub keeps: BTreeSet<StaticOwnedSiteView>,
    pub fortifications: BTreeSet<StaticOwnedSiteView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownEdge {
    pub edge: Edge,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StaticSiteView {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StaticOwnedSiteView {
    pub id: String,
    pub owner: Player,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewPiece {
    pub id: PieceId,
    pub owner: Player,
    pub kind: PieceKind,
    pub at: Coord,
    pub origin: PieceOrigin,
    pub has_moved: bool,
}

impl From<&Piece> for ViewPiece {
    fn from(piece: &Piece) -> Self {
        Self {
            id: piece.id,
            owner: piece.owner,
            kind: piece.kind,
            at: piece.at,
            origin: piece.origin,
            has_moved: piece.has_moved,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementView {
    pub site_index: u16,
    pub id: String,
    pub at: Coord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<SettlementDynamicView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementDynamicView {
    pub owner: Option<Player>,
    pub founder: Option<PieceId>,
    pub establishment_progress: u8,
    pub established: bool,
    pub production_progress: u8,
    pub produced_pawn: Option<PieceId>,
    pub cycle_interrupted: bool,
    pub completed_cycle_continuous: bool,
    pub transfer_candidate: Option<PieceId>,
    pub governance: GovernanceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceState {
    Ungoverned,
    Governed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewTurnPhase {
    Command,
    OwnChoices { queue: Vec<ViewMandatoryChoice> },
    PrivateChoice { player: Player, remaining: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMandatoryChoice {
    Promote {
        pawn: PieceId,
        site_index: u16,
        eligibility: PromotionEligibility,
    },
    PlacePawn {
        settlement_index: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverView {
    pub at: Coord,
    pub currently_visible: bool,
    pub square: KnownSquare,
    pub piece: Option<ViewPiece>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerEvent {
    OwnPieceMoved {
        piece: PieceId,
        from: Coord,
        to: Coord,
    },
    OwnPieceCaptured {
        piece: PieceId,
    },
    OwnPiecePromoted {
        pawn: PieceId,
        promoted: PieceId,
        kind: PieceKind,
        at: Coord,
    },
    OwnPawnProduced {
        settlement_index: u16,
        pawn: PieceId,
        at: Coord,
    },
    ObservedSettlementChanged {
        settlement_index: u16,
    },
    ClockChanged,
    DrawChanged,
    TurnStarted {
        player: Player,
        turn_number: u64,
    },
    MatchEnded {
        outcome: MatchOutcome,
    },
    ActionResolved {
        player: Player,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerIntentError {
    IllegalIntent,
}

/// Builds the only canonical-to-seat state projection. The viewing seat is
/// mandatory; there is deliberately no default, spectator, or omniscient mode.
///
/// # Errors
///
/// Returns scenario, state, exploration, governance, check, or serialization errors.
pub fn project_player_view(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    seat: Player,
) -> Result<PlayerView, TransitionError> {
    validate_exploration(scenario, state)?;
    let visible = visible_coordinates(scenario, state, seat)?;
    let explored = state.exploration.as_ref().map_or_else(
        || visible.clone(),
        |knowledge| knowledge.explored(seat).clone(),
    );

    let squares = explored
        .iter()
        .copied()
        .map(|at| known_square(scenario, at))
        .collect();
    let edges = scenario
        .edges
        .iter()
        .filter(|(edge, _)| explored.contains(&edge.first) || explored.contains(&edge.second))
        .map(|(edge, kind)| KnownEdge {
            edge: *edge,
            kind: *kind,
        })
        .collect();
    let pieces: BTreeMap<_, _> = state
        .pieces
        .iter()
        .filter(|(_, piece)| piece.owner == seat || visible.contains(&piece.at))
        .map(|(id, piece)| (*id, ViewPiece::from(piece)))
        .collect();
    let disclosed_ids: BTreeSet<_> = pieces.keys().copied().collect();
    let settlements =
        project_settlements(scenario, state, seat, &visible, &explored, &disclosed_ids)?;
    let promotion_candidates = state
        .promotion_candidates
        .iter()
        .filter(|(id, _)| disclosed_ids.contains(id))
        .map(|(id, progress)| (*id, *progress))
        .collect();
    let own_castling_routes = scenario
        .castling_routes
        .iter()
        .filter(|route| route.player == seat && state.available_castling_routes.contains(&route.id))
        .map(|route| route.id.clone())
        .collect();
    let own_castling_destinations = scenario
        .castling_routes
        .iter()
        .filter(|route| {
            route.player == seat
                && state.available_castling_routes.contains(&route.id)
                && visible.contains(&route.king_destination)
        })
        .map(|route| route.king_destination)
        .collect();
    let en_passant = state.en_passant.filter(|en_passant| {
        en_passant.expires_for == seat && disclosed_ids.contains(&en_passant.pawn)
    });
    let phase = project_phase(state, seat);
    let checked_players = Player::ALL
        .into_iter()
        .filter_map(|player| {
            is_in_check(scenario, state, player)
                .map(|checked| checked.then_some(player))
                .transpose()
        })
        .collect::<Result<_, _>>()?;

    let mut view = PlayerView {
        schema_version: PLAYER_VIEW_SCHEMA_VERSION,
        seat,
        scenario_id: scenario.id.clone(),
        revision: state.revision,
        projection_hash: String::new(),
        board: scenario.board,
        pawn_forward_y: scenario.rules.pawn_forward_y[&seat],
        allow_pawn_double_step: scenario.rules.allow_pawn_double_step,
        visible,
        squares,
        edges,
        pieces,
        settlements,
        promotion_candidates,
        own_castling_routes,
        own_castling_destinations,
        en_passant,
        active_player: state.active_player,
        turn_number: state.turn_number,
        phase,
        checked_players,
        clocks: state.clocks,
        outstanding_draw_offer: state.outstanding_draw_offer,
        outcome: state.outcome,
    };
    view.projection_hash = view.calculate_hash()?;
    Ok(view)
}

impl PlayerView {
    /// Recomputes the deterministic projection hash without hashing the hash itself.
    ///
    /// # Errors
    ///
    /// Returns a canonical JSON serialization error.
    pub fn calculate_hash(&self) -> Result<String, TransitionError> {
        let mut hashless = self.clone();
        hashless.projection_hash.clear();
        let bytes = serde_json::to_vec(&hashless).map_err(TransitionError::Serialize)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    #[must_use]
    pub fn hover(&self, at: Coord) -> Option<HoverView> {
        self.squares
            .iter()
            .find(|square| square.at == at)
            .cloned()
            .map(|square| HoverView {
                at,
                currently_visible: self.visible.contains(&at),
                square,
                piece: self.pieces.values().find(|piece| piece.at == at).cloned(),
            })
    }

    #[must_use]
    pub fn square_explanation(&self, at: Coord) -> String {
        let Some(hover) = self.hover(at) else {
            return "Undiscovered square".to_owned();
        };
        let visibility = if hover.currently_visible {
            "Visible"
        } else {
            "Explored"
        };
        let mut text = format!("{visibility} {:?} terrain", hover.square.terrain);
        if let Some(site) = hover.square.settlement {
            let _ = write!(text, "; settlement {}", site.id);
        }
        if let Some(site) = hover.square.promotion_site {
            let _ = write!(text, "; promotion site {}", site.id);
        }
        if let Some(piece) = hover.piece {
            let _ = write!(text, "; {:?} {:?}", piece.owner, piece.kind);
        }
        text
    }

    /// Returns projection-derived destinations that may be submitted as intents.
    /// These are deliberately not represented as canonical legal moves.
    #[must_use]
    pub fn intent_candidates(&self, piece_id: PieceId) -> BTreeSet<Coord> {
        if self.active_player != self.seat || !matches!(self.phase, ViewTurnPhase::Command) {
            return BTreeSet::new();
        }
        let Some(piece) = self
            .pieces
            .get(&piece_id)
            .filter(|piece| piece.owner == self.seat)
        else {
            return BTreeSet::new();
        };
        let own_occupied: BTreeSet<_> = self
            .pieces
            .values()
            .filter(|candidate| candidate.owner == self.seat)
            .map(|candidate| candidate.at)
            .collect();
        self.visible
            .iter()
            .copied()
            .filter(|at| *at != piece.at && !own_occupied.contains(at))
            .filter(|at| {
                piece_geometry_can_reach(
                    piece,
                    *at,
                    self.pawn_forward_y,
                    self.allow_pawn_double_step,
                ) || (piece.kind == PieceKind::King && self.own_castling_destinations.contains(at))
            })
            .collect()
    }

    /// Returns visible adjacent squares that may be submitted for the current
    /// own Pawn-placement choice. Hidden occupancy and canonical edge legality
    /// are deliberately not consulted.
    #[must_use]
    pub fn placement_intent_candidates(&self, settlement_index: u16) -> BTreeSet<Coord> {
        let ViewTurnPhase::OwnChoices { queue } = &self.phase else {
            return BTreeSet::new();
        };
        if !matches!(
            queue.first(),
            Some(ViewMandatoryChoice::PlacePawn {
                settlement_index: queued,
            }) if *queued == settlement_index
        ) {
            return BTreeSet::new();
        }
        let Some(settlement) = self.settlements.get(&settlement_index) else {
            return BTreeSet::new();
        };
        let own_occupied: BTreeSet<_> = self
            .pieces
            .values()
            .filter(|piece| piece.owner == self.seat)
            .map(|piece| piece.at)
            .collect();
        self.squares
            .iter()
            .filter(|square| {
                self.visible.contains(&square.at)
                    && square.terrain != TileTerrain::Mountain
                    && !own_occupied.contains(&square.at)
                    && settlement.at.x.abs_diff(square.at.x) <= 1
                    && settlement.at.y.abs_diff(square.at.y) <= 1
                    && square.at != settlement.at
            })
            .map(|square| square.at)
            .collect()
    }
}

/// Applies an authenticated seat intent while collapsing every rejection into
/// one hidden-information-safe error. The returned transition remains authority-internal.
///
/// # Errors
///
/// Returns only [`PlayerIntentError::IllegalIntent`] for a seat mismatch or any
/// canonical rejection.
pub fn apply_player_intent(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    seat: Player,
    action: &Action,
) -> Result<Transition, PlayerIntentError> {
    if action_player(action) != seat {
        return Err(PlayerIntentError::IllegalIntent);
    }
    apply_action(scenario, state, action).map_err(|_| PlayerIntentError::IllegalIntent)
}

/// Filters canonical events against the post-transition seat view. Opponent
/// details are intentionally collapsed; the next `PlayerView` carries every
/// currently permitted fact without leaving a last-known-position log.
#[must_use]
pub fn project_events(
    scenario: &ScenarioDefinition,
    before: &MatchState,
    after: &MatchState,
    seat: Player,
    events: &[TransitionEvent],
) -> Vec<PlayerEvent> {
    let visible = visible_coordinates(scenario, after, seat).unwrap_or_default();
    let own_before = |id: PieceId| {
        before
            .pieces
            .get(&id)
            .is_some_and(|piece| piece.owner == seat)
    };
    let own_after = |id: PieceId| {
        after
            .pieces
            .get(&id)
            .is_some_and(|piece| piece.owner == seat)
    };
    let settlement_permitted = |index: u16| {
        scenario
            .settlements
            .get(usize::from(index))
            .is_some_and(|site| visible.contains(&site.at))
            || after
                .settlements
                .iter()
                .find(|settlement| settlement.site_index == index)
                .is_some_and(|settlement| settlement.owner == Some(seat))
    };
    let mut projected = Vec::new();
    let mut collapsed_opponent = false;
    for event in events {
        let safe = match *event {
            TransitionEvent::PieceMoved { piece, from, to } if own_after(piece) => {
                Some(PlayerEvent::OwnPieceMoved { piece, from, to })
            }
            TransitionEvent::PieceCaptured { piece, .. } if own_before(piece) => {
                Some(PlayerEvent::OwnPieceCaptured { piece })
            }
            TransitionEvent::PiecePromoted {
                pawn,
                promoted,
                kind,
                at,
            } if own_after(promoted) => Some(PlayerEvent::OwnPiecePromoted {
                pawn,
                promoted,
                kind,
                at,
            }),
            TransitionEvent::PawnProduced {
                settlement_index,
                pawn,
                at,
            } if own_after(pawn) => Some(PlayerEvent::OwnPawnProduced {
                settlement_index,
                pawn,
                at,
            }),
            TransitionEvent::ClockAdvanced { .. }
            | TransitionEvent::ClockIncrementApplied { .. } => Some(PlayerEvent::ClockChanged),
            TransitionEvent::DrawOffered { .. } | TransitionEvent::DrawAnswered { .. } => {
                Some(PlayerEvent::DrawChanged)
            }
            TransitionEvent::TurnStarted {
                player,
                turn_number,
            } => Some(PlayerEvent::TurnStarted {
                player,
                turn_number,
            }),
            TransitionEvent::MatchEnded { outcome } => Some(PlayerEvent::MatchEnded { outcome }),
            _ => None,
        };
        if let Some(safe) = safe {
            projected.push(safe);
        } else if let Some(index) =
            settlement_index(event).filter(|index| settlement_permitted(*index))
        {
            projected.push(PlayerEvent::ObservedSettlementChanged {
                settlement_index: index,
            });
        } else {
            collapsed_opponent = true;
        }
    }
    if collapsed_opponent {
        projected.push(PlayerEvent::ActionResolved {
            player: before.active_player,
        });
    }
    projected
}

fn known_square(scenario: &ScenarioDefinition, at: Coord) -> KnownSquare {
    KnownSquare {
        at,
        terrain: scenario
            .terrain
            .get(&at)
            .copied()
            .unwrap_or(TileTerrain::Open),
        settlement: scenario
            .settlements
            .iter()
            .find(|site| site.at == at)
            .map(|site| StaticSiteView {
                id: site.id.clone(),
            }),
        promotion_site: scenario
            .promotion_sites
            .iter()
            .find(|site| site.at == at)
            .map(|site| StaticSiteView {
                id: site.id.clone(),
            }),
        keeps: scenario
            .keeps
            .iter()
            .filter(|keep| keep.tiles.contains(&at))
            .map(|keep| StaticOwnedSiteView {
                id: keep.id.clone(),
                owner: keep.owner,
            })
            .collect(),
        fortifications: scenario
            .fortifications
            .iter()
            .filter(|fortification| fortification.tower == at)
            .map(|fortification| StaticOwnedSiteView {
                id: fortification.id.clone(),
                owner: fortification.owner,
            })
            .collect(),
    }
}

fn project_settlements(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    seat: Player,
    visible: &BTreeSet<Coord>,
    explored: &BTreeSet<Coord>,
    disclosed_ids: &BTreeSet<PieceId>,
) -> Result<BTreeMap<u16, SettlementView>, TransitionError> {
    let mut projected = BTreeMap::new();
    for settlement in &state.settlements {
        let site = &scenario.settlements[usize::from(settlement.site_index)];
        if !explored.contains(&site.at) {
            continue;
        }
        let dynamic_permitted = visible.contains(&site.at) || settlement.owner == Some(seat);
        let dynamic = dynamic_permitted
            .then(|| settlement_dynamic(scenario, state, settlement, disclosed_ids))
            .transpose()?;
        projected.insert(
            settlement.site_index,
            SettlementView {
                site_index: settlement.site_index,
                id: site.id.clone(),
                at: site.at,
                dynamic,
            },
        );
    }
    Ok(projected)
}

fn settlement_dynamic(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    settlement: &SettlementState,
    disclosed_ids: &BTreeSet<PieceId>,
) -> Result<SettlementDynamicView, TransitionError> {
    let disclosed = |id: Option<PieceId>| id.filter(|id| disclosed_ids.contains(id));
    Ok(SettlementDynamicView {
        owner: settlement.owner,
        founder: disclosed(settlement.founder),
        establishment_progress: settlement.establishment_progress,
        established: settlement.established,
        production_progress: settlement.production_progress,
        produced_pawn: disclosed(settlement.produced_pawn),
        cycle_interrupted: settlement.cycle_interrupted,
        completed_cycle_continuous: settlement.completed_cycle_continuous,
        transfer_candidate: disclosed(settlement.transfer_candidate),
        governance: if governance_report(scenario, state, settlement.site_index)?
            .governors
            .is_empty()
        {
            GovernanceState::Ungoverned
        } else {
            GovernanceState::Governed
        },
    })
}

fn project_phase(state: &MatchState, seat: Player) -> ViewTurnPhase {
    match &state.phase {
        TurnPhase::Command => ViewTurnPhase::Command,
        TurnPhase::ResolvingChoices { queue } if state.active_player == seat => {
            ViewTurnPhase::OwnChoices {
                queue: queue.iter().map(project_choice).collect(),
            }
        }
        TurnPhase::ResolvingChoices { queue } => ViewTurnPhase::PrivateChoice {
            player: state.active_player,
            remaining: queue.len(),
        },
    }
}

fn project_choice(choice: &MandatoryChoice) -> ViewMandatoryChoice {
    match choice {
        MandatoryChoice::Promote {
            pawn,
            site_index,
            eligibility,
        } => ViewMandatoryChoice::Promote {
            pawn: *pawn,
            site_index: *site_index,
            eligibility: eligibility.clone(),
        },
        MandatoryChoice::PlacePawn {
            settlement_index, ..
        } => ViewMandatoryChoice::PlacePawn {
            settlement_index: *settlement_index,
        },
    }
}

fn piece_geometry_can_reach(
    piece: &ViewPiece,
    to: Coord,
    pawn_forward_y: i8,
    allow_pawn_double_step: bool,
) -> bool {
    let dx = piece.at.x.abs_diff(to.x);
    let dy = piece.at.y.abs_diff(to.y);
    match piece.kind {
        PieceKind::King => dx <= 1 && dy <= 1,
        PieceKind::Queen => dx == 0 || dy == 0 || dx == dy,
        PieceKind::Rook => dx == 0 || dy == 0,
        PieceKind::Bishop => dx == dy,
        PieceKind::Knight => matches!((dx, dy), (1, 2) | (2, 1)),
        PieceKind::Pawn => {
            let forward = i32::from(to.y) - i32::from(piece.at.y);
            (forward == i32::from(pawn_forward_y) && dx <= 1)
                || (allow_pawn_double_step
                    && !piece.has_moved
                    && forward == i32::from(pawn_forward_y) * 2
                    && dx == 0)
        }
    }
}

const fn action_player(action: &Action) -> Player {
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

const fn settlement_index(event: &TransitionEvent) -> Option<u16> {
    match *event {
        TransitionEvent::SettlementContinuityInterrupted { settlement_index }
        | TransitionEvent::SettlementCycleStarted {
            settlement_index, ..
        }
        | TransitionEvent::SettlementClaimed {
            settlement_index, ..
        }
        | TransitionEvent::SettlementContested {
            settlement_index, ..
        }
        | TransitionEvent::SettlementTransferCancelled {
            settlement_index, ..
        }
        | TransitionEvent::SettlementTransferred {
            settlement_index, ..
        }
        | TransitionEvent::SettlementDevelopmentAdvanced {
            settlement_index, ..
        }
        | TransitionEvent::SettlementEstablished {
            settlement_index, ..
        }
        | TransitionEvent::SettlementProductionAdvanced {
            settlement_index, ..
        }
        | TransitionEvent::SettlementProductionReset { settlement_index }
        | TransitionEvent::PawnPlacementReady {
            settlement_index, ..
        } => Some(settlement_index),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        scenario::{
            ArmySetup, Deployment, FOG_RULES_SCHEMA_VERSION, FogRules, ScenarioMetadata,
            ScenarioRules, SettlementSite,
        },
        state::{ExplorationState, PieceId},
        update_exploration,
    };

    const SECRET_ENEMY_ID: PieceId = PieceId(987_654);

    fn scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: crate::scenario::SCENARIO_SCHEMA_VERSION,
            id: "projection-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "Projection test".to_owned(),
                description: String::new(),
                expected_minutes: (1, 2),
                is_default: false,
            },
            board: BoardSize {
                width: 8,
                height: 8,
            },
            terrain: BTreeMap::from([
                (Coord::new(7, 2), TileTerrain::Forest),
                (Coord::new(5, 5), TileTerrain::Mountain),
            ]),
            edges: BTreeMap::from([(
                Edge::new(Coord::new(7, 2), Coord::new(7, 3)),
                EdgeKind::River,
            )]),
            deployments: vec![
                Deployment {
                    player: Player::North,
                    kind: PieceKind::King,
                    at: Coord::new(0, 0),
                },
                Deployment {
                    player: Player::North,
                    kind: PieceKind::Rook,
                    at: Coord::new(0, 1),
                },
                Deployment {
                    player: Player::South,
                    kind: PieceKind::King,
                    at: Coord::new(7, 7),
                },
                Deployment {
                    player: Player::South,
                    kind: PieceKind::Queen,
                    at: Coord::new(7, 2),
                },
            ],
            settlements: vec![SettlementSite {
                id: "secret-citadel".to_owned(),
                at: Coord::new(7, 2),
            }],
            promotion_sites: Vec::new(),
            keeps: Vec::new(),
            fortifications: Vec::new(),
            castling_routes: Vec::new(),
            rules: ScenarioRules {
                army_setup: ArmySetup::Custom,
                fog: Some(FogRules {
                    schema_version: FOG_RULES_SCHEMA_VERSION,
                    vision_radius: 1,
                }),
                ..ScenarioRules::default()
            },
            guided: None,
        }
    }

    fn state() -> MatchState {
        let scenario = scenario();
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        let old = state
            .pieces
            .values()
            .find(|piece| piece.owner == Player::South && piece.kind == PieceKind::Queen)
            .unwrap()
            .id;
        let mut queen = state.pieces.remove(&old).unwrap();
        queen.id = SECRET_ENEMY_ID;
        state.pieces.insert(SECRET_ENEMY_ID, queen);
        state.next_piece_id = SECRET_ENEMY_ID.0 + 1;
        state.settlements[0].owner = Some(Player::South);
        state.settlements[0].founder = Some(SECRET_ENEMY_ID);
        state.settlements[0].establishment_progress = 19;
        state.clocks = Some(ClockState {
            north_millis: 12_345,
            south_millis: 54_321,
            increment_millis: 1_000,
        });
        state.outstanding_draw_offer = Some(Player::North);
        state
            .exploration
            .as_mut()
            .unwrap()
            .north
            .insert(Coord::new(7, 2));
        state.validate_invariants().unwrap();
        state
    }

    #[test]
    fn projections_are_deterministic_distinct_and_contain_no_forbidden_truth() {
        let scenario = scenario();
        let state = state();
        let canonical_hash = state.canonical_hash().unwrap();
        let north = project_player_view(&scenario, &state, Player::North).unwrap();
        let north_again = project_player_view(&scenario, &state, Player::North).unwrap();
        let south = project_player_view(&scenario, &state, Player::South).unwrap();

        assert_eq!(north, north_again);
        assert_eq!(north.calculate_hash().unwrap(), north.projection_hash);
        assert_ne!(north.projection_hash, south.projection_hash);
        assert!(!north.pieces.contains_key(&SECRET_ENEMY_ID));
        assert!(south.pieces.contains_key(&SECRET_ENEMY_ID));
        assert_eq!(north.settlements[&0].dynamic, None);
        assert_eq!(north.clocks, state.clocks);
        assert_eq!(north.outstanding_draw_offer, Some(Player::North));
        assert_eq!(north.active_player, state.active_player);
        assert_eq!(north.outcome, state.outcome);
        assert!(
            north.edges.contains(&KnownEdge {
                edge: Edge::new(Coord::new(7, 2), Coord::new(7, 3)),
                kind: EdgeKind::River,
            }),
            "one explored endpoint permanently reveals a static edge"
        );
        assert!(
            !north
                .squares
                .iter()
                .any(|square| square.at == Coord::new(5, 5))
        );

        let json = serde_json::to_string(&north).unwrap();
        let debug = format!("{north:?}");
        for forbidden in [
            SECRET_ENEMY_ID.0.to_string(),
            canonical_hash,
            "Mountain".to_owned(),
            "mountain".to_owned(),
        ] {
            assert!(!json.contains(&forbidden));
            assert!(!debug.contains(&forbidden));
        }
        assert!(!json.contains("state_hash"));
        assert!(!json.contains("scenario_hash"));
        assert!(!json.contains("legal_squares"));
    }

    #[test]
    fn generated_projection_fields_obey_the_disclosure_property() {
        let scenario = scenario();
        for (revision, queen_at) in [
            Coord::new(7, 2),
            Coord::new(6, 2),
            Coord::new(5, 2),
            Coord::new(4, 2),
        ]
        .into_iter()
        .enumerate()
        {
            let mut state = state();
            state.revision = u64::try_from(revision).unwrap();
            state.pieces.get_mut(&SECRET_ENEMY_ID).unwrap().at = queen_at;
            update_exploration(&scenario, &mut state).unwrap();
            for seat in Player::ALL {
                let view = project_player_view(&scenario, &state, seat).unwrap();
                let explored = state.exploration.as_ref().unwrap().explored(seat);
                assert!(
                    view.squares
                        .iter()
                        .all(|square| explored.contains(&square.at))
                );
                assert!(view.edges.iter().all(|known| {
                    explored.contains(&known.edge.first) || explored.contains(&known.edge.second)
                }));
                assert!(
                    view.pieces
                        .values()
                        .all(|piece| { piece.owner == seat || view.visible.contains(&piece.at) })
                );
                assert!(view.settlements.values().all(|settlement| {
                    settlement.dynamic.is_none()
                        || view.visible.contains(&settlement.at)
                        || settlement.dynamic.as_ref().unwrap().owner == Some(seat)
                }));
                assert!(
                    view.promotion_candidates
                        .keys()
                        .all(|id| view.pieces.contains_key(id))
                );
                for settlement in view
                    .settlements
                    .values()
                    .filter_map(|view| view.dynamic.as_ref())
                {
                    for id in [
                        settlement.founder,
                        settlement.produced_pawn,
                        settlement.transfer_candidate,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        assert!(view.pieces.contains_key(&id));
                    }
                }
            }
        }
    }

    #[test]
    fn projection_queries_never_claim_canonical_hidden_legality() {
        let scenario = scenario();
        let mut state = state();
        state.active_player = Player::North;
        let view = project_player_view(&scenario, &state, Player::North).unwrap();
        let rook = view
            .pieces
            .values()
            .find(|piece| piece.owner == Player::North && piece.kind == PieceKind::Rook)
            .unwrap();
        let candidates = view.intent_candidates(rook.id);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|at| view.visible.contains(at)));
        assert!(candidates.iter().all(|at| {
            !view
                .pieces
                .values()
                .any(|piece| piece.owner == Player::North && piece.at == *at)
        }));
        assert!(view.intent_candidates(SECRET_ENEMY_ID).is_empty());
        assert_eq!(
            apply_player_intent(
                &scenario,
                &state,
                Player::North,
                &Action::Hold {
                    player: Player::South
                },
            ),
            Err(PlayerIntentError::IllegalIntent)
        );
        assert_eq!(
            apply_player_intent(
                &scenario,
                &state,
                Player::North,
                &Action::Move {
                    player: Player::North,
                    piece: rook.id,
                    to: Coord::new(7, 7),
                },
            ),
            Err(PlayerIntentError::IllegalIntent)
        );
        assert_eq!(
            view.square_explanation(Coord::new(5, 5)),
            "Undiscovered square"
        );
    }

    #[test]
    fn private_choices_and_enemy_events_disclose_no_canonical_candidates() {
        let scenario = scenario();
        let mut state = state();
        state.phase = TurnPhase::ResolvingChoices {
            queue: vec![MandatoryChoice::PlacePawn {
                settlement_index: 0,
                legal_squares: BTreeSet::from([Coord::new(6, 1), Coord::new(6, 2)]),
            }],
        };
        let north = project_player_view(&scenario, &state, Player::North).unwrap();
        assert_eq!(
            north.phase,
            ViewTurnPhase::PrivateChoice {
                player: Player::South,
                remaining: 1,
            }
        );
        assert!(
            !serde_json::to_string(&north)
                .unwrap()
                .contains("legal_squares")
        );
        let south = project_player_view(&scenario, &state, Player::South).unwrap();
        assert!(matches!(south.phase, ViewTurnPhase::OwnChoices { .. }));
        assert!(
            !south.placement_intent_candidates(0).is_empty(),
            "own placement UI receives visible projection-derived intent candidates"
        );
        assert!(
            !serde_json::to_string(&south)
                .unwrap()
                .contains("legal_squares")
        );

        let events = vec![TransitionEvent::PieceMoved {
            piece: SECRET_ENEMY_ID,
            from: Coord::new(7, 3),
            to: Coord::new(7, 2),
        }];
        assert_eq!(
            project_events(&scenario, &state, &state, Player::North, &events),
            vec![PlayerEvent::ActionResolved {
                player: Player::South
            }]
        );
        let encoded = serde_json::to_string(&project_events(
            &scenario,
            &state,
            &state,
            Player::North,
            &events,
        ))
        .unwrap();
        assert!(!encoded.contains(&SECRET_ENEMY_ID.0.to_string()));
        assert!(!encoded.contains("\"x\":7"));
    }

    #[test]
    fn perfect_information_projection_contains_every_square_and_piece_without_state_hashes() {
        let mut scenario = scenario();
        scenario.rules.fog = None;
        let mut state = MatchState::from_scenario(&scenario).unwrap();
        state.exploration = None::<ExplorationState>;
        let view = project_player_view(&scenario, &state, Player::North).unwrap();
        assert_eq!(view.squares.len(), 64);
        assert_eq!(view.visible.len(), 64);
        assert_eq!(view.pieces.len(), state.pieces.len());
        assert_eq!(view.settlements[&0].dynamic.as_ref().unwrap().owner, None);
    }
}
