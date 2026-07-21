use std::time::{SystemTime, UNIX_EPOCH};

use crownline_core::{
    Action, ActionJournal, AppendOutcome, ClockSettings, IdempotencyKey, JournalError, MatchState,
    ScenarioDefinition, scenario::Player, state::OutcomeReason,
};
use crownline_protocol::{ConnectionState, MatchSnapshot};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::actors::{CommandRejection, CommandTiming, ExecutionError, MatchExecutor};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedClockAuthority {
    pub anchor_unix_millis: u64,
    pub deadline_unix_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedAuthorityTransition {
    pub match_id: Uuid,
    pub state: MatchState,
    pub journal: ActionJournal,
    pub clock: Option<PersistedClockAuthority>,
    pub received_unix_millis: u64,
    pub decided_unix_millis: u64,
}

pub struct AuthoritativeMatch {
    match_id: Uuid,
    scenario: ScenarioDefinition,
    scenario_hash: String,
    state: MatchState,
    journal: ActionJournal,
    clock: Option<PersistedClockAuthority>,
    room_state: ConnectionState,
    prepared: Option<PreparedAuthorityTransition>,
}

impl AuthoritativeMatch {
    /// Creates server authority for a newly started match.
    ///
    /// # Errors
    ///
    /// Returns a fatal description when scenario, journal, or wall-clock metadata is invalid.
    pub fn new(
        match_id: Uuid,
        scenario: ScenarioDefinition,
        clock_settings: Option<ClockSettings>,
        started_at: SystemTime,
    ) -> Result<Self, String> {
        let scenario_hash = scenario
            .canonical_hash()
            .map_err(|error| error.to_string())?;
        let journal = if let Some(settings) = clock_settings {
            ActionJournal::new_with_clocks(env!("CARGO_PKG_VERSION"), &scenario, settings)
        } else {
            ActionJournal::new(env!("CARGO_PKG_VERSION"), &scenario)
        }
        .map_err(|error| error.to_string())?;
        let state = journal
            .replay(&scenario)
            .map_err(|error| error.to_string())?;
        let anchor = unix_millis(started_at)?;
        let clock = state.clocks.map(|clocks| PersistedClockAuthority {
            anchor_unix_millis: anchor,
            deadline_unix_millis: Some(
                anchor.saturating_add(remaining_for(clocks, state.active_player)),
            ),
        });
        Ok(Self {
            match_id,
            scenario,
            scenario_hash,
            state,
            journal,
            clock,
            room_state: ConnectionState::Connected,
            prepared: None,
        })
    }

    /// Restores authority from one previously atomic state/journal/clock transition.
    ///
    /// # Errors
    ///
    /// Returns a fatal description when persisted state diverges from its journal or scenario.
    pub fn restore(
        scenario: ScenarioDefinition,
        persisted: PreparedAuthorityTransition,
    ) -> Result<Self, String> {
        let replayed = persisted
            .journal
            .replay(&scenario)
            .map_err(|error| error.to_string())?;
        if replayed != persisted.state {
            return Err("persisted state does not match journal replay".to_owned());
        }
        let scenario_hash = scenario
            .canonical_hash()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            match_id: persisted.match_id,
            scenario,
            scenario_hash,
            state: persisted.state,
            journal: persisted.journal,
            clock: persisted.clock,
            room_state: ConnectionState::Connected,
            prepared: None,
        })
    }

    pub fn take_prepared_transition(&mut self) -> Option<PreparedAuthorityTransition> {
        self.prepared.take()
    }

    fn authoritative_snapshot(&self) -> Result<MatchSnapshot, String> {
        Ok(MatchSnapshot {
            match_id: self.match_id,
            revision: self.state.revision,
            scenario_id: self.state.scenario_id.clone(),
            scenario_hash: self.scenario_hash.clone(),
            state_hash: self
                .state
                .canonical_hash()
                .map_err(|error| error.to_string())?,
            state: self.state.clone(),
            room_state: self.room_state,
        })
    }

    fn apply_authoritative(
        &mut self,
        idempotency_key: Uuid,
        seat: Player,
        action: &Action,
        timing: CommandTiming,
    ) -> Result<MatchSnapshot, ExecutionError> {
        if action_player(action) != seat {
            return Err(ExecutionError::Rejected(CommandRejection::WrongSeat));
        }
        if self.room_state != ConnectionState::Connected || self.state.outcome.is_some() {
            return Err(ExecutionError::Rejected(CommandRejection::InactivePhase));
        }
        let received = unix_millis(timing.received_at).map_err(ExecutionError::Fatal)?;
        let decided = unix_millis(timing.decided_at).map_err(ExecutionError::Fatal)?;
        let elapsed = self
            .clock
            .as_ref()
            .map_or(0, |clock| received.saturating_sub(clock.anchor_unix_millis));
        let mut journal = self.journal.clone();
        let key = IdempotencyKey(*idempotency_key.as_bytes());
        let transition =
            match journal.append_timed(&self.scenario, &self.state, key, action, elapsed) {
                Ok(AppendOutcome::Accepted(transition)) => transition,
                Ok(AppendOutcome::Duplicate { .. }) => {
                    return self.authoritative_snapshot().map_err(ExecutionError::Fatal);
                }
                Err(JournalError::Transition(error)) => {
                    return Err(ExecutionError::Rejected(CommandRejection::IllegalAction(
                        error.to_string(),
                    )));
                }
                Err(error) => return Err(ExecutionError::Fatal(error.to_string())),
            };
        let timed_out = self.state.outcome.is_none()
            && transition
                .state
                .outcome
                .is_some_and(|outcome| outcome.reason == OutcomeReason::Timeout);
        self.state = transition.state;
        self.journal = journal;
        self.clock =
            self.state.clocks.map(|clocks| PersistedClockAuthority {
                anchor_unix_millis: received,
                deadline_unix_millis: self.state.outcome.is_none().then(|| {
                    received.saturating_add(remaining_for(clocks, self.state.active_player))
                }),
            });
        self.prepared = Some(PreparedAuthorityTransition {
            match_id: self.match_id,
            state: self.state.clone(),
            journal: self.journal.clone(),
            clock: self.clock.clone(),
            received_unix_millis: received,
            decided_unix_millis: decided,
        });
        let snapshot = self
            .authoritative_snapshot()
            .map_err(ExecutionError::Fatal)?;
        if timed_out {
            return Err(ExecutionError::Rejected(CommandRejection::ExpiredTime));
        }
        Ok(snapshot)
    }
}

impl MatchExecutor for AuthoritativeMatch {
    fn snapshot(&self) -> MatchSnapshot {
        self.authoritative_snapshot()
            .expect("validated authority state must hash")
    }

    fn execute(
        &mut self,
        idempotency_key: Uuid,
        seat: Player,
        action: &Action,
        timing: CommandTiming,
    ) -> Result<MatchSnapshot, ExecutionError> {
        self.apply_authoritative(idempotency_key, seat, action, timing)
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

const fn remaining_for(clocks: crownline_core::state::ClockState, player: Player) -> u64 {
    match player {
        Player::North => clocks.north_millis,
        Player::South => clocks.south_millis,
    }
}

fn unix_millis(time: SystemTime) -> Result<u64, String> {
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "server wall clock precedes Unix epoch".to_owned())?
        .as_millis();
    u64::try_from(millis).map_err(|_| "server wall clock exceeds u64 milliseconds".to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crownline_core::state::OutcomeReason;

    use super::*;

    fn scenario() -> ScenarioDefinition {
        ron::from_str(include_str!("../../../assets/scenarios/standard.ron")).unwrap()
    }

    fn timing(received_at: SystemTime) -> CommandTiming {
        CommandTiming {
            received_at,
            decided_at: received_at.checked_add(Duration::from_millis(2)).unwrap(),
        }
    }

    #[test]
    fn seat_phase_and_illegal_action_errors_are_distinct_and_transactional() {
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(1_000)).unwrap();
        let mut authority =
            AuthoritativeMatch::new(Uuid::new_v4(), scenario(), None, started).unwrap();
        let before = authority.snapshot();
        let hold = Action::Hold {
            player: Player::South,
        };
        assert_eq!(
            authority.execute(Uuid::new_v4(), Player::North, &hold, timing(started)),
            Err(ExecutionError::Rejected(CommandRejection::WrongSeat))
        );
        let illegal = Action::Hold {
            player: Player::North,
        };
        assert!(matches!(
            authority.execute(Uuid::new_v4(), Player::North, &illegal, timing(started)),
            Err(ExecutionError::Rejected(CommandRejection::IllegalAction(_)))
        ));
        assert_eq!(authority.snapshot(), before);
        authority.room_state = ConnectionState::WaitingForReady;
        assert_eq!(
            authority.execute(Uuid::new_v4(), Player::South, &hold, timing(started)),
            Err(ExecutionError::Rejected(CommandRejection::InactivePhase))
        );
    }

    #[test]
    fn exact_persisted_deadline_times_out_once_and_downtime_counts() {
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(10_000)).unwrap();
        let settings = ClockSettings {
            base_minutes: 1,
            increment_seconds: 0,
        };
        let mut authority =
            AuthoritativeMatch::new(Uuid::new_v4(), scenario(), Some(settings), started).unwrap();
        let deadline = started.checked_add(Duration::from_mins(1)).unwrap();
        let action = Action::Hold {
            player: Player::South,
        };
        assert_eq!(
            authority.execute(Uuid::new_v4(), Player::South, &action, timing(deadline)),
            Err(ExecutionError::Rejected(CommandRejection::ExpiredTime))
        );
        let expired = authority.snapshot();
        assert_eq!(expired.state.revision, 1);
        assert_eq!(
            expired.state.outcome.unwrap().reason,
            OutcomeReason::Timeout
        );
        assert_eq!(
            authority.execute(Uuid::new_v4(), Player::South, &action, timing(deadline)),
            Err(ExecutionError::Rejected(CommandRejection::InactivePhase))
        );
        assert_eq!(authority.snapshot().state.revision, 1);

        let persisted = authority.take_prepared_transition().unwrap();
        let restored = AuthoritativeMatch::restore(scenario(), persisted).unwrap();
        assert_eq!(restored.snapshot(), expired);
    }

    #[test]
    fn accepted_hold_charges_elapsed_adds_increment_and_prepares_atomic_unit() {
        let started = UNIX_EPOCH.checked_add(Duration::from_secs(20_000)).unwrap();
        let mut authority = AuthoritativeMatch::new(
            Uuid::new_v4(),
            scenario(),
            Some(ClockSettings {
                base_minutes: 1,
                increment_seconds: 5,
            }),
            started,
        )
        .unwrap();
        let received = started.checked_add(Duration::from_secs(10)).unwrap();
        let snapshot = authority
            .execute(
                Uuid::new_v4(),
                Player::South,
                &Action::Hold {
                    player: Player::South,
                },
                timing(received),
            )
            .unwrap();
        let clocks = snapshot.state.clocks.unwrap();
        assert_eq!(clocks.south_millis, 55_000);
        assert_eq!(clocks.north_millis, 60_000);
        let prepared = authority.take_prepared_transition().unwrap();
        assert_eq!(prepared.state, snapshot.state);
        assert_eq!(prepared.journal.records.len(), 1);
        assert_eq!(
            prepared.clock.unwrap().deadline_unix_millis,
            Some(unix_millis(received).unwrap() + 60_000)
        );
        assert!(prepared.decided_unix_millis >= prepared.received_unix_millis);
    }
}
