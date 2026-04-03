use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::foundation::seaorm::SeaOrmConnectionFactory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CircuitBreakerState {
    Closed,
    HalfOpen,
    Open,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self::Closed
    }
}

impl CircuitBreakerState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::HalfOpen => "half_open",
            Self::Open => "open",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CircuitBreakerPolicy {
    pub(crate) half_open_threshold: usize,
    pub(crate) open_threshold: usize,
    pub(crate) reset_window: Duration,
}

impl Default for CircuitBreakerPolicy {
    fn default() -> Self {
        Self {
            half_open_threshold: 3,
            open_threshold: 5,
            reset_window: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CircuitBreakerSnapshot {
    pub(crate) channel_id: i64,
    pub(crate) model_id: String,
    pub(crate) state: CircuitBreakerState,
    pub(crate) consecutive_failures: usize,
    pub(crate) next_probe_in_seconds: Option<i64>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChannelBreakerStatus {
    pub(crate) active: Option<CircuitBreakerSnapshot>,
}

#[derive(Debug, Clone, Default)]
struct ChannelModelBreaker {
    consecutive_failures: usize,
    state: CircuitBreakerState,
    next_probe_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub(crate) struct SharedCircuitBreaker {
    inner: Arc<Mutex<HashMap<(i64, String), ChannelModelBreaker>>>,
    policy: CircuitBreakerPolicy,
}

impl SharedCircuitBreaker {
    pub(crate) fn new(policy: CircuitBreakerPolicy) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            policy,
        }
    }

    pub(crate) fn with_factory(factory: &SeaOrmConnectionFactory) -> Self {
        registry_for_factory(factory.instance_id(), CircuitBreakerPolicy::default())
    }

    pub(crate) fn with_factory_and_policy(
        factory: &SeaOrmConnectionFactory,
        policy: CircuitBreakerPolicy,
    ) -> Self {
        registry_for_factory(factory.instance_id(), policy)
    }

    pub(crate) fn current_snapshot(
        &self,
        channel_id: i64,
        model_id: &str,
    ) -> Option<CircuitBreakerSnapshot> {
        let inner = self.inner.lock().expect("circuit breaker lock poisoned");
        let entry = inner.get(&(channel_id, model_id.to_owned()))?;
        let state = effective_state(entry);
        if matches!(state, CircuitBreakerState::Closed) && entry.consecutive_failures == 0 {
            return None;
        }
        let next_probe_in_seconds = entry.next_probe_at.map(|deadline| {
            let now = Instant::now();
            if deadline <= now {
                0
            } else {
                deadline.duration_since(now).as_secs() as i64
            }
        });
        Some(CircuitBreakerSnapshot {
            channel_id,
            model_id: model_id.to_owned(),
            state,
            consecutive_failures: entry.consecutive_failures,
            next_probe_in_seconds,
        })
    }

    pub(crate) fn channel_status(&self, channel_id: i64) -> ChannelBreakerStatus {
        let inner = self.inner.lock().expect("circuit breaker lock poisoned");
        let active = inner
            .iter()
            .filter(|((current_channel_id, _), _)| *current_channel_id == channel_id)
            .filter_map(|((current_channel_id, model_id), _)| {
                let entry = inner.get(&(*current_channel_id, model_id.clone()))?;
                let state = effective_state(entry);
                if matches!(state, CircuitBreakerState::Closed) && entry.consecutive_failures == 0 {
                    return None;
                }
                let next_probe_in_seconds = entry.next_probe_at.map(|deadline| {
                    let now = Instant::now();
                    if deadline <= now {
                        0
                    } else {
                        deadline.duration_since(now).as_secs() as i64
                    }
                });
                Some(CircuitBreakerSnapshot {
                    channel_id: *current_channel_id,
                    model_id: model_id.clone(),
                    state,
                    consecutive_failures: entry.consecutive_failures,
                    next_probe_in_seconds,
                })
            })
            .max_by_key(|snapshot| {
                (
                    circuit_breaker_rank(snapshot.state),
                    snapshot.consecutive_failures,
                    snapshot.next_probe_in_seconds.unwrap_or_default(),
                )
            });
        ChannelBreakerStatus { active }
    }

    pub(crate) fn is_blocked(&self, channel_id: i64, model_id: &str) -> bool {
        self.current_snapshot(channel_id, model_id)
            .is_some_and(|snapshot| matches!(snapshot.state, CircuitBreakerState::Open))
    }

    pub(crate) fn record_failure(&self, channel_id: i64, model_id: &str) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        let entry = inner
            .entry((channel_id, model_id.to_owned()))
            .or_insert_with(ChannelModelBreaker::default);
        entry.consecutive_failures += 1;
        if entry.consecutive_failures >= self.policy.open_threshold {
            entry.state = CircuitBreakerState::Open;
            entry.next_probe_at = Some(Instant::now() + self.policy.reset_window);
        } else if entry.consecutive_failures >= self.policy.half_open_threshold {
            entry.state = CircuitBreakerState::HalfOpen;
            entry.next_probe_at = None;
        } else {
            entry.state = CircuitBreakerState::Closed;
            entry.next_probe_at = None;
        }
    }

    pub(crate) fn record_success(&self, channel_id: i64, model_id: &str) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        let entry = inner
            .entry((channel_id, model_id.to_owned()))
            .or_insert_with(ChannelModelBreaker::default);
        entry.consecutive_failures = 0;
        entry.state = CircuitBreakerState::Closed;
        entry.next_probe_at = None;
    }

    pub(crate) fn reset(&self, channel_id: i64, model_id: &str) {
        self.record_success(channel_id, model_id);
    }
}

fn effective_state(entry: &ChannelModelBreaker) -> CircuitBreakerState {
    match entry.state {
        CircuitBreakerState::Open => {
            if entry
                .next_probe_at
                .is_some_and(|deadline| deadline <= Instant::now())
            {
                CircuitBreakerState::HalfOpen
            } else {
                CircuitBreakerState::Open
            }
        }
        other => other,
    }
}

fn circuit_breaker_rank(state: CircuitBreakerState) -> i32 {
    match state {
        CircuitBreakerState::Open => 3,
        CircuitBreakerState::HalfOpen => 2,
        CircuitBreakerState::Closed => 1,
    }
}

fn registry_for_factory(factory_id: u64, policy: CircuitBreakerPolicy) -> SharedCircuitBreaker {
    type SharedBreakerMap = Arc<Mutex<HashMap<(i64, String), ChannelModelBreaker>>>;
    static REGISTRY: OnceLock<Mutex<HashMap<u64, SharedBreakerMap>>> = OnceLock::new();

    let registry = REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry
        .lock()
        .expect("circuit breaker registry lock poisoned");
    let inner = registry
        .entry(factory_id)
        .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
        .clone();

    SharedCircuitBreaker { inner, policy }
}
