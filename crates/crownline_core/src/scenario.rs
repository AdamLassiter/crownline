use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SCENARIO_SCHEMA_VERSION: u16 = 1;
const MIN_BOARD_DIMENSION: u16 = 8;
const MAX_BOARD_DIMENSION: u16 = 64;
const MAX_EXPECTED_MATCH_MINUTES: u16 = 24 * 60;
const MAX_REALM_CYCLES: u8 = 20;

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
pub struct KeepDefinition {
    pub id: String,
    pub owner: Player,
    pub tiles: BTreeSet<Coord>,
    pub gates: BTreeSet<Edge>,
    pub fortification_ids: BTreeSet<String>,
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
    pub army_setup: ArmySetup,
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
            army_setup: ArmySetup::Standard,
            allow_pawn_double_step: true,
            allow_en_passant: true,
            establishment_cycles: 3,
            production_cycles: 3,
            promotion_cycles: 1,
            development_resets_when_interrupted: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmySetup {
    Standard,
    Custom,
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
    pub keeps: Vec<KeepDefinition>,
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
        validate_header(self, &mut errors);

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
            if self.rules.army_setup == ArmySetup::Standard {
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
            } else if *edge != Edge::new(edge.first, edge.second) {
                errors.push(ScenarioError::NonCanonicalEdge(*edge));
            }
        }
        let mut ids = BTreeMap::new();
        validate_sites(self, &mut ids, &mut errors);
        validate_keeps_and_fortifications(self, &mut ids, &mut errors);
        validate_castling(self, &mut ids, &mut errors);
        validate_rule_configuration(self, &mut errors);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_header(scenario: &ScenarioDefinition, errors: &mut Vec<ScenarioError>) {
    if scenario.schema_version != SCENARIO_SCHEMA_VERSION {
        errors.push(ScenarioError::UnsupportedSchema {
            found: scenario.schema_version,
            supported: SCENARIO_SCHEMA_VERSION,
        });
    }
    if scenario.id.trim().is_empty() {
        errors.push(ScenarioError::EmptyId { kind: "scenario" });
    }
    if scenario.metadata.name.trim().is_empty() {
        errors.push(ScenarioError::EmptyField("metadata.name"));
    }
    if !(MIN_BOARD_DIMENSION..=MAX_BOARD_DIMENSION).contains(&scenario.board.width)
        || !(MIN_BOARD_DIMENSION..=MAX_BOARD_DIMENSION).contains(&scenario.board.height)
    {
        errors.push(ScenarioError::BoardSizeOutOfRange(scenario.board));
    }
    if scenario.metadata.expected_minutes.0 == 0
        || scenario.metadata.expected_minutes.0 > scenario.metadata.expected_minutes.1
        || scenario.metadata.expected_minutes.1 > MAX_EXPECTED_MATCH_MINUTES
    {
        errors.push(ScenarioError::InvalidExpectedDuration);
    }
}

fn validate_rule_configuration(scenario: &ScenarioDefinition, errors: &mut Vec<ScenarioError>) {
    if !(1..=MAX_REALM_CYCLES).contains(&scenario.rules.establishment_cycles)
        || !(1..=MAX_REALM_CYCLES).contains(&scenario.rules.production_cycles)
        || !(1..=MAX_REALM_CYCLES).contains(&scenario.rules.promotion_cycles)
    {
        errors.push(ScenarioError::CycleThresholdOutOfRange {
            minimum: 1,
            maximum: MAX_REALM_CYCLES,
        });
    }
    if scenario.rules.pawn_forward_y.get(&Player::North)
        == scenario.rules.pawn_forward_y.get(&Player::South)
    {
        errors.push(ScenarioError::PawnDirectionsNotOpposed);
    }
}

fn validate_sites(
    scenario: &ScenarioDefinition,
    ids: &mut BTreeMap<String, &'static str>,
    errors: &mut Vec<ScenarioError>,
) {
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
        register_id(ids, kind, id, errors);
        if !at.is_within(scenario.board) {
            errors.push(ScenarioError::OutOfBounds { kind, at });
        }
        if scenario.terrain.get(&at) == Some(&TileTerrain::Mountain) {
            errors.push(ScenarioError::SiteOnMountain { kind, at });
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

fn validate_keeps_and_fortifications(
    scenario: &ScenarioDefinition,
    ids: &mut BTreeMap<String, &'static str>,
    errors: &mut Vec<ScenarioError>,
) {
    validate_fortifications(scenario, ids, errors);
    validate_keeps(scenario, ids, errors);
    validate_fortification_links(scenario, errors);
    validate_standard_keeps(scenario, errors);
}

fn validate_fortifications(
    scenario: &ScenarioDefinition,
    ids: &mut BTreeMap<String, &'static str>,
    errors: &mut Vec<ScenarioError>,
) {
    for fortification in &scenario.fortifications {
        register_id(ids, "fortification", &fortification.id, errors);
        if !fortification.tower.is_within(scenario.board) {
            errors.push(ScenarioError::OutOfBounds {
                kind: "fortification tower",
                at: fortification.tower,
            });
        } else if scenario.terrain.get(&fortification.tower) == Some(&TileTerrain::Mountain) {
            errors.push(ScenarioError::SiteOnMountain {
                kind: "fortification tower",
                at: fortification.tower,
            });
        }
        if !fortification.projected_wall.is_orthogonally_adjacent()
            || ![
                fortification.projected_wall.first,
                fortification.projected_wall.second,
            ]
            .contains(&fortification.tower)
            || scenario.edges.get(&fortification.projected_wall) != Some(&EdgeKind::Wall)
        {
            errors.push(ScenarioError::InvalidFortificationWall {
                id: fortification.id.clone(),
                tower: fortification.tower,
                wall: fortification.projected_wall,
            });
        }
    }
}

fn validate_keeps(
    scenario: &ScenarioDefinition,
    ids: &mut BTreeMap<String, &'static str>,
    errors: &mut Vec<ScenarioError>,
) {
    let fortifications: BTreeMap<_, _> = scenario
        .fortifications
        .iter()
        .map(|fortification| (fortification.id.as_str(), fortification))
        .collect();
    let mut claimed_keep_tiles = BTreeMap::new();

    for keep in &scenario.keeps {
        register_id(ids, "keep", &keep.id, errors);
        if keep.tiles.is_empty() {
            errors.push(ScenarioError::EmptyKeep(keep.id.clone()));
        }
        for tile in &keep.tiles {
            if !tile.is_within(scenario.board) {
                errors.push(ScenarioError::OutOfBounds {
                    kind: "keep tile",
                    at: *tile,
                });
            } else if scenario.terrain.get(tile) == Some(&TileTerrain::Mountain) {
                errors.push(ScenarioError::SiteOnMountain {
                    kind: "keep tile",
                    at: *tile,
                });
            }
            if let Some(other) = claimed_keep_tiles.insert(*tile, keep.id.as_str()) {
                errors.push(ScenarioError::OverlappingKeeps {
                    at: *tile,
                    first: other.to_owned(),
                    second: keep.id.clone(),
                });
            }
        }
        if scenario.rules.army_setup == ArmySetup::Standard && keep.gates.len() < 2 {
            errors.push(ScenarioError::InsufficientKeepExits {
                keep: keep.id.clone(),
                found: keep.gates.len(),
            });
        }
        for gate in &keep.gates {
            let first_inside = keep.tiles.contains(&gate.first);
            let second_inside = keep.tiles.contains(&gate.second);
            if !gate.is_orthogonally_adjacent()
                || first_inside == second_inside
                || scenario.edges.get(gate) != Some(&EdgeKind::Gate)
            {
                errors.push(ScenarioError::InvalidKeepGate {
                    keep: keep.id.clone(),
                    gate: *gate,
                });
            }
        }
        for fortification_id in &keep.fortification_ids {
            match fortifications.get(fortification_id.as_str()) {
                Some(fortification)
                    if fortification.owner == keep.owner
                        && keep.tiles.contains(&fortification.tower) => {}
                Some(_) => errors.push(ScenarioError::InvalidKeepFortificationLink {
                    keep: keep.id.clone(),
                    fortification: fortification_id.clone(),
                }),
                None => errors.push(ScenarioError::UnknownFortification {
                    keep: keep.id.clone(),
                    fortification: fortification_id.clone(),
                }),
            }
        }
    }
}

fn validate_fortification_links(scenario: &ScenarioDefinition, errors: &mut Vec<ScenarioError>) {
    for fortification in &scenario.fortifications {
        let linked = scenario.keeps.iter().any(|keep| {
            keep.owner == fortification.owner
                && keep.fortification_ids.contains(&fortification.id)
                && keep.tiles.contains(&fortification.tower)
        });
        if !linked {
            errors.push(ScenarioError::UnlinkedFortification(
                fortification.id.clone(),
            ));
        }
    }
}

fn validate_standard_keeps(scenario: &ScenarioDefinition, errors: &mut Vec<ScenarioError>) {
    if scenario.rules.army_setup != ArmySetup::Standard {
        return;
    }
    for player in Player::ALL {
        let player_keeps: Vec<_> = scenario
            .keeps
            .iter()
            .filter(|keep| keep.owner == player)
            .collect();
        if player_keeps.len() != 1 {
            errors.push(ScenarioError::KeepCount {
                player,
                found: player_keeps.len(),
            });
            continue;
        }
        let keep = player_keeps[0];
        for deployment in scenario
            .deployments
            .iter()
            .filter(|piece| piece.player == player)
        {
            if !keep.tiles.contains(&deployment.at) {
                errors.push(ScenarioError::DeploymentOutsideKeep {
                    player,
                    at: deployment.at,
                    keep: keep.id.clone(),
                });
            }
        }
    }
}

fn register_id(
    ids: &mut BTreeMap<String, &'static str>,
    kind: &'static str,
    id: &str,
    errors: &mut Vec<ScenarioError>,
) {
    if id.trim().is_empty() {
        errors.push(ScenarioError::EmptyId { kind });
    } else if let Some(previous) = ids.insert(id.to_owned(), kind) {
        errors.push(ScenarioError::DuplicateId {
            id: id.to_owned(),
            first: previous,
            second: kind,
        });
    }
}

fn validate_castling(
    scenario: &ScenarioDefinition,
    ids: &mut BTreeMap<String, &'static str>,
    errors: &mut Vec<ScenarioError>,
) {
    let starts: BTreeSet<_> = scenario
        .deployments
        .iter()
        .map(|piece| (piece.player, piece.kind, piece.at))
        .collect();
    for route in &scenario.castling_routes {
        register_id(ids, "castling route", &route.id, errors);
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
        let mut visited = BTreeSet::from([route.king_start]);
        for at in route.king_path.iter().copied() {
            let dx = previous.x.abs_diff(at.x);
            let dy = previous.y.abs_diff(at.y);
            if dx > 1
                || dy > 1
                || dx + dy == 0
                || !visited.insert(at)
                || scenario.terrain.get(&at) == Some(&TileTerrain::Mountain)
                || !castling_step_is_open(scenario, previous, at)
            {
                errors.push(ScenarioError::InvalidCastlingPath(route.id.clone()));
                break;
            }
            previous = at;
        }
        if route.king_path.last().copied() != Some(route.king_destination)
            || route.king_destination == route.rook_destination
            || !rook_castling_path_is_open(scenario, route)
        {
            errors.push(ScenarioError::InvalidCastlingPath(route.id.clone()));
        }
    }
}

fn castling_step_is_open(scenario: &ScenarioDefinition, from: Coord, to: Coord) -> bool {
    let dx = from.x.abs_diff(to.x);
    let dy = from.y.abs_diff(to.y);
    let edge_open = |edge: Edge| {
        !matches!(
            scenario.edges.get(&edge),
            Some(EdgeKind::River | EdgeKind::Wall)
        )
    };
    if dx == 1 && dy == 1 {
        let horizontal = Coord::new(to.x, from.y);
        let vertical = Coord::new(from.x, to.y);
        return [
            Edge::new(from, horizontal),
            Edge::new(from, vertical),
            Edge::new(horizontal, to),
            Edge::new(vertical, to),
        ]
        .into_iter()
        .all(edge_open);
    }
    edge_open(Edge::new(from, to))
}

fn rook_castling_path_is_open(scenario: &ScenarioDefinition, route: &CastlingRoute) -> bool {
    let (dx, dy) = match (
        route.rook_start.x.cmp(&route.rook_destination.x),
        route.rook_start.y.cmp(&route.rook_destination.y),
    ) {
        (std::cmp::Ordering::Less, std::cmp::Ordering::Equal) => (1_i8, 0_i8),
        (std::cmp::Ordering::Greater, std::cmp::Ordering::Equal) => (-1, 0),
        (std::cmp::Ordering::Equal, std::cmp::Ordering::Less) => (0, 1),
        (std::cmp::Ordering::Equal, std::cmp::Ordering::Greater) => (0, -1),
        _ => return false,
    };
    let mut current = route.rook_start;
    loop {
        let Some(next) = checked_offset(current, dx, dy) else {
            return false;
        };
        if !next.is_within(scenario.board)
            || scenario.terrain.get(&next) == Some(&TileTerrain::Mountain)
            || !castling_step_is_open(scenario, current, next)
        {
            return false;
        }
        if next == route.rook_destination {
            return true;
        }
        if scenario.terrain.get(&next) == Some(&TileTerrain::Forest) {
            return false;
        }
        current = next;
    }
}

fn checked_offset(coord: Coord, dx: i8, dy: i8) -> Option<Coord> {
    let x = if dx.is_negative() {
        coord.x.checked_sub(u16::from(dx.unsigned_abs()))?
    } else {
        coord.x.checked_add(u16::from(dx.unsigned_abs()))?
    };
    let y = if dy.is_negative() {
        coord.y.checked_sub(u16::from(dy.unsigned_abs()))?
    } else {
        coord.y.checked_add(u16::from(dy.unsigned_abs()))?
    };
    Some(Coord::new(x, y))
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
    #[error("required field {0} must not be empty")]
    EmptyField(&'static str),
    #[error("board {0:?} is outside the supported 8..=64 range")]
    BoardSizeOutOfRange(BoardSize),
    #[error("expected match duration must be non-zero, ordered, and at most 1440 minutes")]
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
    #[error("edge {0:?} must be stored in canonical coordinate order")]
    NonCanonicalEdge(Edge),
    #[error("{kind} at {at:?} cannot occupy mountain terrain")]
    SiteOnMountain { kind: &'static str, at: Coord },
    #[error("duplicate id {id:?} is used by both {first} and {second}")]
    DuplicateId {
        id: String,
        first: &'static str,
        second: &'static str,
    },
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
    #[error("Keep {0:?} must contain at least one tile")]
    EmptyKeep(String),
    #[error("Keeps {first:?} and {second:?} overlap at {at:?}")]
    OverlappingKeeps {
        at: Coord,
        first: String,
        second: String,
    },
    #[error("standard Keep {keep:?} needs at least two exits, found {found}")]
    InsufficientKeepExits { keep: String, found: usize },
    #[error("Keep {keep:?} gate {gate:?} must be a boundary edge marked as a gate")]
    InvalidKeepGate { keep: String, gate: Edge },
    #[error("Keep {keep:?} references unknown fortification {fortification:?}")]
    UnknownFortification { keep: String, fortification: String },
    #[error(
        "Keep {keep:?} cannot link fortification {fortification:?} with a different owner or external tower"
    )]
    InvalidKeepFortificationLink { keep: String, fortification: String },
    #[error("fortification {0:?} is not linked to its owner's Keep")]
    UnlinkedFortification(String),
    #[error(
        "fortification {id:?} tower {tower:?} must touch projected wall {wall:?}, which must be a wall edge"
    )]
    InvalidFortificationWall {
        id: String,
        tower: Coord,
        wall: Edge,
    },
    #[error("{player:?} must have exactly one Keep, found {found}")]
    KeepCount { player: Player, found: usize },
    #[error("{player:?} deployment at {at:?} lies outside Keep {keep:?}")]
    DeploymentOutsideKeep {
        player: Player,
        at: Coord,
        keep: String,
    },
    #[error("{player:?} must deploy {expected} {kind:?} piece(s), found {found}")]
    PieceCount {
        player: Player,
        kind: PieceKind,
        expected: usize,
        found: usize,
    },
    #[error("development, production, and promotion thresholds must be in {minimum}..={maximum}")]
    CycleThresholdOutOfRange { minimum: u8, maximum: u8 },
    #[error("North and South Pawn directions must be opposed")]
    PawnDirectionsNotOpposed,
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK_RANK: [PieceKind; 8] = [
        PieceKind::Rook,
        PieceKind::Knight,
        PieceKind::Bishop,
        PieceKind::Queen,
        PieceKind::King,
        PieceKind::Bishop,
        PieceKind::Knight,
        PieceKind::Rook,
    ];

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
            keeps: vec![],
            fortifications: vec![],
            castling_routes: vec![],
            rules: ScenarioRules::default(),
        }
    }

    fn standard_scenario() -> ScenarioDefinition {
        let mut deployments = Vec::new();
        for player in Player::ALL {
            let (back_y, pawn_y) = match player {
                Player::North => (0, 1),
                Player::South => (15, 14),
            };
            for (offset, kind) in BACK_RANK.into_iter().enumerate() {
                deployments.push(Deployment {
                    player,
                    kind,
                    at: Coord::new(4 + u16::try_from(offset).unwrap(), back_y),
                });
                deployments.push(Deployment {
                    player,
                    kind: PieceKind::Pawn,
                    at: Coord::new(4 + u16::try_from(offset).unwrap(), pawn_y),
                });
            }
        }

        let north_gates = BTreeSet::from([
            Edge::new(Coord::new(4, 1), Coord::new(4, 2)),
            Edge::new(Coord::new(11, 1), Coord::new(11, 2)),
        ]);
        let south_gates = BTreeSet::from([
            Edge::new(Coord::new(4, 13), Coord::new(4, 14)),
            Edge::new(Coord::new(11, 13), Coord::new(11, 14)),
        ]);
        let edges = north_gates
            .iter()
            .chain(&south_gates)
            .copied()
            .map(|edge| (edge, EdgeKind::Gate))
            .collect();
        let keep_tiles = |rows: [u16; 2]| {
            rows.into_iter()
                .flat_map(|y| (4..=11).map(move |x| Coord::new(x, y)))
                .collect()
        };

        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "standard-fixture".to_owned(),
            metadata: ScenarioMetadata {
                name: "Standard fixture".to_owned(),
                description: String::new(),
                expected_minutes: (60, 90),
            },
            board: BoardSize {
                width: 16,
                height: 16,
            },
            terrain: BTreeMap::new(),
            edges,
            deployments,
            settlements: vec![SettlementSite {
                id: "central-town".to_owned(),
                at: Coord::new(7, 7),
            }],
            promotion_sites: vec![PromotionSite {
                id: "central-court".to_owned(),
                at: Coord::new(8, 8),
            }],
            keeps: vec![
                KeepDefinition {
                    id: "north-keep".to_owned(),
                    owner: Player::North,
                    tiles: keep_tiles([0, 1]),
                    gates: north_gates,
                    fortification_ids: BTreeSet::new(),
                },
                KeepDefinition {
                    id: "south-keep".to_owned(),
                    owner: Player::South,
                    tiles: keep_tiles([14, 15]),
                    gates: south_gates,
                    fortification_ids: BTreeSet::new(),
                },
            ],
            fortifications: vec![],
            castling_routes: vec![],
            rules: ScenarioRules::default(),
        }
    }

    #[test]
    fn validates_minimal_scenario() {
        let mut scenario = minimal_scenario();
        scenario.rules.army_setup = ArmySetup::Custom;
        assert_eq!(scenario.validate(), Ok(()));
    }

    #[test]
    fn reports_multiple_errors_at_once() {
        let mut scenario = minimal_scenario();
        scenario.rules.army_setup = ArmySetup::Custom;
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

    #[test]
    fn keep_schema_round_trips_through_ron() {
        let mut scenario = minimal_scenario();
        scenario.rules.army_setup = ArmySetup::Custom;
        scenario.keeps.push(KeepDefinition {
            id: "north-keep".to_owned(),
            owner: Player::North,
            tiles: BTreeSet::from([Coord::new(7, 1)]),
            gates: BTreeSet::from([Edge::new(Coord::new(7, 1), Coord::new(7, 2))]),
            fortification_ids: BTreeSet::new(),
        });
        let encoded = ron::to_string(&scenario).expect("scenario serializes");
        let decoded: ScenarioDefinition = ron::from_str(&encoded).expect("scenario deserializes");
        assert_eq!(decoded, scenario);
    }

    #[test]
    fn validates_complete_standard_fixture() {
        assert_eq!(standard_scenario().validate(), Ok(()));
    }

    #[test]
    fn reports_keep_fortification_and_deployment_errors() {
        let mut scenario = standard_scenario();
        scenario.keeps[0].gates.pop_first();
        scenario.deployments[0].at = Coord::new(0, 5);
        scenario.fortifications.push(Fortification {
            id: "orphan-tower".to_owned(),
            owner: Player::North,
            tower: Coord::new(4, 0),
            projected_wall: Edge::new(Coord::new(4, 0), Coord::new(3, 0)),
        });
        scenario.edges.insert(
            Edge::new(Coord::new(4, 0), Coord::new(3, 0)),
            EdgeKind::Wall,
        );

        let errors = scenario.validate().expect_err("fixture should be invalid");
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, ScenarioError::InsufficientKeepExits { .. }))
        );
        assert!(errors.iter().any(|error| matches!(
            error,
            ScenarioError::DeploymentOutsideKeep { at, .. } if *at == Coord::new(0, 5)
        )));
        assert!(errors.iter().any(|error| matches!(
            error,
            ScenarioError::UnlinkedFortification(id) if id == "orphan-tower"
        )));
    }

    #[test]
    fn reports_duplicate_ids_timing_directions_and_site_terrain() {
        let mut scenario = standard_scenario();
        scenario.settlements[0].id = "north-keep".to_owned();
        scenario.rules.establishment_cycles = MAX_REALM_CYCLES + 1;
        scenario.rules.pawn_forward_y.insert(Player::South, 1);
        let site = scenario.promotion_sites[0].at;
        scenario.terrain.insert(site, TileTerrain::Mountain);

        let errors = scenario.validate().expect_err("fixture should be invalid");
        assert!(errors.iter().any(|error| matches!(
            error,
            ScenarioError::DuplicateId { id, .. } if id == "north-keep"
        )));
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, ScenarioError::CycleThresholdOutOfRange { .. }))
        );
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, ScenarioError::PawnDirectionsNotOpposed))
        );
        assert!(errors.iter().any(|error| matches!(
            error,
            ScenarioError::SiteOnMountain { at, .. } if *at == site
        )));
    }

    #[test]
    fn rejects_castling_path_through_blocking_edge() {
        let mut scenario = standard_scenario();
        let wall = Edge::new(Coord::new(8, 0), Coord::new(9, 0));
        scenario.edges.insert(wall, EdgeKind::Wall);
        scenario.castling_routes.push(CastlingRoute {
            id: "north-east-castle".to_owned(),
            player: Player::North,
            king_start: Coord::new(8, 0),
            rook_start: Coord::new(11, 0),
            king_path: vec![Coord::new(9, 0), Coord::new(10, 0)],
            king_destination: Coord::new(10, 0),
            rook_destination: Coord::new(9, 0),
        });

        let errors = scenario.validate().expect_err("route crosses a wall");
        assert!(errors.iter().any(|error| matches!(
            error,
            ScenarioError::InvalidCastlingPath(id) if id == "north-east-castle"
        )));
    }
}
