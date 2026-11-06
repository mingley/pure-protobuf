//! URI-keyed endpoint pools. Resolver address identity remains unchanged.

use super::*;
use crate::client::channel::{Endpoint as ClientEndpoint, balance_inputs};
use crate::lb::{KeyedPick, KeyedRoundRobin};

struct Route {
    id: usize,
    uri: http::Uri,
    address: ResolvedAddress,
    executor: crate::executor::TaskExecutor,
}

#[derive(Default)]
struct Snapshot {
    ordered: Vec<Arc<Route>>,
    by_id: HashMap<usize, Arc<Route>>,
}

impl Snapshot {
    fn ids(&self) -> Vec<usize> {
        self.ordered.iter().map(|route| route.id).collect()
    }
}

type Entries = Arc<Mutex<HashMap<usize, Arc<Mutex<ConnSlot>>>>>;

pub(crate) struct Balanced {
    // Request cancellation before releasing the channel's other state.
    _task: Option<ResolverTask>,
    pub(super) display: &'static str,
    policy: Arc<KeyedRoundRobin<usize>>,
    current: watch::Receiver<Arc<Snapshot>>,
    entries: Entries,
}

/// Normalize full URI keys, preserving first position and last value.
fn snapshot(
    inputs: Vec<BalanceInput>,
    previous: &Snapshot,
    next_id: &mut usize,
) -> Result<Snapshot, Status> {
    let mut positions = HashMap::new();
    let mut ordered: Vec<BalanceInput> = Vec::with_capacity(inputs.len());
    for input in inputs {
        match positions.get(&input.uri).copied() {
            Some(index) => {
                let old = ordered
                    .get_mut(index)
                    .ok_or_else(|| Status::internal("balanced endpoint position disappeared"))?;
                *old = input;
            }
            None => {
                positions.insert(input.uri.clone(), ordered.len());
                ordered.push(input);
            }
        }
    }
    let old: HashMap<_, _> = previous
        .ordered
        .iter()
        .map(|route| (&route.uri, route))
        .collect();
    let mut result = Snapshot::default();
    for input in ordered {
        let address = ResolvedAddress::Tcp(input.address);
        let route = match old.get(&input.uri) {
            Some(route)
                if route.address == address && route.executor.same_instance(&input.executor) =>
            {
                Arc::clone(route)
            }
            _ => {
                let id = *next_id;
                *next_id = next_id.checked_add(1).ok_or_else(|| {
                    Status::resource_exhausted("balanced endpoint identity space exhausted")
                })?;
                Arc::new(Route {
                    id,
                    uri: input.uri,
                    address,
                    executor: input.executor,
                })
            }
        };
        result.by_id.insert(route.id, Arc::clone(&route));
        result.ordered.push(route);
    }
    Ok(result)
}

pub(in crate::client) async fn connect(
    endpoints: Vec<ClientEndpoint>,
    updates: Option<watch::Receiver<Vec<ClientEndpoint>>>,
    config: ChannelConfig,
) -> Result<Channel, Status> {
    if endpoints.is_empty() {
        return Err(Status::invalid_argument(
            "balance_list needs at least one endpoint",
        ));
    }
    let mut next_id = 1;
    let initial = Arc::new(snapshot(
        balance_inputs(&endpoints)?,
        &Snapshot::default(),
        &mut next_id,
    )?);
    let policy = Arc::new(KeyedRoundRobin::new());
    policy.update(initial.ids()).await;
    let (publish, current) = watch::channel(Arc::clone(&initial));
    let entries: Entries = Arc::new(Mutex::new(HashMap::new()));
    let (display, authority) = if updates.is_some() {
        (
            "dns:///pbrs-endpoint-channel.invalid:443",
            "pbrs-endpoint-channel.invalid:443",
        )
    } else {
        (
            "dns:///pbrs-endpoint-list.invalid:443",
            "pbrs-endpoint-list.invalid:443",
        )
    };
    let channelz = crate::channelz::Registry::global_shared().register_channel(display, display);
    let channel_id = channelz.id();
    let task = match updates {
        Some(updates) => Some(ResolverTask::new(tokio::spawn(update_loop(
            updates,
            publish,
            initial,
            Arc::clone(&policy),
            Arc::clone(&entries),
            next_id,
            config.age_grace(),
            channel_id,
        )))),
        None => {
            drop(publish);
            None
        }
    };
    let state = Arc::new(Balanced {
        display,
        policy,
        current,
        entries,
        _task: task,
    });
    let shared = SharedServiceConfig::default();
    shared.set(ServiceConfig::parse(
        r#"{"loadBalancingConfig":[{"round_robin":{}}]}"#,
    )?);
    Ok(finish_channel(
        Endpoint::Balanced(state),
        Authority::from_static(authority),
        config,
        None,
        Vec::new(),
        None,
        shared,
        None,
        channelz,
    ))
}

#[allow(
    clippy::too_many_arguments,
    reason = "one owned endpoint watch worker and its publication/drain state"
)]
async fn update_loop(
    mut updates: watch::Receiver<Vec<ClientEndpoint>>,
    publish: watch::Sender<Arc<Snapshot>>,
    mut previous: Arc<Snapshot>,
    policy: Arc<KeyedRoundRobin<usize>>,
    entries: Entries,
    mut next_id: usize,
    grace: Duration,
    channel: crate::channelz::ChannelId,
) {
    while updates.changed().await.is_ok() {
        let endpoints = updates.borrow_and_update().clone();
        let next = match balance_inputs(&endpoints)
            .and_then(|inputs| snapshot(inputs, &previous, &mut next_id))
        {
            Ok(next) => Arc::new(next),
            Err(status) => {
                crate::channelz::Registry::global().trace_channel(
                    channel,
                    crate::channelz::TraceSeverity::Warning,
                    format!(
                        "Endpoint list update rejected; retaining last valid list: {}",
                        status.message()
                    ),
                );
                continue;
            }
        };
        let ids = next.ids();
        if ids == previous.ids() {
            continue;
        }
        // Publish immutable routing metadata before making its identities
        // pickable. A late pick for a removed identity re-picks, never falling
        // back to the default executor or another entry's metadata.
        if publish.send(Arc::clone(&next)).is_err() {
            return;
        }
        policy.update(ids).await;
        let removed = {
            let mut slots = entries.lock().await;
            let old: Vec<_> = slots
                .keys()
                .filter(|id| !next.by_id.contains_key(id))
                .copied()
                .collect();
            old.into_iter().filter_map(|id| slots.remove(&id)).collect()
        };
        drain_entries(removed, grace).await;
        crate::channelz::Registry::global().trace_channel(
            channel,
            crate::channelz::TraceSeverity::Info,
            format!("Endpoint list updated: {} URI entries", next.ordered.len()),
        );
        previous = next;
    }
}

impl Balanced {
    pub(super) async fn discard(&self, id: usize, generation: u64) {
        let entry = self.entries.lock().await.get(&id).cloned();
        let Some(entry) = entry else {
            return;
        };
        let mut slot = entry.lock().await;
        if slot.r#gen != generation {
            return;
        }
        slot.send = None;
        slot.channelz_socket = None;
        slot.busy = None;
        if let Some(stop) = slot.stop.take() {
            stop.send(true).ok();
        }
        slot.r#gen = slot.r#gen.wrapping_add(1);
    }

    async fn acquire(
        &self,
        inner: &Arc<ChannelInner>,
        wait_for_ready: bool,
        observer: Option<&dyn LifecycleObserver>,
        health: Option<HealthDirective>,
    ) -> Result<LiveConn, Status> {
        let mut changes = self.policy.watch();
        let mut attempt = 0usize;
        loop {
            let id = match self.policy.pick().await {
                KeyedPick::Use(id) => id,
                KeyedPick::Wait => {
                    if !wait_for_ready {
                        return Err(Status::unavailable("balance: no ready endpoint"));
                    }
                    changes.changed().await.ok();
                    continue;
                }
                KeyedPick::Fail(status) => {
                    if !wait_for_ready {
                        return Err(status);
                    }
                    tokio::select! {
                        _ = changes.changed() => {}
                        () = tokio::time::sleep(Duration::from_millis(20)) => {}
                    }
                    continue;
                }
            };
            let route = self.current.borrow().by_id.get(&id).cloned();
            let Some(route) = route else {
                changes.changed().await.ok();
                continue;
            };
            let entry = {
                let mut entries = self.entries.lock().await;
                // Synchronize insertion with retirement. If a snapshot was
                // removed before this lock, do not recreate an orphan entry;
                // if removal races after the check, its reconciliation still
                // waits on this lock and will retire the inserted entry.
                if !self.current.borrow().by_id.contains_key(&id) {
                    None
                } else {
                    Some(Arc::clone(entries.entry(id).or_insert_with(|| {
                        Arc::new(Mutex::new(ConnSlot {
                            r#gen: 0,
                            send: None,
                            stop: None,
                            busy: None,
                            address: Some(route.address.clone()),
                            channelz_socket: None,
                            channelz_subchannel: None,
                        }))
                    })))
                }
            };
            let Some(entry) = entry else {
                changes.changed().await.ok();
                continue;
            };
            let (send, lease, generation, driver, socket) = {
                let slot = entry.lock().await;
                (
                    slot.send.clone(),
                    slot.busy.as_ref().map(crate::keepalive::Busy::start),
                    slot.r#gen,
                    slot.stop.clone(),
                    socket_id_of(&slot),
                )
            };
            if let Some(send) = send {
                if let Ok(ready) = send.ready().await {
                    if self
                        .health_ok(inner, id, &ready, &driver, health.as_ref())
                        .await
                    {
                        return Ok(LiveConn {
                            send: ready,
                            load: None,
                            lease,
                            driver,
                            slot: id,
                            r#gen: generation,
                            rr_addr: None,
                            channelz_socket: socket,
                        });
                    }
                    continue;
                }
            }
            drop(lease);
            inner.note_dial_start(&entry, &route.address).await;
            let started = tokio::time::Instant::now();
            let dial = dial_resolved_with_executor(
                self.display,
                route.address.clone(),
                inner.dial,
                None,
                &route.executor,
            );
            let result = match tokio::time::timeout(inner.dial.handshake_timeout(), dial).await {
                Ok(result) => result,
                Err(_) => Err(Status::unavailable(format!(
                    "connect {}: timed out",
                    route.uri
                ))),
            };
            match result {
                Ok(dialed) => {
                    let mut slot = entry.lock().await;
                    // Removed entries cannot install a late handshake. URI
                    // replacements have a different id, so old RPC failures
                    // cannot discard the replacement's connection either.
                    if !self.current.borrow().by_id.contains_key(&id) || slot.r#gen != generation {
                        dialed.stop.send(true).ok();
                        continue;
                    }
                    let local = dialed.local_addr.clone();
                    let peer = dialed.peer_addr.clone();
                    let send = store_dialed(&mut slot, dialed);
                    slot.busy.get_or_insert_with(crate::keepalive::Busy::new);
                    let lease = slot.busy.as_ref().map(crate::keepalive::Busy::start);
                    let driver = slot.stop.clone();
                    let generation = slot.r#gen;
                    let socket =
                        Some(inner.note_dial_ok(&mut slot, Some(&route.address), peer, local));
                    drop(slot);
                    self.policy.note_success(&id).await;
                    if let Some(observer) = observer {
                        if generation > 1 || attempt > 0 {
                            observer.on_reconnect(&ReconnectEvent {
                                target: self.display,
                                attempt: u32::try_from(attempt)
                                    .unwrap_or(u32::MAX)
                                    .saturating_add(1),
                                duration: started.elapsed(),
                                status: None,
                            });
                        }
                    }
                    if !self
                        .health_ok(inner, id, &send, &driver, health.as_ref())
                        .await
                    {
                        continue;
                    }
                    return Ok(LiveConn {
                        send,
                        load: None,
                        lease,
                        driver,
                        slot: id,
                        r#gen: generation,
                        rr_addr: None,
                        channelz_socket: socket,
                    });
                }
                Err(status) => {
                    if !self.current.borrow().by_id.contains_key(&id) {
                        continue;
                    }
                    self.policy.note_failure(&id, status.clone()).await;
                    inner
                        .note_dial_err(Some(&entry), Some(&route.address), &status)
                        .await;
                    if let Some(observer) = observer {
                        observer.on_reconnect(&ReconnectEvent {
                            target: self.display,
                            attempt: u32::try_from(attempt).unwrap_or(u32::MAX).saturating_add(1),
                            duration: started.elapsed(),
                            status: Some(status.code()),
                        });
                    }
                    if !wait_for_ready {
                        return Err(status);
                    }
                    let delay = WAIT_FOR_READY_BACKOFF_MS
                        .get(attempt)
                        .copied()
                        .unwrap_or(1000);
                    attempt = attempt.saturating_add(1);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }

    async fn health_ok(
        &self,
        inner: &ChannelInner,
        id: usize,
        send: &backend::SendRequest,
        driver: &Option<watch::Sender<bool>>,
        directive: Option<&HealthDirective>,
    ) -> bool {
        let Some(directive) = directive else {
            return true;
        };
        let Some(mut stop) = driver.as_ref().map(watch::Sender::subscribe) else {
            return true;
        };
        if !self.current.borrow().by_id.contains_key(&id) {
            return false;
        }
        let Some(pending) = admit_health_watch(&self.entries, &self.policy, &id, &stop).await
        else {
            return false;
        };
        if pending {
            let target = HealthTarget::Endpoint {
                policy: Arc::clone(&self.policy),
                id,
            };
            if spawn_health_watch(inner, target, send.clone(), stop.clone(), directive).is_err() {
                self.policy.health_gone_from(&id, &stop).await;
                return false;
            }
        }
        let mut changes = self.policy.watch();
        loop {
            if *stop.borrow() || !self.current.borrow().by_id.contains_key(&id) {
                return false;
            }
            if let Some(signal) = self.policy.health_of(&id).await {
                return signal == HealthSignal::Healthy;
            }
            tokio::select! {
                changed = changes.changed() => { if changed.is_err() { return false; } }
                _ = stop.changed() => return false,
            }
        }
    }
}

#[inline(never)]
pub(super) fn acquire<'a>(
    inner: &'a Arc<ChannelInner>,
    state: &'a Arc<Balanced>,
    wait_for_ready: bool,
    observer: Option<&'a dyn LifecycleObserver>,
    health: Option<HealthDirective>,
) -> Pin<Box<impl Future<Output = Result<LiveConn, Status>> + 'a>> {
    Box::pin(state.acquire(inner, wait_for_ready, observer, health))
}
