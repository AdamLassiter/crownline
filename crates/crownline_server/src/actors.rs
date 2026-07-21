use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use crownline_core::{Action, scenario::Player};
use crownline_protocol::{
    MAX_CACHED_MUTATIONS_PER_MATCH, MatchSnapshot, MutationContext, MutationResult,
};
use thiserror::Error;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{error, info};
use uuid::Uuid;

pub const DEFAULT_ACTOR_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub struct MatchCommand {
    pub context: MutationContext,
    pub seat: Player,
    pub action: Action,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandTiming {
    pub received_at: SystemTime,
    pub decided_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRejection {
    WrongSeat,
    InactivePhase,
    ExpiredTime,
    IllegalAction(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    Rejected(CommandRejection),
    Fatal(String),
}

pub trait MatchExecutor: Send {
    fn snapshot(&self) -> MatchSnapshot;

    /// Applies one already revision-checked command.
    ///
    /// # Errors
    ///
    /// Returns a recoverable rejection or a fatal error requiring durable reload.
    fn execute(
        &mut self,
        idempotency_key: Uuid,
        seat: Player,
        action: &Action,
        timing: CommandTiming,
    ) -> Result<MatchSnapshot, ExecutionError>;
}

pub trait MatchLoader: Send + Sync {
    /// Reconstructs an executor from durable match state.
    ///
    /// # Errors
    ///
    /// Returns a non-sensitive load failure message when recovery is unavailable.
    fn load(&self, match_id: Uuid) -> Result<Box<dyn MatchExecutor>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchCommandResult {
    Accepted(MutationResult),
    Stale(MatchSnapshot),
    Rejected {
        reason: CommandRejection,
        snapshot: MatchSnapshot,
    },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ActorSubmitError {
    #[error("match command queue is full")]
    QueueFull,
    #[error("match actor is unavailable")]
    Unavailable,
    #[error("match actor failed and will be reloaded")]
    Failed,
    #[error("durable match could not be loaded: {0}")]
    Load(String),
}

impl ActorSubmitError {
    pub const fn retryable(&self) -> bool {
        matches!(self, Self::QueueFull | Self::Unavailable | Self::Failed)
    }
}

struct QueuedCommand {
    command: MatchCommand,
    received_at: SystemTime,
    response: oneshot::Sender<Result<MatchCommandResult, ActorSubmitError>>,
}

#[derive(Clone)]
struct ActorHandle {
    sender: mpsc::Sender<QueuedCommand>,
    last_used: Instant,
}

pub struct MatchActorRegistry {
    actors: Mutex<BTreeMap<Uuid, ActorHandle>>,
    loader: Arc<dyn MatchLoader>,
    queue_capacity: usize,
}

impl MatchActorRegistry {
    pub fn new(loader: Arc<dyn MatchLoader>, queue_capacity: usize) -> Self {
        Self {
            actors: Mutex::new(BTreeMap::new()),
            loader,
            queue_capacity: queue_capacity.max(1),
        }
    }

    /// Enqueues one command without holding the global registry during execution.
    ///
    /// # Errors
    ///
    /// Returns controlled load, saturation, actor-failure, and availability errors.
    pub async fn submit(
        &self,
        command: MatchCommand,
    ) -> Result<MatchCommandResult, ActorSubmitError> {
        let match_id = command.context.match_id;
        let sender = self.sender_for(match_id).await?;
        let (response, receiver) = oneshot::channel();
        match sender.try_send(QueuedCommand {
            command,
            received_at: SystemTime::now(),
            response,
        }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(ActorSubmitError::QueueFull),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.remove_if_closed(match_id).await;
                return Err(ActorSubmitError::Unavailable);
            }
        }
        match receiver.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => {
                if error == ActorSubmitError::Failed {
                    self.remove_if_closed(match_id).await;
                }
                Err(error)
            }
            Err(_) => {
                self.remove_if_closed(match_id).await;
                Err(ActorSubmitError::Unavailable)
            }
        }
    }

    pub async fn unload_idle(&self, now: Instant, maximum_idle: Duration) -> usize {
        let mut actors = self.actors.lock().await;
        let before = actors.len();
        actors.retain(|match_id, handle| {
            let keep = now.duration_since(handle.last_used) < maximum_idle;
            if !keep {
                info!(%match_id, "unloaded idle match actor; durable match retained");
            }
            keep
        });
        before - actors.len()
    }

    async fn sender_for(
        &self,
        match_id: Uuid,
    ) -> Result<mpsc::Sender<QueuedCommand>, ActorSubmitError> {
        let mut actors = self.actors.lock().await;
        let now = Instant::now();
        if let Some(handle) = actors.get_mut(&match_id) {
            handle.last_used = now;
            return Ok(handle.sender.clone());
        }
        let executor = self.loader.load(match_id).map_err(ActorSubmitError::Load)?;
        let (sender, receiver) = mpsc::channel(self.queue_capacity);
        tokio::spawn(run_actor(match_id, executor, receiver));
        actors.insert(
            match_id,
            ActorHandle {
                sender: sender.clone(),
                last_used: now,
            },
        );
        Ok(sender)
    }

    async fn remove_if_closed(&self, match_id: Uuid) {
        let mut actors = self.actors.lock().await;
        if actors
            .get(&match_id)
            .is_some_and(|handle| handle.sender.is_closed())
        {
            actors.remove(&match_id);
        }
    }
}

async fn run_actor(
    match_id: Uuid,
    mut executor: Box<dyn MatchExecutor>,
    mut receiver: mpsc::Receiver<QueuedCommand>,
) {
    let mut decisions = BTreeMap::<Uuid, MatchCommandResult>::new();
    let mut decision_order = std::collections::VecDeque::<Uuid>::new();
    while let Some(queued) = receiver.recv().await {
        let context = queued.command.context;
        if let Some(original) = decisions.get(&context.idempotency_key) {
            let _ = queued.response.send(Ok(original.clone()));
            continue;
        }
        let current = executor.snapshot();
        if context.match_id != match_id || context.expected_revision != current.revision {
            let _ = queued.response.send(Ok(MatchCommandResult::Stale(current)));
            continue;
        }
        let timing = CommandTiming {
            received_at: queued.received_at,
            decided_at: SystemTime::now(),
        };
        match executor.execute(
            context.idempotency_key,
            queued.command.seat,
            &queued.command.action,
            timing,
        ) {
            Ok(snapshot) => {
                let result = MutationResult {
                    match_id,
                    idempotency_key: context.idempotency_key,
                    snapshot,
                };
                let decision = MatchCommandResult::Accepted(result);
                decisions.insert(context.idempotency_key, decision.clone());
                decision_order.push_back(context.idempotency_key);
                if decisions.len() > MAX_CACHED_MUTATIONS_PER_MATCH
                    && let Some(oldest) = decision_order.pop_front()
                {
                    decisions.remove(&oldest);
                }
                let _ = queued.response.send(Ok(decision));
            }
            Err(ExecutionError::Rejected(reason)) => {
                let decision = MatchCommandResult::Rejected {
                    reason,
                    snapshot: executor.snapshot(),
                };
                decisions.insert(context.idempotency_key, decision.clone());
                decision_order.push_back(context.idempotency_key);
                if decisions.len() > MAX_CACHED_MUTATIONS_PER_MATCH
                    && let Some(oldest) = decision_order.pop_front()
                {
                    decisions.remove(&oldest);
                }
                let _ = queued.response.send(Ok(decision));
            }
            Err(ExecutionError::Fatal(message)) => {
                error!(%match_id, %message, "match actor failed; durable reload required");
                let _ = queued.response.send(Err(ActorSubmitError::Failed));
                receiver.close();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
        mpsc as std_mpsc,
    };

    use crownline_core::{MatchState, ScenarioDefinition, scenario::Player};

    use super::*;

    #[derive(Clone)]
    struct DurableLoader {
        snapshot: Arc<StdMutex<MatchSnapshot>>,
        loads: Arc<AtomicUsize>,
        fail_next: Arc<StdMutex<bool>>,
    }

    impl MatchLoader for DurableLoader {
        fn load(&self, _match_id: Uuid) -> Result<Box<dyn MatchExecutor>, String> {
            self.loads.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeExecutor {
                snapshot: Arc::clone(&self.snapshot),
                fail_next: Arc::clone(&self.fail_next),
            }))
        }
    }

    struct FakeExecutor {
        snapshot: Arc<StdMutex<MatchSnapshot>>,
        fail_next: Arc<StdMutex<bool>>,
    }

    impl MatchExecutor for FakeExecutor {
        fn snapshot(&self) -> MatchSnapshot {
            self.snapshot.lock().unwrap().clone()
        }

        fn execute(
            &mut self,
            _idempotency_key: Uuid,
            _seat: Player,
            _action: &Action,
            _timing: CommandTiming,
        ) -> Result<MatchSnapshot, ExecutionError> {
            let mut fail_next = self.fail_next.lock().unwrap();
            if *fail_next {
                *fail_next = false;
                return Err(ExecutionError::Fatal(
                    "simulated durable failure".to_owned(),
                ));
            }
            drop(fail_next);
            let mut snapshot = self.snapshot.lock().unwrap();
            snapshot.revision += 1;
            snapshot.state.revision += 1;
            snapshot.state_hash = snapshot.state.canonical_hash().unwrap();
            Ok(snapshot.clone())
        }
    }

    fn fixture() -> (Uuid, DurableLoader) {
        let scenario: ScenarioDefinition =
            ron::from_str(include_str!("../../../assets/scenarios/standard.ron")).unwrap();
        let state = MatchState::from_scenario(&scenario).unwrap();
        let match_id = Uuid::new_v4();
        let snapshot = MatchSnapshot {
            match_id,
            revision: state.revision,
            scenario_id: state.scenario_id.clone(),
            scenario_hash: scenario.canonical_hash().unwrap(),
            state_hash: state.canonical_hash().unwrap(),
            state,
            room_state: crownline_protocol::ConnectionState::Connected,
            rematch_state: None,
        };
        (
            match_id,
            DurableLoader {
                snapshot: Arc::new(StdMutex::new(snapshot)),
                loads: Arc::new(AtomicUsize::new(0)),
                fail_next: Arc::new(StdMutex::new(false)),
            },
        )
    }

    fn command(match_id: Uuid, revision: u64) -> MatchCommand {
        MatchCommand {
            context: MutationContext {
                match_id,
                expected_revision: revision,
                idempotency_key: Uuid::new_v4(),
            },
            seat: Player::South,
            action: Action::Hold {
                player: Player::South,
            },
        }
    }

    #[tokio::test]
    async fn simultaneous_same_revision_commands_accept_at_most_one() {
        let (match_id, loader) = fixture();
        let registry = Arc::new(MatchActorRegistry::new(Arc::new(loader), 4));
        let first = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.submit(command(match_id, 0)).await })
        };
        let second = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.submit(command(match_id, 0)).await })
        };
        let results = [
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap(),
        ];
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, MatchCommandResult::Accepted(_)))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, MatchCommandResult::Stale(_)))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn fatal_actor_failure_is_contained_and_next_command_reloads_durable_state() {
        let (match_id, loader) = fixture();
        *loader.fail_next.lock().unwrap() = true;
        let loads = Arc::clone(&loader.loads);
        let registry = MatchActorRegistry::new(Arc::new(loader), 4);
        assert_eq!(
            registry.submit(command(match_id, 0)).await,
            Err(ActorSubmitError::Failed)
        );
        assert!(matches!(
            registry.submit(command(match_id, 0)).await,
            Ok(MatchCommandResult::Accepted(_))
        ));
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn idle_actor_unloads_without_deleting_durable_match() {
        let (match_id, loader) = fixture();
        let loads = Arc::clone(&loader.loads);
        let registry = MatchActorRegistry::new(Arc::new(loader), 4);
        registry.submit(command(match_id, 0)).await.unwrap();
        assert_eq!(
            registry
                .unload_idle(
                    Instant::now().checked_add(Duration::from_mins(2)).unwrap(),
                    Duration::from_mins(1)
                )
                .await,
            1
        );
        registry.submit(command(match_id, 1)).await.unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 2);
    }

    struct BlockingLoader {
        executor: StdMutex<Option<BlockingExecutor>>,
    }

    impl MatchLoader for BlockingLoader {
        fn load(&self, _match_id: Uuid) -> Result<Box<dyn MatchExecutor>, String> {
            self.executor
                .lock()
                .unwrap()
                .take()
                .map(|executor| Box::new(executor) as Box<dyn MatchExecutor>)
                .ok_or_else(|| "blocking executor already loaded".to_owned())
        }
    }

    struct BlockingExecutor {
        snapshot: MatchSnapshot,
        started: Option<std_mpsc::Sender<()>>,
        release: std_mpsc::Receiver<()>,
    }

    impl MatchExecutor for BlockingExecutor {
        fn snapshot(&self) -> MatchSnapshot {
            self.snapshot.clone()
        }

        fn execute(
            &mut self,
            _idempotency_key: Uuid,
            _seat: Player,
            _action: &Action,
            _timing: CommandTiming,
        ) -> Result<MatchSnapshot, ExecutionError> {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
            }
            self.release
                .recv()
                .map_err(|error| ExecutionError::Fatal(error.to_string()))?;
            self.snapshot.revision += 1;
            self.snapshot.state.revision += 1;
            self.snapshot.state_hash = self.snapshot.state.canonical_hash().unwrap();
            Ok(self.snapshot.clone())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saturated_queue_returns_controlled_retryable_error() {
        let (match_id, durable) = fixture();
        let snapshot = durable.snapshot.lock().unwrap().clone();
        let (started_tx, started_rx) = std_mpsc::channel();
        let (release_tx, release_rx) = std_mpsc::channel();
        let loader = BlockingLoader {
            executor: StdMutex::new(Some(BlockingExecutor {
                snapshot,
                started: Some(started_tx),
                release: release_rx,
            })),
        };
        let registry = Arc::new(MatchActorRegistry::new(Arc::new(loader), 1));
        let first = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.submit(command(match_id, 0)).await })
        };
        tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
            .await
            .unwrap();
        let second = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move { registry.submit(command(match_id, 0)).await })
        };
        loop {
            let is_full = registry
                .actors
                .lock()
                .await
                .get(&match_id)
                .is_some_and(|handle| handle.sender.capacity() == 0);
            if is_full {
                break;
            }
            tokio::task::yield_now().await;
        }
        let error = registry.submit(command(match_id, 0)).await.unwrap_err();
        assert_eq!(error, ActorSubmitError::QueueFull);
        assert!(error.retryable());
        release_tx.send(()).unwrap();
        first.await.unwrap().unwrap();
        assert!(matches!(
            second.await.unwrap().unwrap(),
            MatchCommandResult::Stale(_)
        ));
    }
}
