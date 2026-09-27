//! `grpc.channelz.v1.Channelz` service: the A14 query surface.
//!
//! [`ChannelzService`] answers the seven v1 RPCs from a [`Registry`]
//! snapshot: top-level channels and servers paginate by id (`start_*`
//! is inclusive; clients pass `last_id + 1`), single-entity lookups
//! answer `NOT_FOUND` for missing or dropped ids, and server
//! sockets page over that server's sockets in ascending id order.
//! `max_results <= 0`... precisely: zero selects
//! [`DEFAULT_PAGE_SIZE`], negative is `INVALID_ARGUMENT` (the proto
//! forbids it). `GetSocket(summary = true)` is accepted; we emit no
//! getsockopt options, so summary and full responses coincide.
//!
//! Conversion is mechanical and total: every entity field maps to
//! its proto peer, counters saturate at `i64::MAX`, unset timestamps
//! stay absent, and unknown connectivity reads back as `UNKNOWN`.

use super::Severity as ProtoSeverity;
use super::model::{
    ChannelId, Connectivity, EndpointAddr, Registry, ServerId, SocketId, SocketSecurity,
    SubchannelId,
};
use super::trace::{TraceChild, TraceSeverity, TraceSnapshot};
use super::{
    Address, Channel, ChannelConnectivityState, ChannelData, ChannelRef, ChannelTrace,
    ChannelTraceEvent, GetChannelResponse, GetServerResponse, GetServerSocketsResponse,
    GetServersResponse, GetSocketResponse, GetSubchannelResponse, GetTopChannelsResponse, Security,
    Server, ServerData, ServerRef, Socket, SocketData, SocketRef, State as ProtoState, Subchannel,
    SubchannelRef, TcpIpAddress, Timestamp, Tls, UdsAddress,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Page size when the client passes `max_results = 0` (A14 default).
pub const DEFAULT_PAGE_SIZE: usize = 100;

/// The channelz query service over one registry.
#[derive(Clone, Debug)]
pub struct ChannelzService {
    registry: Arc<Registry>,
}

impl ChannelzService {
    /// Serve `registry`.
    #[must_use]
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }

    /// Serve the process-global registry.
    #[must_use]
    pub fn shared_global() -> Self {
        Self::new(Registry::global_shared())
    }
}

/// Validate `max_results`: negative is rejected, zero selects the default.
fn page_size(max_results: i64) -> Result<usize, crate::status::Status> {
    if max_results < 0 {
        return Err(crate::status::Status::invalid_argument(
            "channelz: max_results must never be negative",
        ));
    }
    if max_results == 0 {
        return Ok(DEFAULT_PAGE_SIZE);
    }
    Ok(usize::try_from(max_results).unwrap_or(usize::MAX))
}

fn timestamp_at(time: SystemTime) -> Option<Timestamp> {
    let elapsed = time.duration_since(UNIX_EPOCH).ok()?;
    let mut out = Timestamp::new();
    out.set_seconds(i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX));
    out.set_nanos(i32::try_from(elapsed.subsec_nanos()).unwrap_or(i32::MAX));
    Some(out)
}

fn timestamp_ms(millis: u64) -> Option<Timestamp> {
    if millis == 0 {
        return None;
    }
    let mut out = Timestamp::new();
    out.set_seconds(i64::try_from(millis / 1000).unwrap_or(i64::MAX));
    let nanos = (millis % 1000) * 1_000_000;
    out.set_nanos(i32::try_from(nanos).unwrap_or(i32::MAX));
    Some(out)
}

fn counter(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn connectivity_state(state: Connectivity) -> ChannelConnectivityState {
    let mut out = ChannelConnectivityState::new();
    out.set_state(match state {
        Connectivity::Unknown => ProtoState::Unknown,
        Connectivity::Idle => ProtoState::Idle,
        Connectivity::Connecting => ProtoState::Connecting,
        Connectivity::Ready => ProtoState::Ready,
        Connectivity::TransientFailure => ProtoState::TransientFailure,
        Connectivity::Shutdown => ProtoState::Shutdown,
    });
    out
}

fn severity(severity: TraceSeverity) -> ProtoSeverity {
    match severity {
        TraceSeverity::Unknown => ProtoSeverity::CtUnknown,
        TraceSeverity::Info => ProtoSeverity::CtInfo,
        TraceSeverity::Warning => ProtoSeverity::CtWarning,
        TraceSeverity::Error => ProtoSeverity::CtError,
    }
}

fn trace_to_proto(snapshot: &TraceSnapshot) -> ChannelTrace {
    let mut out = ChannelTrace::new();
    out.set_num_events_logged(counter(snapshot.num_logged));
    if let Some(created) = timestamp_at(snapshot.created) {
        out.set_creation_timestamp(created);
    }
    let events = snapshot.events.iter().map(|event| {
        let mut proto = ChannelTraceEvent::new();
        proto.set_description(event.description.clone());
        proto.set_severity(severity(event.severity));
        if let Some(at) = timestamp_at(event.at) {
            proto.set_timestamp(at);
        }
        match &event.child {
            Some(TraceChild::Channel { id, name }) => {
                let mut child = ChannelRef::new();
                child.set_channel_id(counter(*id));
                child.set_name(name.clone());
                proto.set_channel_ref(child);
            }
            Some(TraceChild::Subchannel { id, name }) => {
                let mut child = SubchannelRef::new();
                child.set_subchannel_id(counter(*id));
                child.set_name(name.clone());
                proto.set_subchannel_ref(child);
            }
            None => {}
        }
        proto
    });
    out.set_events(events);
    out
}

fn addr_to_proto(addr: &EndpointAddr) -> Address {
    let mut out = Address::new();
    match addr {
        EndpointAddr::Tcp(sock) => {
            let mut tcp = TcpIpAddress::new();
            match sock.ip() {
                std::net::IpAddr::V4(ip) => tcp.set_ip_address(ip.octets().to_vec()),
                std::net::IpAddr::V6(ip) => tcp.set_ip_address(ip.octets().to_vec()),
            }
            tcp.set_port(i32::from(sock.port()));
            out.set_tcpip_address(tcp);
        }
        EndpointAddr::Uds(path) => {
            let mut uds = UdsAddress::new();
            uds.set_filename(path.clone());
            out.set_uds_address(uds);
        }
        EndpointAddr::Other(text) => {
            let mut other = super::OtherAddress::new();
            other.set_name(text.clone());
            out.set_other_address(other);
        }
    }
    out
}

fn security_to_proto(security: &SocketSecurity) -> Option<Security> {
    match security {
        SocketSecurity::None => None,
        SocketSecurity::Tls {
            local_certificate,
            remote_certificate,
        } => {
            let mut tls = Tls::new();
            if !local_certificate.is_empty() {
                tls.set_local_certificate(local_certificate.clone());
            }
            if !remote_certificate.is_empty() {
                tls.set_remote_certificate(remote_certificate.clone());
            }
            let mut out = Security::new();
            out.set_tls(tls);
            Some(out)
        }
    }
}

fn channel_ref(id: ChannelId, name: &str) -> ChannelRef {
    let mut out = ChannelRef::new();
    out.set_channel_id(counter(id.get()));
    out.set_name(name.to_owned());
    out
}

fn subchannel_ref(id: SubchannelId, name: &str) -> SubchannelRef {
    let mut out = SubchannelRef::new();
    out.set_subchannel_id(counter(id.get()));
    out.set_name(name.to_owned());
    out
}

fn socket_ref(id: SocketId, name: &str) -> SocketRef {
    let mut out = SocketRef::new();
    out.set_socket_id(counter(id.get()));
    out.set_name(name.to_owned());
    out
}

fn server_ref(id: ServerId, name: &str) -> ServerRef {
    let mut out = ServerRef::new();
    out.set_server_id(counter(id.get()));
    out.set_name(name.to_owned());
    out
}

impl super::Channelz for ChannelzService {
    async fn get_top_channels(
        &self,
        request: crate::Request<GetTopChannelsRequest>,
    ) -> Result<crate::Response<GetTopChannelsResponse>, crate::Status> {
        let query = request.get_ref();
        let max = page_size(query.max_results())?;
        let entries = self.registry.top_channels(query.start_channel_id(), max);
        let mut out = GetTopChannelsResponse::new();
        out.set_channel(
            entries
                .items
                .into_iter()
                .map(|entry| channel_to_proto(&entry)),
        );
        out.set_end(entries.end);
        Ok(crate::Response::new(out))
    }

    async fn get_servers(
        &self,
        request: crate::Request<GetServersRequest>,
    ) -> Result<crate::Response<GetServersResponse>, crate::Status> {
        let query = request.get_ref();
        let max = page_size(query.max_results())?;
        let entries = self.registry.servers(query.start_server_id(), max);
        let mut out = GetServersResponse::new();
        out.set_server(
            entries
                .items
                .into_iter()
                .map(|entry| server_to_proto(&entry)),
        );
        out.set_end(entries.end);
        Ok(crate::Response::new(out))
    }

    async fn get_server(
        &self,
        request: crate::Request<GetServerRequest>,
    ) -> Result<crate::Response<GetServerResponse>, crate::Status> {
        let query = request.get_ref();
        let Some(entry) = self.registry.server_by_wire_id(query.server_id()) else {
            return Err(crate::Status::not_found(format!(
                "channelz: no server {}",
                query.server_id()
            )));
        };
        let mut out = GetServerResponse::new();
        out.set_server(server_to_proto(&entry));
        Ok(crate::Response::new(out))
    }

    async fn get_server_sockets(
        &self,
        request: crate::Request<GetServerSocketsRequest>,
    ) -> Result<crate::Response<GetServerSocketsResponse>, crate::Status> {
        let query = request.get_ref();
        let max = page_size(query.max_results())?;
        let Some((refs, end)) =
            self.registry
                .server_sockets(query.server_id(), query.start_socket_id(), max)
        else {
            return Err(crate::Status::not_found(format!(
                "channelz: no server {}",
                query.server_id()
            )));
        };
        let mut out = GetServerSocketsResponse::new();
        out.set_socket_ref(refs.into_iter().map(|(id, name)| socket_ref(id, &name)));
        out.set_end(end);
        Ok(crate::Response::new(out))
    }

    async fn get_channel(
        &self,
        request: crate::Request<GetChannelRequest>,
    ) -> Result<crate::Response<GetChannelResponse>, crate::Status> {
        let query = request.get_ref();
        let Some(entry) = self.registry.channel_by_wire_id(query.channel_id()) else {
            return Err(crate::Status::not_found(format!(
                "channelz: no channel {}",
                query.channel_id()
            )));
        };
        let mut out = GetChannelResponse::new();
        out.set_channel(channel_to_proto(&entry));
        Ok(crate::Response::new(out))
    }

    async fn get_subchannel(
        &self,
        request: crate::Request<GetSubchannelRequest>,
    ) -> Result<crate::Response<GetSubchannelResponse>, crate::Status> {
        let query = request.get_ref();
        let Some(entry) = self.registry.subchannel_by_wire_id(query.subchannel_id()) else {
            return Err(crate::Status::not_found(format!(
                "channelz: no subchannel {}",
                query.subchannel_id()
            )));
        };
        let mut out = GetSubchannelResponse::new();
        out.set_subchannel(subchannel_to_proto(&entry));
        Ok(crate::Response::new(out))
    }

    async fn get_socket(
        &self,
        request: crate::Request<GetSocketRequest>,
    ) -> Result<crate::Response<GetSocketResponse>, crate::Status> {
        let query = request.get_ref();
        let Some(entry) = self.registry.socket_by_wire_id(query.socket_id()) else {
            return Err(crate::Status::not_found(format!(
                "channelz: no socket {}",
                query.socket_id()
            )));
        };
        // `summary` only omits getsockopt options, which we never
        // emit, so both shapes coincide.
        let mut out = GetSocketResponse::new();
        out.set_socket(socket_to_proto(&entry));
        Ok(crate::Response::new(out))
    }
}

use super::{
    GetChannelRequest, GetServerRequest, GetServerSocketsRequest, GetServersRequest,
    GetSocketRequest, GetSubchannelRequest, GetTopChannelsRequest,
};

fn channel_to_proto(entry: &super::model::ChannelEntry) -> Channel {
    let mut out = Channel::new();
    out.set_ref(channel_ref(entry.id, &entry.name));
    let mut data = ChannelData::new();
    data.set_state(connectivity_state(entry.state()));
    data.set_target(entry.target.clone());
    data.set_trace(trace_to_proto(&entry.trace_snapshot()));
    data.set_calls_started(counter(entry.calls_started()));
    data.set_calls_succeeded(counter(entry.calls_succeeded()));
    data.set_calls_failed(counter(entry.calls_failed()));
    if let Some(at) = timestamp_ms(entry.last_call_started_ms()) {
        data.set_last_call_started_timestamp(at);
    }
    out.set_data(data);
    out.set_subchannel_ref(
        entry
            .subchannel_children()
            .into_iter()
            .map(|(id, name)| subchannel_ref(id, &name)),
    );
    out.set_socket_ref(
        entry
            .socket_children()
            .into_iter()
            .map(|(id, name)| socket_ref(id, &name)),
    );
    out
}

fn subchannel_to_proto(entry: &super::model::SubchannelEntry) -> Subchannel {
    let mut out = Subchannel::new();
    out.set_ref(subchannel_ref(entry.id, &entry.name));
    let mut data = ChannelData::new();
    data.set_state(connectivity_state(entry.state()));
    data.set_target(entry.name.clone());
    data.set_trace(trace_to_proto(&entry.trace_snapshot()));
    data.set_calls_started(counter(entry.calls_started()));
    data.set_calls_succeeded(counter(entry.calls_succeeded()));
    data.set_calls_failed(counter(entry.calls_failed()));
    if let Some(at) = timestamp_ms(entry.last_call_started_ms()) {
        data.set_last_call_started_timestamp(at);
    }
    // One live connection per address subchannel, by pool construction.
    data.set_max_connections_per_subchannel(1);
    out.set_data(data);
    out.set_socket_ref(
        entry
            .socket_children()
            .into_iter()
            .map(|(id, name)| socket_ref(id, &name)),
    );
    out
}

fn server_to_proto(entry: &super::model::ServerEntry) -> Server {
    let mut out = Server::new();
    out.set_ref(server_ref(entry.id, &entry.name));
    let mut data = ServerData::new();
    data.set_trace(trace_to_proto(&entry.trace_snapshot()));
    data.set_calls_started(counter(entry.calls_started()));
    data.set_calls_succeeded(counter(entry.calls_succeeded()));
    data.set_calls_failed(counter(entry.calls_failed()));
    if let Some(at) = timestamp_ms(entry.last_call_started_ms()) {
        data.set_last_call_started_timestamp(at);
    }
    out.set_data(data);
    out.set_listen_socket(
        entry
            .listen_children()
            .into_iter()
            .map(|(id, name)| socket_ref(id, &name)),
    );
    out
}

fn socket_to_proto(entry: &super::model::SocketEntry) -> Socket {
    let mut out = Socket::new();
    out.set_ref(socket_ref(entry.id, &entry.name));
    let mut data = SocketData::new();
    data.set_streams_started(counter(entry.streams_started()));
    data.set_streams_succeeded(counter(entry.streams_succeeded()));
    data.set_streams_failed(counter(entry.streams_failed()));
    data.set_messages_sent(counter(entry.messages_sent()));
    data.set_messages_received(counter(entry.messages_received()));
    data.set_keep_alives_sent(counter(entry.keep_alives_sent()));
    if let Some(at) = timestamp_ms(entry.last_local_stream_ms()) {
        data.set_last_local_stream_created_timestamp(at);
    }
    if let Some(at) = timestamp_ms(entry.last_remote_stream_ms()) {
        data.set_last_remote_stream_created_timestamp(at);
    }
    if let Some(at) = timestamp_ms(entry.last_message_sent_ms()) {
        data.set_last_message_sent_timestamp(at);
    }
    if let Some(at) = timestamp_ms(entry.last_message_received_ms()) {
        data.set_last_message_received_timestamp(at);
    }
    // Flow-control windows, socket options, GOAWAY codes, and peer
    // stream caps stay absent: the h2 facade does not expose them.
    out.set_data(data);
    out.set_local(addr_to_proto(&entry.local));
    if let Some(remote) = &entry.remote {
        out.set_remote(addr_to_proto(remote));
    }
    if let Some(security) = security_to_proto(&entry.security) {
        out.set_security(security);
    }
    if let Some(name) = &entry.remote_name {
        out.set_remote_name(name.clone());
    }
    out
}
