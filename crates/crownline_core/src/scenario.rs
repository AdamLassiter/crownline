use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SCENARIO_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Coord {
    pub x: u16,
    pub y: u16,
}

impl Coord {
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }

    pub const fn is_within(self, board: BoardSize) -> bool {
        self.x < board.width && self.y < board.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Player {
    North,
    South,
}

impl Player {
    pub const ALL: [Self; 2] = [Self::North, Self::South];

    #[must_use]
    pub const fn opponent(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceKind {
    King,
    Queen,
    Rook,
    Bishop,
    Knight,
    Pawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileTerrain {
    Open,
    Forest,
    Mountain,
    Road,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    River,
    Bridge,
    Ford,
    Wall,
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub first: Coord,
    pub second: Coord,
}

impl Edge {
    pub fn new(a: Coord, b: Coord) -> Self {
        if a <= b {
            Self {
                first: a,
                second: b,
            }
        } else {
            Self {
                first: b,
                second: a,
            }
        }
    }

    pub fn is_orthogonally_adjacent(self) -> bool {
        let dx = self.first.x.abs_diff(self.second.x);
        let dy = self.first.y.abs_diff(self.second.y);
        dx + dy == 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub player: Player,
    pub kind: PieceKind,
    pub at: Coord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementSite {
    pub id: String,
    pub at: Coord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionSite {
    pub id: String,
    pub at: Coord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fortification {
    pub id: String,
    pub owner: Player,
    pub tower: Coord,
    pub projected_wall: Edge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastlingRoute {
    pub id: String,
    pub player: Player,
    pub king_start: Coord,
    pub rook_start: Coord,
    pub king_path: Vec<Coord>,
    pub king_destination: Coord,
    pub rook_destination: Coord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioRules {
    pub pawn_forward_y: BTreeMap<Player, i8>,
    pub require_standard_armies: bool,
    pub allow_pawn_double_step: bool,
    pub allow_en_passant: bool,
    pub establishment_cycles: u8,
    pub production_cycles: u8,
    pub promotion_cycles: u8,
    pub development_resets_when_interrupted: bool,
}

impl Default for ScenarioRules {
    fn default() -> Self {
        Self {
            pawn_forward_y: BTreeMap::from([(Player::North, 1), (Player::South, -1)]),
            require_standard_armies: true,
            allow_pawn_double_step: true,
            allow_en_passant: true,
            establishment_cycles: 3,
            production_cycles: 3,
            promotion_cycles: 1,
            development_resets_when_interrupted: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioMetadata {
    pub name: String,
    pub description: String,
    pub expected_minutes: (u16, u16),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioDefinition {
    pub schema_version: u16,
    pub id: String,
    pub metadata: ScenarioMetadata,
    pub board: BoardSize,
    #[serde(default)]
    pub terrain: BTreeMap<Coord, TileTerrain>,
    #[serde(default)]
    pub edges: BTreeMap<Edge, EdgeKind>,
    pub deployments: Vec<Deployment>,
    #[serde(default)]
    pub settlements: Vec<SettlementSite>,
    #[serde(default)]
    pub promotion_sites: Vec<PromotionSite>,
    #[serde(default)]
    pub fortifications: Vec<Fortification>,
    #[serde(default)]
    pub castling_routes: Vec<CastlingRoute>,
    pub rules: ScenarioRules,
}

impl ScenarioDefinition {
    /// Checks schema compatibility and all invariants discoverable without
    /// executing a match.
    ///
    /// # Errors
    ///
    /// Returns every discovered validation error so map authors can correct a
    /// scenario in one pass.
    pub fn validate(&self) -> Result<(), Vec<ScenarioError>> {
        let mut errors = Vec::new();
        if self.schema_version != SCENARIO_SCHEMA_VERSION {
            errors.push(ScenarioError::UnsupportedSchema {
                found: self.schema_version,
                supported: SCENARIO_SCHEMA_VERSION,
            });
        }
        if self.id.trim().is_empty() {
            errors.push(ScenarioError::EmptyId { kind: "scenario" });
        }
        if self.board.width < 8 || self.board.height < 8 {
            errors.push(ScenarioError::BoardTooSmall(self.board));
        }
        if self.metadata.expected_minutes.0 == 0
            || self.metadata.expected_minutes.0 > self.metadata.expected_minutes.1
        {
            errors.push(ScenarioError::InvalidExpectedDuration);
        }

        let mut occupied = BTreeSet::new();
        let mut kings = BTreeMap::from([(Player::North, 0_u8), (Player::South, 0_u8)]);
        for deployment in &self.deployments {
            if !deployment.at.is_within(self.board) {
                errors.push(ScenarioError::OutOfBounds {
                    kind: "deployment",
                    at: deployment.at,
                });
            }
            if !occupied.insert(deployment.at) {
                errors.push(ScenarioError::DuplicateDeployment(deployment.at));
            }
            if self.terrain.get(&deployment.at) == Some(&TileTerrain::Mountain) {
                errors.push(ScenarioError::DeploymentOnMountain(deployment.at));
            }
            if deployment.kind == PieceKind::King {
                *kings.entry(deployment.player).or_default() += 1;
            }
        }
        for player in Player::ALL {
            if kings[&player] != 1 {
                errors.push(ScenarioError::KingCount {
                    player,
                    found: kings[&player],
                });
            }
            match self.rules.pawn_forward_y.get(&player) {
                Some(-1 | 1) => {}
                _ => errors.push(ScenarioError::InvalidPawnDirection(player)),
            }
            if self.rules.require_standard_armies {
                validate_standard_army(self, player, &mut errors);
            }
        }

        for at in self.terrain.keys() {
            if !at.is_within(self.board) {
                errors.push(ScenarioError::OutOfBounds {
                    kind: "terrain",
                    at: *at,
                });
            }
        }
        for edge in self.edges.keys() {
            if !edge.first.is_within(self.board) || !edge.second.is_within(self.board) {
                errors.push(ScenarioError::OutOfBounds {
                    kind: "edge",
                    at: edge.first,
                });
            } else if !edge.is_orthogonally_adjacent() {
                errors.push(ScenarioError::InvalidEdge(*edge));
            }
        }
        validate_sites(self, &mut errors);
        validate_castling(self, &mut errors);
        if self.rules.establishment_cycles == 0
            || self.rules.production_cycles == 0
            || self.rules.promotion_cycles == 0
        {
            errors.push(ScenarioError::ZeroCycleThreshold);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_sites(scenario: &ScenarioDefinition, errors: &mut Vec<ScenarioError>) {
    let mut ids = BTreeSet::new();
    let mut coordinates = BTreeMap::new();
    for (kind, id, at) in scenario
        .settlements
        .iter()
        .map(|site| ("settlement", site.id.as_str(), site.at))
        .chain(
            scenario
                .promotion_sites
                .iter()
                .map(|site| ("promotion site", site.id.as_str(), site.at)),
        )
    {
        if id.trim().is_empty() {
            errors.push(ScenarioError::EmptyId { kind });
        } else if !ids.insert(id) {
            errors.push(ScenarioError::DuplicateId(id.to_owned()));
        }
        if !at.is_within(scenario.board) {
            errors.push(ScenarioError::OutOfBounds { kind, at });
        }
        if let Some(previous) = coordinates.insert(at, kind) {
            errors.push(ScenarioError::OverlappingSites {
                at,
                first: previous,
                second: kind,
            });
        }
    }
}

fn validate_castling(scenario: &ScenarioDefinition, errors: &mut Vec<ScenarioError>) {
    let starts: BTreeSet<_> = scenario
        .deployments
        .iter()
        .map(|piece| (piece.player, piece.kind, piece.at))
        .collect();
    let mut ids = BTreeSet::new();
    for route in &scenario.castling_routes {
        if !ids.insert(route.id.as_str()) {
            errors.push(ScenarioError::DuplicateId(route.id.clone()));
        }
        if !starts.contains(&(route.player, PieceKind::King, route.king_start))
            || !starts.contains(&(route.player, PieceKind::Rook, route.rook_start))
        {
            errors.push(ScenarioError::InvalidCastlingParticipants(route.id.clone()));
        }
        for at in route
            .king_path
            .iter()
            .copied()
            .chain([route.king_destination, route.rook_destination])
        {
            if !at.is_within(scenario.board) {
                errors.push(ScenarioError::OutOfBounds {
                    kind: "castling route",
                    at,
                });
            }
        }
        let mut previous = route.king_start;
        for at in route.king_path.iter().copied() {
            let dx = previous.x.abs_diff(at.x);
            let dy = previous.y.abs_diff(at.y);
            if dx > 1 || dy > 1 || dx + dy == 0 {
                errors.push(ScenarioError::InvalidCastlingPath(route.id.clone()));
                break;
            }
            previous = at;
        }
        if route.king_path.last().copied() != Some(route.king_destination) {
            errors.push(ScenarioError::InvalidCastlingPath(route.id.clone()));
        }
    }
}

fn validate_standard_army(
    scenario: &ScenarioDefinition,
    player: Player,
    errors: &mut Vec<ScenarioError>,
) {
    let expected = [
        (PieceKind::King, 1),
        (PieceKind::Queen, 1),
        (PieceKind::Rook, 2),
        (PieceKind::Bishop, 2),
        (PieceKind::Knight, 2),
        (PieceKind::Pawn, 8),
    ];
    for (kind, expected_count) in expected {
        let found = scenario
            .deployments
            .iter()
            .filter(|piece| piece.player == player && piece.kind == kind)
            .count();
        if found != expected_count {
            errors.push(ScenarioError::PieceCount {
                player,
                kind,
                expected: expected_count,
                found,
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScenarioError {
    #[error("scenario schema {found} is unsupported; this build supports {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("{kind} id must not be empty")]
    EmptyId { kind: &'static str },
    #[error("board {0:?} is too small; both dimensions must be at least 8")]
    BoardTooSmall(BoardSize),
    #[error("expected match duration must be non-zero and ordered")]
    InvalidExpectedDuration,
    #[error("{kind} at {at:?} is outside the board")]
    OutOfBounds { kind: &'static str, at: Coord },
    #[error("multiple pieces deploy at {0:?}")]
    DuplicateDeployment(Coord),
    #[error("a piece deploys on mountain terrain at {0:?}")]
    DeploymentOnMountain(Coord),
    #[error("{player:?} must have exactly one King, found {found}")]
    KingCount { player: Player, found: u8 },
    #[error("{0:?} must have a Pawn direction of -1 or 1")]
    InvalidPawnDirection(Player),
    #[error("edge {0:?} must join two orthogonally adjacent in-bounds squares")]
    InvalidEdge(Edge),
    #[error("duplicate site or route id {0:?}")]
    DuplicateId(String),
    #[error("{first} and {second} overlap at {at:?}")]
    OverlappingSites {
        at: Coord,
        first: &'static str,
        second: &'static str,
    },
    #[error("castling route {0:?} does not reference its player's deployed King and Rook")]
    InvalidCastlingParticipants(String),
    #[error("castling route {0:?} must contain adjacent King steps and end at its destination")]
    InvalidCastlingPath(String),
    #[error("{player:?} must deploy {expected} {kind:?} piece(s), found {found}")]
    PieceCount {
        player: Player,
        kind: PieceKind,
        expected: usize,
        found: usize,
    },
    #[error("development, production, and promotion thresholds must be non-zero")]
    ZeroCycleThreshold,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "test".to_owned(),
            metadata: ScenarioMetadata {
                name: "Test".to_owned(),
                description: String::new(),
                expected_minutes: (30, 45),
            },
            board: BoardSize {
                width: 16,
                height: 16,
            },
            terrain: BTreeMap::new(),
            edges: BTreeMap::new(),
            deployments: vec![
                Deployment {
                    player: Player::North,
                    kind: PieceKind::King,
                    at: Coord::new(7, 1),
                },
                Deployment {
                    player: Player::South,
                    kind: PieceKind::King,
                    at: Coord::new(7, 14),
                },
            ],
            settlements: vec![],
            promotion_sites: vec![],
            fortifications: vec![],
            castling_routes: vec![],
            rules: ScenarioRules::default(),
        }
    }

    #[test]
    fn validates_minimal_scenario() {
        let mut scenario = minimal_scenario();
        scenario.rules.require_standard_armies = false;
        assert_eq!(scenario.validate(), Ok(()));
    }

    #[test]
    fn reports_multiple_errors_at_once() {
        let mut scenario = minimal_scenario();
        scenario.rules.require_standard_armies = false;
        scenario.schema_version = 99;
        scenario.deployments[0].at = Coord::new(99, 99);
        scenario.rules.establishment_cycles = 0;
        let errors = scenario.validate().expect_err("scenario should be invalid");
        assert!(errors.len() >= 3);
    }

    #[test]
    fn canonicalizes_edges() {
        let a = Coord::new(1, 2);
        let b = Coord::new(1, 3);
        assert_eq!(Edge::new(a, b), Edge::new(b, a));
    }
}
