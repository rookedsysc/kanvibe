//! 호스트별 daemon 연결 풀(TS `configureRemoteHostTransport`, `getRemoteHostConnection`, `stopRemoteHostConnections`).
//!
//! 같은 원격 계정(실효 ssh 설정 hash)과 namespace에는 연결 하나만 둔다. 시작 중인 연결은 뒤에 온 요청이 함께
//! 기다리고, daemon을 쓸 수 없는 호스트라는 결과는 60초 동안 재사용한다. 멈추면 세대를 올려 시작 중이던 연결도 버린다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_process::hooks::socket_path::is_valid_hook_client_id;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_remote_agent::protocol::is_valid_host_namespace;

use crate::agent_binaries::AgentBinaryCatalog;
use crate::connection::{
    HostConnection, HostTransportSpawner, RemoteHostConnectionError, connect_through_loader,
};
use crate::hook_tunnel::SshHostLookup;
use crate::ssh_transport::{
    EffectiveSshConfig, build_endpoint_probe_args, endpoint_key_from_effective_config,
};

const UNSUPPORTED_CACHE: Duration = Duration::from_secs(60);

pub struct RemoteHostConnectionPoolDependencies {
    pub ssh_hosts: Arc<dyn SshHostLookup>,
    pub binaries: Arc<AgentBinaryCatalog>,
    pub transport_spawner: Arc<dyn HostTransportSpawner>,
    pub effective_ssh_config: Arc<dyn EffectiveSshConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransportIdentity {
    namespace: String,
    client_id: String,
}

enum SlotOutcome {
    Starting,
    Ready(Arc<HostConnection>),
    Unsupported { expires_at: Instant },
    Failed(RemoteHostConnectionError),
}

struct ConnectionSlot {
    outcome: Mutex<SlotOutcome>,
    settled: Condvar,
}

impl ConnectionSlot {
    fn is_reusable(&self) -> bool {
        match &*self.outcome.lock().unwrap() {
            SlotOutcome::Starting => true,
            SlotOutcome::Ready(connection) => !connection.is_closed(),
            SlotOutcome::Unsupported { expires_at } => Instant::now() < *expires_at,
            SlotOutcome::Failed(_) => false,
        }
    }

    fn wait(&self) -> Result<Option<Arc<HostConnection>>, RemoteHostConnectionError> {
        let outcome = self.outcome.lock().unwrap();
        let outcome = self
            .settled
            .wait_while(outcome, |outcome| matches!(outcome, SlotOutcome::Starting))
            .unwrap();
        match &*outcome {
            SlotOutcome::Ready(connection) => Ok(Some(Arc::clone(connection))),
            SlotOutcome::Unsupported { .. } => Ok(None),
            SlotOutcome::Failed(error) => Err(error.clone()),
            SlotOutcome::Starting => unreachable!("시작이 끝날 때까지 기다렸다"),
        }
    }

    fn settle(&self, outcome: SlotOutcome) {
        *self.outcome.lock().unwrap() = outcome;
        self.settled.notify_all();
    }
}

/// `ssh -G` 조회 하나. 같은 설정을 동시에 물으면 한 번만 실행하고 결과(실패 포함)를 함께 쓴다(TS `endpointKeys`의 Promise).
/// 성공한 결과는 남아 캐시가 되고, 실패한 조회는 지워 다음 호출이 다시 실행한다
#[derive(Default)]
struct EndpointKeyLookup {
    outcome: Mutex<Option<Result<String, String>>>,
    settled: Condvar,
}

impl EndpointKeyLookup {
    fn wait(&self) -> Result<String, String> {
        let outcome = self.outcome.lock().unwrap();
        let outcome = self
            .settled
            .wait_while(outcome, |outcome| outcome.is_none())
            .unwrap();
        outcome.clone().expect("결과가 정해질 때까지 기다렸다")
    }

    fn settle(&self, outcome: Result<String, String>) {
        *self.outcome.lock().unwrap() = Some(outcome);
        self.settled.notify_all();
    }
}

#[derive(Default)]
struct PoolState {
    identity: Option<TransportIdentity>,
    generation: u64,
    slots: HashMap<String, Arc<ConnectionSlot>>,
    live: Vec<Arc<HostConnection>>,
    endpoint_keys: HashMap<String, Arc<EndpointKeyLookup>>,
}

pub struct RemoteHostConnectionPool {
    dependencies: RemoteHostConnectionPoolDependencies,
    state: Mutex<PoolState>,
}

impl RemoteHostConnectionPool {
    pub fn new(dependencies: RemoteHostConnectionPoolDependencies) -> Arc<Self> {
        Arc::new(Self {
            dependencies,
            state: Mutex::new(PoolState::default()),
        })
    }

    /// 이 데스크탑의 namespace와 hook client ID로 daemon을 쓰게 한다. 신원이 바뀌면 기존 연결을 모두 닫는다
    pub fn configure_transport(&self, namespace: &str, client_id: &str) -> Result<(), String> {
        if !is_valid_host_namespace(namespace) || !is_valid_hook_client_id(client_id) {
            return Err("Invalid host transport identity".into());
        }
        let identity = TransportIdentity {
            namespace: namespace.into(),
            client_id: client_id.into(),
        };
        if self.state.lock().unwrap().identity.as_ref() == Some(&identity) {
            return Ok(());
        }
        self.stop_connections();
        self.state.lock().unwrap().identity = Some(identity);
        Ok(())
    }

    pub fn namespace(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap()
            .identity
            .as_ref()
            .map(|identity| identity.namespace.clone())
    }

    /// 모든 연결을 닫고 신원을 지운다. 시작 중인 연결은 끝나는 대로 닫힌다
    pub fn stop_connections(&self) {
        let live = {
            let mut state = self.state.lock().unwrap();
            state.generation += 1;
            state.identity = None;
            state.slots.clear();
            state.endpoint_keys.clear();
            std::mem::take(&mut state.live)
        };
        for connection in live {
            connection.close();
        }
    }

    /// `ssh -G` 실효 설정 hash. 같은 설정은 진행 중인 조회와 끝난 결과를 함께 쓴다
    pub fn endpoint_key(&self, config: &SshHostConfig) -> Result<String, String> {
        let cache_key = config.identity_json();
        let (lookup, is_owner) = {
            let mut state = self.state.lock().unwrap();
            match state.endpoint_keys.get(&cache_key) {
                Some(lookup) => (Arc::clone(lookup), false),
                None => {
                    let lookup = Arc::new(EndpointKeyLookup::default());
                    state
                        .endpoint_keys
                        .insert(cache_key.clone(), Arc::clone(&lookup));
                    (lookup, true)
                }
            }
        };
        if !is_owner {
            return lookup.wait();
        }
        let outcome = self
            .dependencies
            .effective_ssh_config
            .resolve(&build_endpoint_probe_args(config))
            .map(|effective| endpoint_key_from_effective_config(config, &effective));
        if outcome.is_err() {
            let mut state = self.state.lock().unwrap();
            if state
                .endpoint_keys
                .get(&cache_key)
                .is_some_and(|current| Arc::ptr_eq(current, &lookup))
            {
                state.endpoint_keys.remove(&cache_key);
            }
        }
        lookup.settle(outcome.clone());
        outcome
    }

    /// 호스트의 daemon 연결. 신원을 설정하지 않았거나 원격이 지원하지 않으면 `None`이다.
    /// 연결 시작 전 준비(호스트 찾기, 실효 ssh 설정)의 실패는 준비 단계 실패다
    pub fn connection(
        self: &Arc<Self>,
        ssh_host: &str,
    ) -> Result<Option<Arc<HostConnection>>, RemoteHostConnectionError> {
        let Some((identity, epoch)) = self.current_identity() else {
            return Ok(None);
        };
        let config = self
            .dependencies
            .ssh_hosts
            .find_host(ssh_host)
            .ok_or_else(|| {
                RemoteHostConnectionError::setup(format!(
                    "SSH 호스트를 찾을 수 없습니다: {ssh_host}"
                ))
            })?;
        let endpoint_key = self
            .endpoint_key(&config)
            .map_err(RemoteHostConnectionError::setup)?;
        let key = serde_json::to_string(&[endpoint_key, identity.namespace.clone()])
            .expect("문자열 배열");
        let (slot, is_owner) = {
            let mut state = self.state.lock().unwrap();
            if state.generation != epoch {
                return Err(RemoteHostConnectionError::setup(
                    "Host preparation cancelled",
                ));
            }
            match state.slots.get(&key).filter(|slot| slot.is_reusable()) {
                Some(slot) => (Arc::clone(slot), false),
                None => {
                    let slot = Arc::new(ConnectionSlot {
                        outcome: Mutex::new(SlotOutcome::Starting),
                        settled: Condvar::new(),
                    });
                    state.slots.insert(key.clone(), Arc::clone(&slot));
                    (slot, true)
                }
            }
        };
        if is_owner {
            self.start_connection(&slot, &key, &config, &identity, epoch);
        }
        slot.wait()
    }

    fn current_identity(&self) -> Option<(TransportIdentity, u64)> {
        let state = self.state.lock().unwrap();
        state
            .identity
            .clone()
            .map(|identity| (identity, state.generation))
    }

    fn start_connection(
        self: &Arc<Self>,
        slot: &Arc<ConnectionSlot>,
        key: &str,
        config: &SshHostConfig,
        identity: &TransportIdentity,
        epoch: u64,
    ) {
        let started = connect_through_loader(
            self.dependencies.transport_spawner.as_ref(),
            config,
            &self.dependencies.binaries,
            &identity.namespace,
            &identity.client_id,
        );
        let outcome = match started {
            Ok(Some(connection)) => self.register_live(slot, key, connection, epoch),
            Ok(None) => SlotOutcome::Unsupported {
                expires_at: Instant::now() + UNSUPPORTED_CACHE,
            },
            Err(error) => SlotOutcome::Failed(error),
        };
        if matches!(outcome, SlotOutcome::Failed(_)) {
            let mut state = self.state.lock().unwrap();
            if state
                .slots
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, slot))
            {
                state.slots.remove(key);
            }
        }
        slot.settle(outcome);
    }

    fn register_live(
        self: &Arc<Self>,
        slot: &Arc<ConnectionSlot>,
        key: &str,
        connection: Arc<HostConnection>,
        epoch: u64,
    ) -> SlotOutcome {
        {
            let mut state = self.state.lock().unwrap();
            if state.generation != epoch {
                drop(state);
                connection.close();
                return SlotOutcome::Failed(RemoteHostConnectionError::setup(
                    "Host preparation cancelled",
                ));
            }
            state.live.push(Arc::clone(&connection));
        }
        let (pool, slot, key, closed) = (
            Arc::downgrade(self),
            Arc::downgrade(slot),
            key.to_string(),
            Arc::downgrade(&connection),
        );
        connection.on_close(Box::new(move || {
            let Some(pool) = pool.upgrade() else {
                return;
            };
            let mut state = pool.state.lock().unwrap();
            state.live.retain(|live| {
                closed
                    .upgrade()
                    .is_none_or(|closed| !Arc::ptr_eq(live, &closed))
            });
            state.endpoint_keys.clear();
            if state.slots.get(&key).is_some_and(|current| {
                slot.upgrade()
                    .is_some_and(|slot| Arc::ptr_eq(current, &slot))
            }) {
                state.slots.remove(&key);
            }
        }));
        SlotOutcome::Ready(connection)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{Receiver, channel};
    use std::time::Duration;

    use super::*;
    use crate::connection::HostTransport;

    struct OneHost;

    impl SshHostLookup for OneHost {
        fn find_host(&self, ssh_host: &str) -> Option<SshHostConfig> {
            Some(SshHostConfig {
                host: ssh_host.into(),
                hostname: "10.0.0.7".into(),
                port: 22,
                username: "fixture".into(),
                private_key_path: "/tmp/unused-key".into(),
            })
        }
    }

    struct NoTransport;

    impl HostTransportSpawner for NoTransport {
        fn spawn(&self, _: &SshHostConfig, _: &str) -> Result<HostTransport, String> {
            Err("이 테스트는 연결을 열지 않는다".into())
        }
    }

    /// 풀려날 때까지 끝나지 않는 `ssh -G`. 몇 번 불렸는지 센다
    struct HeldEffectiveConfig {
        calls: AtomicUsize,
        release: Mutex<Receiver<Result<String, String>>>,
    }

    impl EffectiveSshConfig for HeldEffectiveConfig {
        fn resolve(&self, _: &[String]) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.release.lock().unwrap().recv().unwrap()
        }
    }

    fn pool_with(effective: Arc<HeldEffectiveConfig>) -> Arc<RemoteHostConnectionPool> {
        RemoteHostConnectionPool::new(RemoteHostConnectionPoolDependencies {
            ssh_hosts: Arc::new(OneHost),
            binaries: Arc::new(AgentBinaryCatalog::from_files(Vec::new()).unwrap()),
            transport_spawner: Arc::new(NoTransport),
            effective_ssh_config: effective,
        })
    }

    fn concurrent_endpoint_keys(
        pool: &Arc<RemoteHostConnectionPool>,
        count: usize,
    ) -> Vec<std::thread::JoinHandle<Result<String, String>>> {
        (0..count)
            .map(|_| {
                let pool = Arc::clone(pool);
                std::thread::spawn(move || pool.endpoint_key(&OneHost.find_host("host-a").unwrap()))
            })
            .collect()
    }

    #[test]
    fn concurrent_lookups_of_one_configuration_run_ssh_g_once_and_share_the_result() {
        let (release, released) = channel();
        let effective = Arc::new(HeldEffectiveConfig {
            calls: AtomicUsize::new(0),
            release: Mutex::new(released),
        });
        let pool = pool_with(Arc::clone(&effective));

        let lookups = concurrent_endpoint_keys(&pool, 4);
        std::thread::sleep(Duration::from_millis(100));
        release.send(Ok("user fixture\n".into())).unwrap();

        let keys: Vec<String> = lookups
            .into_iter()
            .map(|lookup| lookup.join().unwrap().unwrap())
            .collect();
        assert!(keys.iter().all(|key| *key == keys[0]));
        assert_eq!(effective.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_lookup_is_shared_by_its_waiters_and_retried_by_the_next_call() {
        let (release, released) = channel();
        let effective = Arc::new(HeldEffectiveConfig {
            calls: AtomicUsize::new(0),
            release: Mutex::new(released),
        });
        let pool = pool_with(Arc::clone(&effective));

        let lookups = concurrent_endpoint_keys(&pool, 3);
        std::thread::sleep(Duration::from_millis(100));
        release
            .send(Err("ssh -G timed out after 10 seconds".into()))
            .unwrap();
        for lookup in lookups {
            assert!(lookup.join().unwrap().is_err());
        }
        assert_eq!(effective.calls.load(Ordering::SeqCst), 1);

        release.send(Ok("user fixture\n".into())).unwrap();
        assert!(
            pool.endpoint_key(&OneHost.find_host("host-a").unwrap())
                .is_ok()
        );
        assert_eq!(effective.calls.load(Ordering::SeqCst), 2);
    }
}
