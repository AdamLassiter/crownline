//! Deterministic chess-clock transitions driven by host-supplied elapsed time.

use serde::{Deserialize, Serialize};

use crate::{
    rules::{Transition, TransitionEvent, apply_action},
    scenario::{Player, ScenarioDefinition},
    state::{Action, ClockState, MatchOutcome, MatchState, OutcomeReason, TransitionError},
};

pub const MIN_BASE_MINUTES: u16 = 1;
pub const MAX_BASE_MINUTES: u16 = 180;
pub const MAX_INCREMENT_SECONDS: u8 = 60;

/// Host-facing clock configuration. Untimed matches omit this value entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockSettings {
    pub base_minutes: u16,
    pub increment_seconds: u8,
}

/// Creates the initial canonical clock values without consulting a clock source.
///
/// # Errors
///
/// Returns a typed error for invalid bounds or a match that has already started.
pub fn start_clocks(
    state: &MatchState,
    settings: ClockSettings,
) -> Result<MatchState, TransitionError> {
    if !(MIN_BASE_MINUTES..=MAX_BASE_MINUTES).contains(&settings.base_minutes) {
        return Err(TransitionError::InvalidClockBase(settings.base_minutes));
    }
    if settings.increment_seconds > MAX_INCREMENT_SECONDS {
        return Err(TransitionError::InvalidClockIncrement(
            settings.increment_seconds,
        ));
    }
    if state.clocks.is_some() {
        return Err(TransitionError::ClocksAlreadyStarted);
    }
    if state.revision != 0 || state.turn_number != 1 || state.outcome.is_some() {
        return Err(TransitionError::ClocksMustStartWithMatch);
    }

    let base_millis = u64::from(settings.base_minutes) * 60_000;
    let mut next = state.clone();
    next.clocks = Some(ClockState {
        north_millis: base_millis,
        south_millis: base_millis,
        increment_millis: u64::from(settings.increment_seconds) * 1_000,
    });
    Ok(next)
}

/// Charges explicit elapsed time to the active player.
///
/// Hosts decide how elapsed time is measured: local play supplies monotonic
/// runtime deltas, while a server may derive a delta from a persisted deadline.
///
/// # Errors
///
/// Returns an error when the match is already terminal or its revision overflows.
pub fn advance_clock(
    state: &MatchState,
    elapsed_millis: u64,
) -> Result<Transition, TransitionError> {
    if state.outcome.is_some() {
        return Err(TransitionError::MatchFinished);
    }
    let Some(clocks) = state.clocks else {
        return Ok(Transition {
            state: state.clone(),
            events: Vec::new(),
        });
    };

    let remaining = remaining_for(clocks, state.active_player);
    let charged_millis = elapsed_millis.min(remaining);
    let mut next = state.clone();
    let next_clocks = next.clocks.get_or_insert(clocks);
    *remaining_for_mut(next_clocks, state.active_player) = remaining - charged_millis;

    let mut events = Vec::new();
    if charged_millis > 0 {
        events.push(TransitionEvent::ClockAdvanced {
            player: state.active_player,
            elapsed_millis: charged_millis,
            remaining_millis: remaining - charged_millis,
        });
    }
    if elapsed_millis >= remaining {
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(TransitionError::RevisionOverflow)?;
        let outcome = MatchOutcome {
            winner: Some(state.active_player.opponent()),
            reason: OutcomeReason::Timeout,
        };
        next.outcome = Some(outcome);
        events.push(TransitionEvent::MatchEnded { outcome });
    }

    Ok(Transition {
        state: next,
        events,
    })
}

/// Charges the clock before applying an action and applies increment after an
/// accepted Move or Hold. Clock expiration wins over the submitted action.
///
/// # Errors
///
/// Returns clock or action transition errors without mutating the source state.
pub fn apply_timed_action(
    scenario: &ScenarioDefinition,
    state: &MatchState,
    action: &Action,
    elapsed_millis: u64,
) -> Result<Transition, TransitionError> {
    let clock_transition = advance_clock(state, elapsed_millis)?;
    if clock_transition.state.outcome.is_some() {
        return Ok(clock_transition);
    }

    let mut action_transition = apply_action(scenario, &clock_transition.state, action)?;
    let mut events = clock_transition.events;
    events.append(&mut action_transition.events);

    if let Action::Move { player, .. } | Action::Hold { player } = *action
        && let Some(clocks) = action_transition.state.clocks.as_mut()
    {
        let increment_millis = clocks.increment_millis;
        let remaining = remaining_for_mut(clocks, player);
        *remaining = remaining
            .checked_add(increment_millis)
            .ok_or(TransitionError::ClockOverflow)?;
        events.push(TransitionEvent::ClockIncrementApplied {
            player,
            increment_millis,
            remaining_millis: *remaining,
        });
    }

    Ok(Transition {
        state: action_transition.state,
        events,
    })
}

const fn remaining_for(clocks: ClockState, player: Player) -> u64 {
    match player {
        Player::North => clocks.north_millis,
        Player::South => clocks.south_millis,
    }
}

const fn remaining_for_mut(clocks: &mut ClockState, player: Player) -> &mut u64 {
    match player {
        Player::North => &mut clocks.north_millis,
        Player::South => &mut clocks.south_millis,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::scenario::{
        ArmySetup, BoardSize, Deployment, PieceKind, SCENARIO_SCHEMA_VERSION, ScenarioMetadata,
        ScenarioRules,
    };

    use super::*;

    fn scenario() -> ScenarioDefinition {
        ScenarioDefinition {
            schema_version: SCENARIO_SCHEMA_VERSION,
            id: "clock-test".to_owned(),
            metadata: ScenarioMetadata {
                name: "Clock test".to_owned(),
                description: String::new(),
                expected_minutes: (1, 2),
                is_default: false,
            },
            board: BoardSize {
                width: 8,
                height: 8,
            },
            terrain: BTreeMap::new(),
            edges: BTreeMap::new(),
            deployments: vec![
                Deployment {
                    player: Player::North,
                    kind: PieceKind::King,
                    at: crate::scenario::Coord::new(4, 0),
                },
                Deployment {
                    player: Player::South,
                    kind: PieceKind::King,
                    at: crate::scenario::Coord::new(4, 7),
                },
            ],
            settlements: Vec::new(),
            promotion_sites: Vec::new(),
            keeps: Vec::new(),
            fortifications: Vec::new(),
            castling_routes: Vec::new(),
            rules: ScenarioRules {
                army_setup: ArmySetup::Custom,
                ..ScenarioRules::default()
            },
            guided: None,
        }
    }

    fn timed_state() -> (ScenarioDefinition, MatchState) {
        let scenario = scenario();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let state = start_clocks(
            &state,
            ClockSettings {
                base_minutes: 1,
                increment_seconds: 2,
            },
        )
        .unwrap();
        (scenario, state)
    }

    #[test]
    fn settings_enforce_documented_bounds_and_untimed_is_default() {
        let scenario = scenario();
        let state = MatchState::from_scenario(&scenario).unwrap();
        assert_eq!(state.clocks, None);
        for base_minutes in [0, 181] {
            assert!(matches!(
                start_clocks(
                    &state,
                    ClockSettings {
                        base_minutes,
                        increment_seconds: 0,
                    }
                ),
                Err(TransitionError::InvalidClockBase(value)) if value == base_minutes
            ));
        }
        assert!(matches!(
            start_clocks(
                &state,
                ClockSettings {
                    base_minutes: 1,
                    increment_seconds: 61,
                }
            ),
            Err(TransitionError::InvalidClockIncrement(61))
        ));
    }

    #[test]
    fn clock_runs_through_choices_and_move_or_hold_adds_increment() {
        let (scenario, mut state) = timed_state();
        state.phase = crate::state::TurnPhase::ResolvingChoices { queue: Vec::new() };
        let advanced = advance_clock(&state, 1_500).unwrap();
        assert_eq!(advanced.state.clocks.unwrap().south_millis, 58_500);

        state.phase = crate::state::TurnPhase::Command;
        let transition = apply_timed_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::South,
            },
            1_500,
        )
        .unwrap();
        let clocks = transition.state.clocks.unwrap();
        assert_eq!(clocks.south_millis, 60_500);
        assert_eq!(clocks.north_millis, 60_000);
        assert_eq!(transition.state.active_player, Player::North);
    }

    #[test]
    fn expiration_wins_once_over_action_at_the_exact_deadline() {
        let (scenario, mut state) = timed_state();
        state.clocks.as_mut().unwrap().south_millis = 500;
        let transition = apply_timed_action(
            &scenario,
            &state,
            &Action::Hold {
                player: Player::South,
            },
            500,
        )
        .unwrap();

        assert_eq!(transition.state.active_player, Player::South);
        assert_eq!(transition.state.revision, state.revision + 1);
        assert_eq!(transition.state.clocks.unwrap().south_millis, 0);
        assert_eq!(
            transition.state.outcome,
            Some(MatchOutcome {
                winner: Some(Player::North),
                reason: OutcomeReason::Timeout,
            })
        );
        assert!(!transition.events.contains(&TransitionEvent::TurnHeld {
            player: Player::South,
        }));
        assert!(matches!(
            apply_timed_action(
                &scenario,
                &transition.state,
                &Action::Hold {
                    player: Player::South,
                },
                1,
            ),
            Err(TransitionError::MatchFinished)
        ));
    }
}
