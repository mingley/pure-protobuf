use super::recv::RecvHeaderBlockError;
use super::store::{self, Entry, Resolve, Store};
use super::{Buffer, BufferStatus, Config, Counts, Prioritized, Recv, Send, Stream, StreamId};
use crate::h2_backend::codec::{Codec, SendError, UserError};
use crate::h2_backend::ext::Protocol;
use crate::h2_backend::frame::{self, Frame, Reason};
use crate::h2_backend::proto::{Error, Initiator, Open, Peer, WindowSize, peer};
use crate::h2_backend::{client, proto, server};

use bytes::{Buf, Bytes};
use http::{HeaderMap, Request, Response};
use std::task::{Context, Poll, Waker};
use tokio::io::AsyncWrite;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::lock::Mutex;
use std::{fmt, io};

#[derive(Debug)]
pub(crate) struct Streams<B, P>
where
    P: Peer,
{
    /// Holds most of the connection and stream related state for processing
    /// HTTP/2 frames associated with streams.
    inner: Arc<Mutex<Inner>>,

    /// This is the queue of frames to be written to the wire. This is split out
    /// to avoid requiring a `B` generic on all public API types even if `B` is
    /// not technically required.
    ///
    /// Currently, splitting this out requires a second `Arc` + `Mutex`.
    /// However, it should be possible to avoid this duplication with a little
    /// bit of unsafe code. This optimization has been postponed until it has
    /// been shown to be necessary.
    send_buffer: Arc<SendBuffer<B>>,

    /// Set when a remote stream was refused and the RST_STREAM is not yet
    /// buffered. `send_pending_refusal` skips the stream lock when this is clear.
    refusal_pending: Arc<AtomicBool>,

    /// Set when a locally reset stream is waiting to be reaped.
    /// `clear_expired_reset_streams` skips the stream lock when this is clear.
    reset_pending: Arc<AtomicBool>,

    /// Set while `Counts` has a send or receive stream. Lets
    /// `has_streams_or_other_references` return without the stream lock.
    streams_open: Arc<AtomicBool>,

    /// Set when `poll_complete` has observed an idle send side and stored
    /// the driver waker. Cleared before a frame or window update is queued.
    buffer_idle: Arc<AtomicBool>,

    _p: ::std::marker::PhantomData<P>,
}

// Like `Streams` but with a `peer::Dyn` field instead of a static `P: Peer` type parameter.
// Ensures that the methods only get one instantiation, instead of two (client and server)
#[derive(Debug)]
pub(crate) struct DynStreams<'a, B> {
    inner: &'a Mutex<Inner>,

    send_buffer: &'a SendBuffer<B>,

    peer: peer::Dyn,
}

/// Reference to the stream state
#[derive(Debug)]
pub(crate) struct StreamRef<B> {
    opaque: OpaqueStreamRef,
    send_buffer: Arc<SendBuffer<B>>,
}

/// Reference to the stream state that hides the send data chunk generic
pub(crate) struct OpaqueStreamRef {
    inner: Arc<Mutex<Inner>>,
    key: store::Key,
}

/// Fields needed to manage state related to managing the set of streams. This
/// is mostly split out to make ownership happy.
///
/// TODO: better name
#[derive(Debug)]
struct Inner {
    /// Tracks send & recv stream concurrency.
    counts: Counts,

    /// Connection level state and performs actions on streams
    actions: Actions,

    /// Stores stream state
    store: Store,

    /// The number of stream refs to this shared state.
    refs: usize,

    /// Shared with [`Streams::refusal_pending`].
    refusal_pending: Arc<AtomicBool>,

    /// Shared with [`Streams::reset_pending`].
    reset_pending: Arc<AtomicBool>,
}

#[derive(Debug)]
struct Actions {
    /// Manages state transitions initiated by receiving frames
    recv: Recv,

    /// Manages state transitions initiated by sending frames
    send: Send,

    /// Task that calls `poll_complete`.
    task: Option<Waker>,

    /// If the connection errors, a copy is kept for any StreamRefs.
    conn_error: Option<proto::Error>,
}

/// Contains the buffer of frames to be written to the wire.
#[derive(Debug)]
struct SendBuffer<B> {
    inner: Mutex<Buffer<Frame<B>>>,
}

// ===== impl Streams =====

impl<B, P> Streams<B, P>
where
    B: Buf,
    P: Peer,
{
    pub fn new(config: Config) -> Self {
        let peer = P::r#dyn();

        let (inner, refusal_pending, reset_pending, streams_open, buffer_idle) =
            Inner::new(peer, config);
        Streams {
            inner,
            send_buffer: Arc::new(SendBuffer::new()),
            refusal_pending,
            reset_pending,
            streams_open,
            buffer_idle,
            _p: ::std::marker::PhantomData,
        }
    }

    pub fn set_target_connection_window_size(&mut self, size: WindowSize) -> Result<(), Reason> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        me.actions
            .recv
            .set_target_connection_window(size, &mut me.actions.task)
    }

    pub fn next_incoming(&mut self) -> Option<StreamRef<B>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;
        let (key, remote_reset) = me.actions.recv.next_incoming(&mut me.store)?;
        // TODO: ideally, OpaqueStreamRefs::new would do this, but we're holding
        // the lock, so it can't.
        me.refs += 1;

        // Pending-accepted remotely-reset streams are counted.
        if remote_reset {
            me.counts.dec_num_remote_reset_streams();
        }

        // The refcount was incremented through the slot `next_incoming` popped.
        Some(StreamRef {
            opaque: OpaqueStreamRef {
                inner: self.inner.clone(),
                key,
            },
            send_buffer: self.send_buffer.clone(),
        })
    }

    #[inline]
    pub fn send_pending_refusal<T>(
        &mut self,
        cx: &mut Context,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> Poll<io::Result<()>>
    where
        T: AsyncWrite + Unpin,
    {
        if !self.refusal_pending.load(Ordering::Acquire) {
            return Poll::Ready(Ok(()));
        }
        self.send_pending_refusal_cold(cx, dst)
    }

    #[cold]
    #[inline(never)]
    fn send_pending_refusal_cold<T>(
        &mut self,
        cx: &mut Context,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> Poll<io::Result<()>>
    where
        T: AsyncWrite + Unpin,
    {
        loop {
            let status = {
                let mut me = self.inner.lock().unwrap();
                let me = &mut *me;
                let status = me.actions.recv.send_pending_refusal(dst)?;
                if !me.actions.recv.has_pending_refusal() {
                    me.refusal_pending.store(false, Ordering::Release);
                }
                status
            };

            match status {
                BufferStatus::Complete | BufferStatus::Idle => return Poll::Ready(Ok(())),
                BufferStatus::CodecFull => ready!(dst.poll_ready(cx))?,
            }
        }
    }

    #[inline]
    pub fn clear_expired_reset_streams(&mut self) {
        if self.reset_pending.load(Ordering::Acquire) {
            self.clear_expired_reset_streams_cold();
        }
    }

    #[cold]
    #[inline(never)]
    fn clear_expired_reset_streams_cold(&mut self) {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;
        me.actions
            .recv
            .clear_expired_reset_streams(&mut me.store, &mut me.counts);
        if me.actions.recv.reset_queue_is_empty() {
            self.reset_pending.store(false, Ordering::Release);
        }
    }

    pub fn poll_complete<T>(
        &mut self,
        cx: &mut Context,
        dst: &mut Codec<T, Prioritized<B>>,
        armed: bool,
    ) -> Poll<io::Result<()>>
    where
        T: AsyncWrite + Unpin,
    {
        // Nothing queued since the last idle observation, and the codec is
        // not holding a write. The driver waker was stored on that observation.
        if self.buffer_idle.load(Ordering::Acquire) && !dst.has_pending_flush() {
            return Poll::Ready(Ok(()));
        }
        loop {
            // Make any required socket progress before taking stream locks.
            ready!(dst.poll_ready(cx))?;

            let status = {
                let mut me = self.inner.lock().unwrap();
                let status = me.buffer_pending(&self.send_buffer, dst)?;

                // Register the task while holding the same lock used to
                // observe that all pending frames have been buffered. A
                // producer that queues another frame while the codec is being
                // flushed will then take and wake this task. The preface
                // poll is not armed, so it does not keep the connector
                // waker. The driver keeps the waker it already stored.
                if armed
                    && matches!(status, BufferStatus::Complete | BufferStatus::Idle)
                    && me.actions.task.is_none()
                {
                    me.actions.task = Some(cx.waker().clone());
                }
                // Set while the lock is still held. A producer clears the
                // flag before it wakes, so it cannot sneak in between the
                // idle observation and this store. `has_pending_flush` keeps
                // the next poll from skipping a write that is still buffered.
                if armed && me.actions.task.is_some() && matches!(status, BufferStatus::Idle) {
                    self.buffer_idle.store(true, Ordering::Release);
                }

                status
            };

            match status {
                // Nothing was staged and no data frame is in flight, so the
                // reclaim lock cannot observe one. Flush still runs: a previous
                // write may be waiting on the socket.
                BufferStatus::Idle => {
                    ready!(dst.flush(cx))?;
                    return Poll::Ready(Ok(()));
                }
                BufferStatus::Complete => {}
                BufferStatus::CodecFull => continue,
            }

            // Flush any frames staged by `buffer_pending` without holding the
            // stream-state or send-buffer mutexes.
            ready!(dst.flush(cx))?;

            // Header and trailer flushes leave no data frame. Reclaim would
            // only take the stream lock to observe that.
            if !dst.has_last_data_frame() {
                return Poll::Ready(Ok(()));
            }

            let reclaimed = {
                let mut me = self.inner.lock().unwrap();
                me.reclaim_written_frame(&self.send_buffer, dst)
            };

            if !reclaimed {
                return Poll::Ready(Ok(()));
            }
        }
    }

    pub fn apply_remote_settings(
        &mut self,
        frame: &frame::Settings,
        is_initial: bool,
    ) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        me.counts.apply_remote_settings(frame, is_initial);

        me.actions.send.apply_remote_settings(
            frame,
            send_buffer,
            &mut me.store,
            &mut me.counts,
            &mut me.actions.task,
        )
    }

    pub fn apply_local_settings(&mut self, frame: &frame::Settings) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        me.actions.recv.apply_local_settings(frame, &mut me.store)
    }

    pub fn send_request(
        &mut self,
        mut request: Request<()>,
        end_of_stream: bool,
        pending: Option<&OpaqueStreamRef>,
    ) -> Result<(StreamRef<B>, bool), SendError> {
        use super::stream::ContentLength;
        use http::Method;

        let protocol = request.extensions_mut().remove::<Protocol>();

        // Clear before taking lock, incase extensions contain a StreamRef.
        request.extensions_mut().clear();

        // TODO: There is a hazard with assigning a stream ID before the
        // prioritize layer. If prioritization reorders new streams, this
        // implicitly closes the earlier stream IDs.
        //
        // See: hyperium/h2#11
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        me.actions.ensure_no_conn_error()?;
        me.actions.send.ensure_next_stream_id()?;

        // The `pending` argument is provided by the `Client`, and holds
        // a store `Key` of a `Stream` that may have been not been opened
        // yet.
        //
        // If that stream is still pending, the Client isn't allowed to
        // queue up another pending stream. They should use `poll_ready`.
        if let Some(stream) = pending {
            if me.store.resolve(stream.key).is_pending_open {
                return Err(UserError::Rejected.into());
            }
        }

        if me.counts.peer().is_server() {
            // Servers cannot open streams. PushPromise must first be reserved.
            return Err(UserError::UnexpectedFrameType.into());
        }

        let stream_id = me.actions.send.open()?;
        let head = *request.method() == Method::HEAD;
        let send_window = me.actions.send.init_window_sz();
        let recv_window = me.actions.recv.init_window_sz();

        // Convert the message
        let headers =
            client::Peer::convert_send_message(stream_id, request, protocol, end_of_stream)?;

        let mut stream = me.store.insert_new(stream_id, send_window, recv_window);
        if head {
            stream.content_length = ContentLength::Head;
        }
        // Headers and the single handle share the slot the insert just wrote.
        let slot = stream.stream_ptr();

        let sent = me.actions.send.send_headers_boxed(
            headers,
            send_buffer,
            &mut stream,
            slot,
            &mut me.counts,
            &mut me.actions.task,
        );

        // send_headers can return a UserError, if it does,
        // we should forget about this stream.
        if let Err(err) = sent {
            stream.unlink();
            stream.remove();
            return Err(err.into());
        }

        // Given that the stream has been initialized, it should not be in the
        // closed state.
        debug_assert!(!stream.state.is_closed());

        // TODO: ideally, OpaqueStreamRefs::new would do this, but we're holding
        // the lock, so it can't.
        me.refs += 1;
        #[allow(
            unsafe_code,
            reason = "the new handle's refcount uses the slot the insert just wrote"
        )]
        unsafe {
            (*slot).ref_inc();
        }

        let is_full = me.counts.next_send_stream_will_reach_capacity();
        Ok((
            StreamRef {
                opaque: OpaqueStreamRef {
                    inner: self.inner.clone(),
                    key: stream.key(),
                },
                send_buffer: self.send_buffer.clone(),
            },
            is_full,
        ))
    }

    pub fn poll_send_request(
        &mut self,
        request: &mut Option<Request<()>>,
        end_of_stream: bool,
        cx: &Context<'_>,
        waiter: &mut Option<std::sync::Arc<atomic_waker::AtomicWaker>>,
    ) -> Poll<Result<(StreamRef<B>, OpaqueStreamRef, bool), crate::h2_backend::Error>> {
        use super::stream::ContentLength;
        use http::Method;

        // Declare before the lock so any extension-owned StreamRef is dropped
        // after the lock, including on early error returns.
        let mut deferred_extensions = http::Extensions::new();

        // TODO: There is a hazard with assigning a stream ID before the
        // prioritize layer. If prioritization reorders new streams, this
        // implicitly closes the earlier stream IDs.
        //
        // See: hyperium/h2#11
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        me.actions.ensure_no_conn_error()?;
        let stream_id = me.actions.send.ensure_next_stream_id()?;

        if !me.counts.can_reserve_send_stream() {
            me.counts.wait_send_capacity(cx, waiter);
            return Poll::Pending;
        }
        let mut request = request
            .take()
            .expect("request future polled after completion");
        std::mem::swap(request.extensions_mut(), &mut deferred_extensions);
        let protocol = deferred_extensions.remove::<Protocol>();

        // The peer is a type parameter. The client build drops this branch.
        if P::r#dyn().is_server() {
            // Servers cannot open streams. PushPromise must first be reserved.
            return Poll::Ready(Err(UserError::UnexpectedFrameType.into()));
        }

        // The id was checked before the request was taken. Bump it once.
        me.actions.send.commit_open_id(stream_id);
        let head = *request.method() == Method::HEAD;
        let send_window = me.actions.send.init_window_sz();
        let recv_window = me.actions.recv.init_window_sz();

        // Convert the message
        let headers =
            client::Peer::convert_send_message(stream_id, request, protocol, end_of_stream)?;

        let mut stream = me.store.insert_new(stream_id, send_window, recv_window);
        if head {
            stream.content_length = ContentLength::Head;
        }
        // Headers and both handles share the slot the insert just wrote.
        let slot = stream.stream_ptr();

        let sent = me.actions.send.send_headers_boxed(
            headers,
            send_buffer,
            &mut stream,
            slot,
            &mut me.counts,
            &mut me.actions.task,
        );

        // send_headers can return a UserError, if it does,
        // we should forget about this stream.
        if let Err(err) = sent {
            stream.unlink();
            stream.remove();
            return Poll::Ready(Err(err.into()));
        }

        // The reservation remains authoritative while this mutex is held.
        // The driver may transfer it to the active count immediately after
        // unlocking, so callers must not assert this transient state later.
        debug_assert!(stream.is_pending_open);

        // Given that the stream has been initialized, it should not be in the
        // closed state.
        debug_assert!(!stream.state.is_closed());

        // The response future and the send stream each own a handle. Building
        // both here avoids a second lock and slab lookup in `clone_to_opaque`.
        me.refs += 2;
        #[allow(
            unsafe_code,
            reason = "both new handles share the slot the insert just wrote"
        )]
        unsafe {
            (*slot).ref_inc();
            (*slot).ref_inc();
        }
        let key = stream.key();

        let is_full = me.counts.next_send_stream_will_reach_capacity();
        let response = OpaqueStreamRef {
            inner: self.inner.clone(),
            key,
        };
        Poll::Ready(Ok((
            StreamRef {
                opaque: OpaqueStreamRef {
                    inner: self.inner.clone(),
                    key,
                },
                send_buffer: self.send_buffer.clone(),
            },
            response,
            is_full,
        )))
    }

    /// [`poll_send_request`] for a repeated stock POST.
    ///
    /// The caller already matched the saved pseudos. Taking a `Request<()>`
    /// here copied 216 bytes out of the admission future and then dropped it.
    /// The admission poll wraps this result immediately. Out of line, the
    /// stream handles were copied into the return slot and back out.
    #[inline(always)]
    pub fn poll_send_stock(
        &mut self,
        _end_of_stream: bool,
        cx: &Context<'_>,
        waiter: &mut Option<std::sync::Arc<atomic_waker::AtomicWaker>>,
    ) -> Poll<Result<(StreamRef<B>, OpaqueStreamRef, bool), crate::h2_backend::Error>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        me.actions.ensure_no_conn_error()?;
        let stream_id = me.actions.send.ensure_next_stream_id()?;

        if !me.counts.can_reserve_send_stream() {
            me.counts.wait_send_capacity(cx, waiter);
            return Poll::Pending;
        }

        // The peer is a type parameter. The client build drops this branch.
        if P::r#dyn().is_server() {
            return Poll::Ready(Err(UserError::UnexpectedFrameType.into()));
        }

        me.actions.send.commit_open_id(stream_id);
        // The mark must not leak onto a later non-stock send.
        let _ = crate::h2_backend::header_pool::take_next_stock(
            crate::h2_backend::header_pool::STOCK_REQUEST,
        );
        let Some(headers) = frame::Headers::cached_stock_post(stream_id) else {
            return Poll::Ready(Err(UserError::MissingUriSchemeAndAuthority.into()));
        };
        // gRPC sends a body after the headers. END_STREAM would be a different frame.
        debug_assert!(!_end_of_stream);
        let send_window = me.actions.send.init_window_sz();
        let recv_window = me.actions.recv.init_window_sz();

        let mut stream = me.store.insert_new(stream_id, send_window, recv_window);
        let slot = stream.stream_ptr();

        let sent = me.actions.send.send_headers_boxed(
            headers,
            send_buffer,
            &mut stream,
            slot,
            &mut me.counts,
            &mut me.actions.task,
        );

        if let Err(err) = sent {
            stream.unlink();
            stream.remove();
            return Poll::Ready(Err(err.into()));
        }

        debug_assert!(stream.is_pending_open);
        debug_assert!(!stream.state.is_closed());

        me.refs += 2;
        #[allow(
            unsafe_code,
            reason = "both new handles share the slot the insert just wrote"
        )]
        unsafe {
            (*slot).ref_inc();
            (*slot).ref_inc();
        }
        let key = stream.key();

        let is_full = me.counts.next_send_stream_will_reach_capacity();
        let response = OpaqueStreamRef {
            inner: self.inner.clone(),
            key,
        };
        Poll::Ready(Ok((
            StreamRef {
                opaque: OpaqueStreamRef {
                    inner: self.inner.clone(),
                    key,
                },
                send_buffer: self.send_buffer.clone(),
            },
            response,
            is_full,
        )))
    }

    pub(crate) fn is_extended_connect_protocol_enabled(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .actions
            .send
            .is_extended_connect_protocol_enabled()
    }

    pub fn current_max_send_streams(&self) -> usize {
        let me = self.inner.lock().unwrap();
        me.counts.max_send_streams()
    }

    pub fn current_max_recv_streams(&self) -> usize {
        let me = self.inner.lock().unwrap();
        me.counts.max_recv_streams()
    }
}

impl<B> DynStreams<'_, B> {
    pub fn is_buffer_empty(&self) -> bool {
        self.send_buffer.is_empty()
    }

    pub fn is_server(&self) -> bool {
        self.peer.is_server()
    }

    pub fn recv_headers(&mut self, frame: Box<frame::Headers>) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();

        me.recv_headers(self.peer, self.send_buffer, frame)
    }

    pub fn recv_data(&mut self, frame: frame::Data) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        me.recv_data(self.peer, self.send_buffer, frame)
    }

    pub fn recv_reset(&mut self, frame: frame::Reset) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();

        me.recv_reset(self.send_buffer, frame)
    }

    /// Notify all streams that a connection-level error happened.
    pub fn handle_error(&mut self, err: proto::Error) -> StreamId {
        let mut me = self.inner.lock().unwrap();
        let result = me.handle_error(self.send_buffer, err);
        me.counts.wake_send_capacity();
        result
    }

    pub fn recv_go_away(&mut self, frame: &frame::GoAway) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        let result = me.recv_go_away(self.send_buffer, frame);
        me.counts.wake_send_capacity();
        result
    }

    pub fn last_processed_id(&self) -> StreamId {
        self.inner.lock().unwrap().actions.recv.last_processed_id()
    }

    pub fn recv_window_update(&mut self, frame: frame::WindowUpdate) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        me.recv_window_update(self.send_buffer, frame)
    }

    pub fn recv_push_promise(&mut self, frame: frame::PushPromise) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        me.recv_push_promise(self.send_buffer, frame)
    }

    pub fn recv_eof(&mut self, clear_pending_accept: bool) -> Result<(), ()> {
        let mut me = self.inner.lock().map_err(|_| ())?;
        let result = me.recv_eof(self.send_buffer, clear_pending_accept);
        me.counts.wake_send_capacity();
        result
    }

    pub fn send_reset(
        &mut self,
        id: StreamId,
        reason: Reason,
    ) -> Result<(), crate::h2_backend::proto::error::GoAway> {
        let mut me = self.inner.lock().unwrap();
        me.send_reset(self.send_buffer, id, reason)
    }

    pub fn send_go_away(&mut self, last_processed_id: StreamId) {
        let mut me = self.inner.lock().unwrap();
        me.actions.recv.go_away(last_processed_id);
        me.counts.wake_send_capacity();
    }
}

impl Inner {
    fn new(
        peer: peer::Dyn,
        config: Config,
    ) -> (
        Arc<Mutex<Self>>,
        Arc<AtomicBool>,
        Arc<AtomicBool>,
        Arc<AtomicBool>,
        Arc<AtomicBool>,
    ) {
        let refusal_pending = Arc::new(AtomicBool::new(false));
        let reset_pending = Arc::new(AtomicBool::new(false));
        // False until an armed poll has stored the driver waker.
        let buffer_idle = Arc::new(AtomicBool::new(false));
        let counts = Counts::new(peer, &config, buffer_idle.clone());
        let streams_open = counts.streams_open_flag();
        let inner = Arc::new(Mutex::new(Inner {
            counts,
            actions: Actions {
                recv: Recv::new(peer, &config, buffer_idle.clone()),
                send: Send::new(&config, buffer_idle.clone()),
                task: None,
                conn_error: None,
            },
            store: Store::new(),
            refs: 1,
            refusal_pending: refusal_pending.clone(),
            reset_pending: reset_pending.clone(),
        }));
        (
            inner,
            refusal_pending,
            reset_pending,
            streams_open,
            buffer_idle,
        )
    }

    fn recv_headers<B>(
        &mut self,
        peer: peer::Dyn,
        send_buffer: &SendBuffer<B>,
        frame: Box<frame::Headers>,
    ) -> Result<(), Error> {
        let id = frame.stream_id();

        // The GOAWAY process has begun. All streams with a greater ID than
        // specified as part of GOAWAY should be ignored.
        if id > self.actions.recv.max_stream_id() {
            tracing::trace!(
                "id ({:?}) > max_stream_id ({:?}), ignoring HEADERS",
                id,
                self.actions.recv.max_stream_id()
            );
            return Ok(());
        }

        // A response or trailer on the only stream is already in `sole`.
        // The first header on an empty connection opens a stream without
        // building the id-map entry. `find_entry` stays for every other miss.
        let key = if let Some(stream) = self.store.sole_hit(id) {
            stream.key()
        } else if peer.is_server() && self.store.has_no_ids() {
            match self
                .actions
                .recv
                .open(id, Open::Headers, &mut self.counts)?
            {
                Some(stream_id) => {
                    let send_window = self.actions.send.init_window_sz();
                    let recv_window = self.actions.recv.init_window_sz();
                    self.store
                        .insert_new(stream_id, send_window, recv_window)
                        .key()
                }
                None => {
                    self.refusal_pending.store(true, Ordering::Release);
                    return Ok(());
                }
            }
        } else {
            match self.store.find_entry(id) {
            Entry::Occupied(e) => e.key(),
            Entry::Vacant(e) => {
                // Client: it's possible to send a request, and then send
                // a RST_STREAM while the response HEADERS were in transit.
                //
                // Server: we can't reset a stream before having received
                // the request headers, so don't allow.
                if !peer.is_server() {
                    // This may be response headers for a stream we've already
                    // forgotten about...
                    if self.actions.may_have_forgotten_stream(peer, id) {
                        tracing::debug!(
                            "recv_headers for old stream={:?}, sending STREAM_CLOSED",
                            id,
                        );
                        return Err(Error::library_reset(id, Reason::STREAM_CLOSED));
                    }
                }

                match self
                    .actions
                    .recv
                    .open(id, Open::Headers, &mut self.counts)?
                {
                    Some(stream_id) => {
                        let send_window = self.actions.send.init_window_sz();
                        let recv_window = self.actions.recv.init_window_sz();
                        e.insert_new(stream_id, send_window, recv_window)
                    }
                    None => {
                        self.refusal_pending.store(true, Ordering::Release);
                        return Ok(());
                    }
                }
            }
            }
        };

        let mut stream = self.store.resolve(key);

        if stream.is_pending_open {
            proto_err!(conn: "recv_headers: received frame on idle stream {:?}", id);
            return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
        }

        if stream.state.is_local_error() {
            // Locally reset streams must ignore frames "for some time".
            // This is because the remote may have sent trailers before
            // receiving the RST_STREAM frame.
            tracing::trace!("recv_headers; ignoring trailers on {:?}", stream.id);
            return Ok(());
        }

        // Inlined `Counts::transition`. A closure would store the header
        // block by value and copy it again on the way into `recv_headers`.
        // The reset flag and the closed check share this slot.
        #[allow(
            unsafe_code,
            reason = "the reset flag is read from the slot the closed check reuses"
        )]
        let (slot, is_pending_reset) = {
            let slot = stream.stream_ptr();
            let is_pending_reset = unsafe { (*slot).is_pending_reset_expiration() };
            (slot, is_pending_reset)
        };
        let reset_pending = &self.reset_pending;
        let actions = &mut self.actions;
        let counts = &mut self.counts;
        tracing::trace!(
            "recv_headers; stream={:?}; state={:?}",
            stream.id,
            stream.state
        );

        let res = if stream.state.is_recv_headers() {
            match actions.recv.recv_headers(frame, &mut stream, counts) {
                Ok(()) => Ok(()),
                Err(RecvHeaderBlockError::Oversize(resp)) => {
                    if let Some(resp) = resp {
                        let mut send_buffer = send_buffer.inner.lock().unwrap();
                        let sent = actions.send.send_headers(
                            resp, &mut *send_buffer, &mut stream, counts, &mut actions.task);
                        debug_assert!(sent.is_ok(), "oversize response should not fail");

                        actions.send.schedule_implicit_reset(
                            &mut stream,
                            Reason::PROTOCOL_ERROR,
                            counts,
                            &mut actions.task);

                        actions
                            .recv
                            .enqueue_reset_expiration(&mut stream, counts, reset_pending);

                        Ok(())
                    } else {
                        Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR))
                    }
                },
                Err(RecvHeaderBlockError::State(err)) => Err(err),
            }
        } else if !frame.is_end_stream() {
            // Receiving trailers that don't set EOS is a "malformed"
            // message. Malformed messages are a stream error.
            proto_err!(stream: "recv_headers: trailers frame was not EOS; stream={:?}", stream.id);
            Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR))
        } else {
            actions.recv.recv_trailers(frame, &mut stream)
        };

        // The send buffer is only used to emit RST_STREAM. A delivered
        // header block does not touch it.
        let res = if !matches!(res, Err(Error::Reset(..))) {
            res
        } else {
            let mut locked = send_buffer.inner.lock().unwrap();
            actions.reset_on_recv_stream_err(
                &mut *locked,
                &mut stream,
                counts,
                res,
                reset_pending,
            )
        };
        counts.transition_after_at(stream, slot, is_pending_reset);
        res
    }

    fn recv_data<B>(
        &mut self,
        peer: peer::Dyn,
        send_buffer: &SendBuffer<B>,
        frame: frame::Data,
    ) -> Result<(), Error> {
        let id = frame.stream_id();

        let mut stream = if let Some(stream) = self.store.sole_hit(id) {
            stream
        } else {
            match self.store.find_mut(&id) {
            Some(stream) => stream,
            None => {
                // The GOAWAY process has begun. All streams with a greater ID
                // than specified as part of GOAWAY should be ignored.
                if id > self.actions.recv.max_stream_id() {
                    tracing::trace!(
                        "id ({:?}) > max_stream_id ({:?}), ignoring DATA",
                        id,
                        self.actions.recv.max_stream_id()
                    );

                    // We still need to account for connection-level flow control.
                    let sz = frame.flow_controlled_len();
                    assert!(sz <= super::MAX_WINDOW_SIZE as usize);
                    let sz = sz as WindowSize;
                    self.actions.recv.ignore_data(sz)?;

                    return Ok(());
                }

                if self.actions.may_have_forgotten_stream(peer, id) {
                    tracing::debug!("recv_data for old stream={:?}, sending STREAM_CLOSED", id,);

                    let sz = frame.flow_controlled_len();
                    // This should have been enforced at the codec::FramedRead layer, so
                    // this is just a sanity check.
                    assert!(sz <= super::MAX_WINDOW_SIZE as usize);
                    let sz = sz as WindowSize;
                    self.actions.recv.ignore_data(sz)?;

                    return Err(Error::library_reset(id, Reason::STREAM_CLOSED));
                }

                proto_err!(conn: "recv_data: stream not found; id={:?}", id);
                return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
            }
            }
        };

        let reset_pending = &self.reset_pending;
        let actions = &mut self.actions;
        // The reset flag and the closed check are the same slot. `transition`
        // looks it up twice around this body.
        let slot = stream.stream_ptr();

        self.counts.transition_at(stream, slot, |counts, stream| {
            let sz = frame.flow_controlled_len();
            let is_end_stream = frame.is_end_stream();
            let payload_len = frame.payload().len();
            let mut res = actions.recv.recv_data(frame, stream, slot);
            // A stream can receive at most one final DATA frame, so it cannot
            // be used to create unbounded framing overhead on that stream.
            if res.is_ok() && !is_end_stream {
                res = counts.record_data_frame(payload_len).map_err(|_| {
                    tracing::debug!("too many small DATA frames");
                    Error::library_go_away_data(Reason::ENHANCE_YOUR_CALM, "too_many_data_frames")
                });
            }

            // Any stream error after receiving a DATA frame means
            // we won't give the data to the user, and so they can't
            // release the capacity. We do it automatically.
            if let Err(Error::Reset(..)) = res {
                actions
                    .recv
                    .release_connection_capacity(sz as WindowSize, &mut None);
                let mut send_buffer = send_buffer.inner.lock().unwrap();
                actions.reset_on_recv_stream_err(
                    &mut *send_buffer,
                    stream,
                    counts,
                    res,
                    reset_pending,
                )
            } else {
                res
            }
        })
    }

    fn recv_reset<B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        frame: frame::Reset,
    ) -> Result<(), Error> {
        let id = frame.stream_id();

        if id.is_zero() {
            proto_err!(conn: "recv_reset: invalid stream ID 0");
            return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
        }

        // The GOAWAY process has begun. All streams with a greater ID than
        // specified as part of GOAWAY should be ignored.
        if id > self.actions.recv.max_stream_id() {
            tracing::trace!(
                "id ({:?}) > max_stream_id ({:?}), ignoring RST_STREAM",
                id,
                self.actions.recv.max_stream_id()
            );
            return Ok(());
        }

        let stream = match self.store.find_mut(&id) {
            Some(stream) => stream,
            None => {
                // TODO: Are there other error cases?
                self.actions
                    .ensure_not_idle(self.counts.peer(), id)
                    .map_err(Error::library_go_away)?;

                return Ok(());
            }
        };

        if stream.is_pending_open {
            proto_err!(conn: "recv_reset: received frame on idle stream {:?}", id);
            return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
        }

        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        let actions = &mut self.actions;

        self.counts.transition(stream, |counts, stream| {
            actions.recv.recv_reset(frame, stream, counts)?;
            actions.send.handle_error(send_buffer, stream, counts);
            assert!(stream.state.is_closed());
            Ok(())
        })
    }

    fn recv_window_update<B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        frame: frame::WindowUpdate,
    ) -> Result<(), Error> {
        let id = frame.stream_id();

        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        if id.is_zero() {
            self.actions
                .send
                .recv_connection_window_update(frame, &mut self.store, &mut self.counts)
                .map_err(Error::library_go_away)?;
        } else {
            // The remote may send window updates for streams that the local now
            // considers closed. It's ok...
            if let Some(mut stream) = self.store.find_mut(&id) {
                if stream.is_pending_open {
                    proto_err!(conn: "recv_window_update: received frame on idle stream {:?}", id);
                    return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
                }

                let res = self
                    .actions
                    .send
                    .recv_stream_window_update(
                        frame.size_increment(),
                        send_buffer,
                        &mut stream,
                        &mut self.counts,
                        &mut self.actions.task,
                    )
                    .map_err(|reason| Error::library_reset(id, reason));

                return self.actions.reset_on_recv_stream_err(
                    send_buffer,
                    &mut stream,
                    &mut self.counts,
                    res,
                    &self.reset_pending,
                );
            } else {
                self.actions
                    .ensure_not_idle(self.counts.peer(), id)
                    .map_err(Error::library_go_away)?;
            }
        }

        Ok(())
    }

    fn handle_error<B>(&mut self, send_buffer: &SendBuffer<B>, err: proto::Error) -> StreamId {
        let actions = &mut self.actions;
        let counts = &mut self.counts;
        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        let last_processed_id = actions.recv.last_processed_id();

        self.store.for_each(|stream| {
            counts.transition(stream, |counts, stream| {
                actions.recv.handle_error(&err, &mut *stream);
                actions.send.handle_error(send_buffer, stream, counts);
            })
        });

        actions.conn_error = Some(err);

        last_processed_id
    }

    fn recv_go_away<B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        frame: &frame::GoAway,
    ) -> Result<(), Error> {
        let actions = &mut self.actions;
        let counts = &mut self.counts;
        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        let last_stream_id = frame.last_stream_id();

        actions.send.recv_go_away(last_stream_id)?;

        let err = Error::remote_go_away(frame.debug_data().clone(), frame.reason());

        let peer = counts.peer();
        self.store.for_each(|stream| {
            if stream.id > last_stream_id && peer.is_local_init(stream.id) {
                counts.transition(stream, |counts, stream| {
                    actions.recv.handle_error(&err, &mut *stream);
                    actions.send.handle_error(send_buffer, stream, counts);
                })
            }
        });

        actions.conn_error = Some(err);

        Ok(())
    }

    fn recv_push_promise<B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        frame: frame::PushPromise,
    ) -> Result<(), Error> {
        let id = frame.stream_id();
        let promised_id = frame.promised_id();

        // First, ensure that the initiating stream is still in a valid state.
        let parent_key = match self.store.find_mut(&id) {
            Some(stream) => {
                // The GOAWAY process has begun. All streams with a greater ID
                // than specified as part of GOAWAY should be ignored.
                if id > self.actions.recv.max_stream_id() {
                    tracing::trace!(
                        "id ({:?}) > max_stream_id ({:?}), ignoring PUSH_PROMISE",
                        id,
                        self.actions.recv.max_stream_id()
                    );
                    return Ok(());
                }

                // The stream must be receive open
                if !stream.state.ensure_recv_open()? {
                    proto_err!(conn: "recv_push_promise: initiating stream is not opened");
                    return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
                }

                stream.key()
            }
            None => {
                proto_err!(conn: "recv_push_promise: initiating stream is in an invalid state");
                return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
            }
        };

        // TODO: Streams in the reserved states do not count towards the concurrency
        // limit. However, it seems like there should be a cap otherwise this
        // could grow in memory indefinitely.

        // Ensure that we can reserve streams
        self.actions.recv.ensure_can_reserve()?;

        // Next, open the stream.
        //
        // If `None` is returned, then the stream is being refused. There is no
        // further work to be done.
        if self
            .actions
            .recv
            .open(promised_id, Open::PushPromise, &mut self.counts)?
            .is_none()
        {
            self.refusal_pending.store(true, Ordering::Release);
            return Ok(());
        }

        // Try to handle the frame and create a corresponding key for the pushed stream
        // this requires a bit of indirection to make the borrow checker happy.
        let child_key: Option<store::Key> = {
            // Create state for the stream
            let stream = self.store.insert(promised_id, {
                Stream::new(
                    promised_id,
                    self.actions.send.init_window_sz(),
                    self.actions.recv.init_window_sz(),
                )
            });

            let reset_pending = &self.reset_pending;
            let actions = &mut self.actions;

            self.counts.transition(stream, |counts, stream| {
                let stream_valid = actions.recv.recv_push_promise(frame, stream);

                match stream_valid {
                    Ok(()) => Ok(Some(stream.key())),
                    _ => {
                        let mut send_buffer = send_buffer.inner.lock().unwrap();
                        actions
                            .reset_on_recv_stream_err(
                                &mut *send_buffer,
                                stream,
                                counts,
                                stream_valid,
                                reset_pending,
                            )
                            .map(|()| None)
                    }
                }
            })?
        };
        // If we're successful, push the headers and stream...
        if let Some(child) = child_key {
            let mut ppp = self.store[parent_key].pending_push_promises.take();
            ppp.push(&mut self.store.resolve(child));

            let parent = &mut self.store.resolve(parent_key);
            parent.pending_push_promises = ppp;
            parent.notify_push();
        };

        Ok(())
    }

    fn recv_eof<B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        clear_pending_accept: bool,
    ) -> Result<(), ()> {
        let actions = &mut self.actions;
        let counts = &mut self.counts;
        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        if actions.conn_error.is_none() {
            actions.conn_error = Some(
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "connection closed because of a broken pipe",
                )
                .into(),
            );
        }

        tracing::trace!("Streams::recv_eof");

        self.store.for_each(|stream| {
            counts.transition(stream, |counts, stream| {
                actions.recv.recv_eof(stream);

                // This handles resetting send state associated with the
                // stream
                actions.send.handle_error(send_buffer, stream, counts);
            })
        });

        actions.clear_queues(clear_pending_accept, &mut self.store, counts);
        Ok(())
    }

    fn buffer_pending<T, B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> io::Result<BufferStatus>
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        // Send WINDOW_UPDATE frames first. They do not touch the frame slab,
        // so an idle send side never takes that lock.
        //
        // TODO: It would probably be better to interleave updates w/ data
        // frames.
        let recv_status =
            self.actions
                .recv
                .buffer_pending(&mut self.store, &mut self.counts, dst)?;
        if recv_status == BufferStatus::CodecFull {
            return Ok(BufferStatus::CodecFull);
        }
        if self.actions.send.is_idle() {
            return Ok(if recv_status == BufferStatus::Idle {
                BufferStatus::Idle
            } else {
                BufferStatus::Complete
            });
        }

        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        // Send any other pending frames
        let send_status = self.actions.send.buffer_pending(
            send_buffer,
            &mut self.store,
            &mut self.counts,
            dst,
        )?;
        if send_status == BufferStatus::CodecFull {
            return Ok(BufferStatus::CodecFull);
        }
        if recv_status == BufferStatus::Idle && send_status == BufferStatus::Idle {
            Ok(BufferStatus::Idle)
        } else {
            Ok(BufferStatus::Complete)
        }
    }

    fn reclaim_written_frame<T, B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> bool
    where
        B: Buf,
    {
        // A fully written frame only clears the in-flight mark. The slab
        // lock is for the remainder that has to be queued again.
        let Some(frame) = dst.take_last_data_frame() else {
            return false;
        };
        if !frame.payload().has_remaining() {
            self.actions.send.reclaim_consumed(frame);
            return false;
        }
        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;
        self.actions
            .send
            .reclaim_partial(send_buffer, &mut self.store, frame)
    }

    fn send_reset<B>(
        &mut self,
        send_buffer: &SendBuffer<B>,
        id: StreamId,
        reason: Reason,
    ) -> Result<(), crate::h2_backend::proto::error::GoAway> {
        let key = match self.store.find_entry(id) {
            Entry::Occupied(e) => e.key(),
            Entry::Vacant(e) => {
                // Resetting a stream we don't know about? That could be OK...
                //
                // 1. As a server, we just received a request, but that request
                //    was bad, so we're resetting before even accepting it.
                //    This is totally fine.
                //
                // 2. The remote may have sent us a frame on new stream that
                //    it's *not* supposed to have done, and thus, we don't know
                //    the stream. In that case, sending a reset will "open" the
                //    stream in our store. Maybe that should be a connection
                //    error instead? At least for now, we need to update what
                //    our vision of the next stream is.
                if self.counts.peer().is_local_init(id) {
                    // We normally would open this stream, so update our
                    // next-send-id record.
                    self.actions.send.maybe_reset_next_stream_id(id);
                } else {
                    // We normally would recv this stream, so update our
                    // next-recv-id record.
                    self.actions.recv.maybe_reset_next_stream_id(id);
                }

                let stream = Stream::new(id, 0, 0);

                e.insert(stream)
            }
        };

        let stream = self.store.resolve(key);
        let mut send_buffer = send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;
        self.actions.send_reset(
            stream,
            reason,
            Initiator::Library,
            &mut self.counts,
            send_buffer,
            &self.reset_pending,
        )
    }
}

impl<B> Streams<B, client::Peer>
where
    B: Buf,
{
    pub fn poll_pending_open(
        &mut self,
        cx: &Context,
        pending: Option<&OpaqueStreamRef>,
    ) -> Poll<Result<(), crate::h2_backend::Error>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        me.actions.ensure_no_conn_error()?;
        me.actions.send.ensure_next_stream_id()?;

        if let Some(pending) = pending {
            let mut stream = me.store.resolve(pending.key);
            tracing::trace!("poll_pending_open; stream = {:?}", stream.is_pending_open);
            if stream.is_pending_open {
                stream.wait_send(cx);
                return Poll::Pending;
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<B, P> Streams<B, P>
where
    P: Peer,
{
    /// DATA on the steady path. Skips building a [`DynStreams`] just to pass
    /// the peer and the send buffer into the lock.
    pub(crate) fn recv_data(&mut self, frame: frame::Data) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        me.recv_data(P::r#dyn(), &self.send_buffer, frame)
    }

    /// HEADERS on the steady path. Same as [`Self::recv_data`].
    pub(crate) fn recv_headers(&mut self, frame: Box<frame::Headers>) -> Result<(), Error> {
        let mut me = self.inner.lock().unwrap();
        me.recv_headers(P::r#dyn(), &self.send_buffer, frame)
    }

    pub fn as_dyn(&self) -> DynStreams<'_, B> {
        let Self {
            inner,
            send_buffer,
            refusal_pending: _,
            reset_pending: _,
            streams_open: _,
            buffer_idle: _,
            _p,
        } = self;
        DynStreams {
            inner,
            send_buffer,
            peer: P::r#dyn(),
        }
    }

    /// This function is safe to call multiple times.
    ///
    /// A `Result` is returned to avoid panicking if the mutex is poisoned.
    pub fn recv_eof(&mut self, clear_pending_accept: bool) -> Result<(), ()> {
        self.as_dyn().recv_eof(clear_pending_accept)
    }

    pub(crate) fn max_send_streams(&self) -> usize {
        self.inner.lock().unwrap().counts.max_send_streams()
    }

    pub(crate) fn max_recv_streams(&self) -> usize {
        self.inner.lock().unwrap().counts.max_recv_streams()
    }

    #[cfg(any())]
    pub fn num_active_streams(&self) -> usize {
        let me = self.inner.lock().unwrap();
        me.store.num_active_streams()
    }

    pub fn has_streams(&self) -> bool {
        let me = self.inner.lock().unwrap();
        me.counts.has_streams()
    }

    #[inline]
    pub fn has_streams_or_other_references(&self) -> bool {
        // While a stream is counted this is true regardless of `refs`.
        // The flag is updated under the stream lock and loaded with Acquire,
        // so a same-task close is visible to the check after `poll`.
        // The lock stays out of line: a live connection takes this branch.
        if self.streams_open.load(Ordering::Acquire) {
            return true;
        }
        self.locked_has_streams_or_refs()
    }

    /// Idle connection: no stream flag is set, so confirm under the lock.
    #[cold]
    #[inline(never)]
    fn locked_has_streams_or_refs(&self) -> bool {
        let me = self.inner.lock().unwrap();
        me.counts.has_streams() || me.refs > 1
    }

    #[cfg(any())]
    pub fn num_wired_streams(&self) -> usize {
        let me = self.inner.lock().unwrap();
        me.store.num_wired_streams()
    }
}

// no derive because we don't need B and P to be Clone.
impl<B, P> Clone for Streams<B, P>
where
    P: Peer,
{
    fn clone(&self) -> Self {
        let mut inner = self.inner.lock().unwrap();
        inner.refs += 1;
        inner.counts.owners += 1;
        drop(inner);
        Streams {
            inner: self.inner.clone(),
            send_buffer: self.send_buffer.clone(),
            refusal_pending: self.refusal_pending.clone(),
            reset_pending: self.reset_pending.clone(),
            streams_open: self.streams_open.clone(),
            buffer_idle: self.buffer_idle.clone(),
            _p: ::std::marker::PhantomData,
        }
    }
}

impl<B, P> Drop for Streams<B, P>
where
    P: Peer,
{
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.refs -= 1;
            inner.counts.owners -= 1;
            if inner.counts.owners <= 1 {
                inner.counts.clear_open_if_idle();
            }
            if inner.refs == 1 {
                if let Some(task) = inner.actions.task.take() {
                    task.wake();
                }
            }
        }
    }
}

// ===== impl StreamRef =====

impl<B> StreamRef<B> {
    /// Queue DATA only if the configured stream buffer can hold it. Returning
    /// the original buffer lets the caller wait without copying or cloning it.
    pub(crate) fn try_send_data(
        &mut self,
        data: B,
        end_stream: bool,
    ) -> Result<Result<(), UserError>, B>
    where
        B: Buf,
    {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;
        let mut stream = me.store.resolve(self.opaque.key);
        let actions = &mut me.actions;
        let len = data.remaining();
        // The buffer check, the stream id, and an end-of-stream close share
        // one slot. The close used to look the stream up again.
        let slot = stream.stream_ptr();
        #[allow(
            unsafe_code,
            reason = "the buffer check and the end-of-stream close share this slot"
        )]
        let stream_id = unsafe {
            if len
                > actions
                    .send
                    .max_buffer_size()
                    .saturating_sub((*slot).buffered_send_data)
            {
                return Err(data);
            }
            (*slot).id
        };
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        // A non-empty frame that does not end the stream adds buffered
        // bytes, so the stream cannot be closed. The close check would
        // look the stream up again and then do nothing.
        if !end_stream && len > 0 {
            let result = {
                let frame = frame::Data::new(stream_id, data);
                actions.send.send_data(
                    frame,
                    &mut send_buffer,
                    &mut stream,
                    slot,
                    &mut me.counts,
                    &mut actions.task,
                )
            };
            if result.is_ok() {
                return Ok(Ok(()));
            }
            #[allow(
                unsafe_code,
                reason = "the reset flag is read from the slot the buffer check already resolved"
            )]
            let pending = unsafe { (*slot).is_pending_reset_expiration() };
            me.counts.transition_after_at(stream, slot, pending);
            return Ok(result);
        }
        Ok(me.counts.transition_at(stream, slot, |counts, stream| {
            let mut frame = frame::Data::new(stream_id, data);
            frame.set_end_stream(end_stream);
            actions
                .send
                .send_data(frame, &mut send_buffer, stream, slot, counts, &mut actions.task)
        }))
    }

    pub fn send_data(&mut self, data: B, end_stream: bool) -> Result<(), UserError>
    where
        B: Buf,
    {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);
        let actions = &mut me.actions;
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;
        // The id and the close check share the slot `transition` would look up twice.
        let slot = stream.stream_ptr();
        #[allow(
            unsafe_code,
            reason = "the frame id is read from the slot the close check reuses"
        )]
        let stream_id = unsafe { (*slot).id };

        me.counts.transition_at(stream, slot, |counts, stream| {
            // Create the data frame
            let mut frame = frame::Data::new(stream_id, data);
            frame.set_end_stream(end_stream);

            // Send the data frame
            actions
                .send
                .send_data(frame, send_buffer, stream, slot, counts, &mut actions.task)
        })
    }

    pub fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), UserError> {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);
        let actions = &mut me.actions;
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;
        // The id and the close share the slot `transition` would look up again.
        let slot = stream.stream_ptr();
        #[allow(
            unsafe_code,
            reason = "the trailer id is read from the slot the close reuses"
        )]
        let frame = frame::Headers::trailers_boxed(unsafe { (*slot).id }, trailers);

        me.counts.transition_at(stream, slot, |counts, stream| {
            actions.send.send_trailers(
                frame,
                send_buffer,
                stream,
                slot,
                counts,
                &mut actions.task,
            )
        })
    }

    pub fn send_reset(&mut self, reason: Reason) {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let stream = me.store.resolve(self.opaque.key);
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        match me.actions.send_reset(
            stream,
            reason,
            Initiator::User,
            &mut me.counts,
            send_buffer,
            &me.reset_pending,
        ) {
            Ok(()) => (),
            Err(crate::h2_backend::proto::error::GoAway { .. }) => {
                // this should never happen, because Initiator::User resets do
                // not count toward the local limit.
                // we could perhaps make this state impossible, if we made the
                // initiator argument a generic, and so this could return
                // Infallible instead of an impossible GoAway, but oh well.
                unreachable!("Initiator::User should not error sending reset");
            }
        }
    }

    pub fn send_informational_headers(&mut self, frame: frame::Headers) -> Result<(), UserError> {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let stream = me.store.resolve(self.opaque.key);
        let actions = &mut me.actions;
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        me.counts.transition(stream, |counts, stream| {
            // For informational responses (1xx), we need to send headers without
            // changing the stream state. This allows multiple informational responses
            // to be sent before the final response.

            // Validate that this is actually an informational response
            debug_assert!(
                frame.is_informational(),
                "Frame must be informational after conversion from informational response"
            );

            // Ensure the frame is not marked as end_stream for informational responses
            if frame.is_end_stream() {
                return Err(UserError::UnexpectedFrameType);
            }

            // Send the interim informational headers directly to the buffer without state changes
            // This bypasses the normal send_headers flow that would transition the stream state
            actions.send.send_interim_informational_headers(
                frame,
                send_buffer,
                stream,
                counts,
                &mut actions.task,
            )
        })
    }

    pub fn send_response(
        &mut self,
        mut response: Response<()>,
        end_of_stream: bool,
    ) -> Result<(), UserError> {
        // Clear before taking lock, incase extensions contain a StreamRef.
        response.extensions_mut().clear();
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);
        let actions = &mut me.actions;
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;
        // The id, the close, and the queued response share one slot.
        let slot = stream.stream_ptr();
        #[allow(
            unsafe_code,
            reason = "the response id is read from the slot the close and the frame share"
        )]
        let stream_id = unsafe { (*slot).id };

        me.counts.transition_at(stream, slot, |counts, stream| {
            let frame = server::Peer::convert_send_message(stream_id, response, end_of_stream);

            actions.send.send_headers_boxed(
                frame,
                send_buffer,
                stream,
                slot,
                counts,
                &mut actions.task,
            )
        })
    }

    /// Send the stock HTTP 200 without copying an `http::Response`.
    ///
    /// The parked response path only needs `:status` and the saved header
    /// block. Moving a response into this call copied that head twice.
    pub fn send_stock_response(&mut self, end_of_stream: bool) -> Result<(), UserError> {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);
        let actions = &mut me.actions;
        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;
        let slot = stream.stream_ptr();
        #[allow(
            unsafe_code,
            reason = "the response id is read from the slot the close and the frame share"
        )]
        let stream_id = unsafe { (*slot).id };
        let mut frame = frame::Headers::response_boxed(
            stream_id,
            http::StatusCode::OK,
            HeaderMap::new(),
        );
        frame.preset_stock_response();
        if end_of_stream {
            frame.set_end_stream();
        }

        me.counts.transition_at(stream, slot, |counts, stream| {
            actions.send.send_headers_boxed(
                frame,
                send_buffer,
                stream,
                slot,
                counts,
                &mut actions.task,
            )
        })
    }

    pub fn send_push_promise(
        &mut self,
        mut request: Request<()>,
    ) -> Result<StreamRef<B>, UserError> {
        // Clear before taking lock, incase extensions contain a StreamRef.
        request.extensions_mut().clear();
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut send_buffer = self.send_buffer.inner.lock().unwrap();
        let send_buffer = &mut *send_buffer;

        let actions = &mut me.actions;
        let promised_id = actions.send.reserve_local()?;

        let child_key = {
            let mut child_stream = me.store.insert(
                promised_id,
                Stream::new(
                    promised_id,
                    actions.send.init_window_sz(),
                    actions.recv.init_window_sz(),
                ),
            );
            child_stream.state.reserve_local()?;
            child_stream.is_pending_push = true;
            child_stream.key()
        };

        let pushed = {
            let mut stream = me.store.resolve(self.opaque.key);

            let frame = crate::h2_backend::server::Peer::convert_push_message(
                stream.id,
                promised_id,
                request,
            )?;

            actions
                .send
                .send_push_promise(frame, send_buffer, &mut stream, &mut actions.task)
        };

        if let Err(err) = pushed {
            let mut child_stream = me.store.resolve(child_key);
            child_stream.unlink();
            child_stream.remove();
            return Err(err);
        }

        me.refs += 1;
        let opaque =
            OpaqueStreamRef::new(self.opaque.inner.clone(), &mut me.store.resolve(child_key));

        Ok(StreamRef {
            opaque,
            send_buffer: self.send_buffer.clone(),
        })
    }

    /// Called by the server after the stream is accepted. Given that clients
    /// initialize streams by sending HEADERS, the request will always be
    /// available.
    ///
    /// # Panics
    ///
    /// This function panics if the request isn't present.
    /// The accept poll maps this head immediately.
    #[inline(always)]
    pub fn take_request(&self) -> Request<()> {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);
        me.actions.recv.take_request(&mut stream)
    }

    /// Called by a client to see if the current stream is pending open
    pub fn is_pending_open(&self) -> bool {
        let mut me = self.opaque.inner.lock().unwrap();
        me.store.resolve(self.opaque.key).is_pending_open
    }

    /// Request capacity to send data
    pub fn reserve_capacity(&mut self, capacity: WindowSize) {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);

        me.actions
            .send
            .reserve_capacity(capacity, &mut stream, &mut me.counts)
    }

    /// Returns the stream's current send capacity.
    pub fn capacity(&self) -> WindowSize {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);

        me.actions.send.capacity(&mut stream)
    }

    /// Request to be notified when the stream's capacity increases
    pub fn poll_capacity(&mut self, cx: &Context) -> Poll<Option<Result<WindowSize, UserError>>> {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);

        me.actions.send.poll_capacity(cx, &mut stream)
    }

    /// Request to be notified for if a `RST_STREAM` is received for this stream.
    pub(crate) fn poll_reset(
        &mut self,
        cx: &Context,
        mode: proto::PollReset,
    ) -> Poll<Result<Reason, crate::h2_backend::Error>> {
        let mut me = self.opaque.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.opaque.key);

        me.actions.send.poll_reset(cx, &mut stream, mode)
    }

    pub fn clone_to_opaque(&self) -> OpaqueStreamRef {
        self.opaque.clone()
    }

    pub fn stream_id(&self) -> StreamId {
        self.opaque.stream_id()
    }
}

impl<B> Clone for StreamRef<B> {
    fn clone(&self) -> Self {
        StreamRef {
            opaque: self.opaque.clone(),
            send_buffer: self.send_buffer.clone(),
        }
    }
}

// ===== impl OpaqueStreamRef =====

impl OpaqueStreamRef {
    fn new(inner: Arc<Mutex<Inner>>, stream: &mut store::Ptr) -> OpaqueStreamRef {
        stream.ref_inc();
        OpaqueStreamRef {
            inner,
            key: stream.key(),
        }
    }
    /// Called by a client to check for a received response.
    pub fn poll_response(&mut self, cx: &Context) -> Poll<Result<Response<()>, proto::Error>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);

        me.actions.recv.poll_response(cx, &mut stream)
    }

    /// Called by a client to check for informational responses (1xx status codes)
    pub fn poll_informational(
        &mut self,
        cx: &Context,
    ) -> Poll<Option<Result<Response<()>, proto::Error>>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);

        me.actions.recv.poll_informational(cx, &mut stream)
    }
    /// Called by a client to check for a pushed request.
    pub fn poll_pushed(
        &mut self,
        cx: &Context,
    ) -> Poll<Option<Result<(Request<()>, OpaqueStreamRef), proto::Error>>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);
        me.actions
            .recv
            .poll_pushed(cx, &mut stream)
            .map_ok(|(h, key)| {
                me.refs += 1;
                let opaque_ref =
                    OpaqueStreamRef::new(self.inner.clone(), &mut me.store.resolve(key));
                (h, opaque_ref)
            })
    }

    pub fn is_end_stream(&self) -> bool {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let stream = me.store.resolve(self.key);

        me.actions.recv.is_end_stream(&stream)
    }

    pub fn poll_data(&mut self, cx: &Context) -> Poll<Option<Result<Bytes, proto::Error>>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);

        me.actions
            .recv
            .poll_data(cx, &mut stream)
            .map(|result| match result {
                Some(Ok(data)) => {
                    if data.is_budgeted {
                        me.counts.release_data_frame(data.payload.len());
                    }
                    Some(Ok(data.payload))
                }
                Some(Err(err)) => Some(Err(err)),
                None => None,
            })
    }

    /// Take the next chunk and return its window under one stream lock.
    ///
    /// The split `poll_data` + `release_capacity` pair locks twice per chunk.
    /// Callers that release immediately use this instead.
    pub fn poll_data_released(
        &mut self,
        cx: &Context,
    ) -> Poll<Option<Result<Bytes, proto::Error>>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;
        let mut stream = me.store.resolve(self.key);
        match me.actions.recv.poll_data(cx, &mut stream) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(Some(Ok(data))) => {
                if data.is_budgeted {
                    me.counts.release_data_frame(data.payload.len());
                }
                let n = data.payload.len();
                if n > 0
                    && me
                        .actions
                        .recv
                        .release_capacity(n as WindowSize, &mut stream, &mut me.actions.task)
                        .is_err()
                {
                    return Poll::Ready(Some(Err(proto::Error::library_go_away(
                        Reason::INTERNAL_ERROR,
                    ))));
                }
                Poll::Ready(Some(Ok(data.payload)))
            }
        }
    }

    pub fn poll_trailers(&mut self, cx: &Context) -> Poll<Option<Result<HeaderMap, proto::Error>>> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);

        me.actions.recv.poll_trailers(cx, &mut stream)
    }

    pub(crate) fn available_recv_capacity(&self) -> isize {
        let me = self.inner.lock().unwrap();
        let me = &*me;

        let stream = &me.store[self.key];
        stream.recv_flow.available().into()
    }

    pub(crate) fn used_recv_capacity(&self) -> WindowSize {
        let me = self.inner.lock().unwrap();
        let me = &*me;

        let stream = &me.store[self.key];
        stream.in_flight_recv_data
    }

    /// Releases recv capacity back to the peer. This may result in sending
    /// WINDOW_UPDATE frames on both the stream and connection.
    pub fn release_capacity(&mut self, capacity: WindowSize) -> Result<(), UserError> {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);

        me.actions
            .recv
            .release_capacity(capacity, &mut stream, &mut me.actions.task)
    }

    /// Clear the receive queue and set the status to no longer receive data frames.
    pub(crate) fn clear_recv_buffer(&mut self) {
        let mut me = self.inner.lock().unwrap();
        let me = &mut *me;

        let mut stream = me.store.resolve(self.key);
        stream.is_recv = false;
        me.actions
            .recv
            .clear_recv_buffer(&mut stream, &mut me.actions.task, &mut me.counts);
    }

    pub fn stream_id(&self) -> StreamId {
        self.inner.lock().unwrap().store[self.key].id
    }
}

impl fmt::Debug for OpaqueStreamRef {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.inner.try_lock() {
            Ok(me) => {
                let stream = &me.store[self.key];
                fmt.debug_struct("OpaqueStreamRef")
                    .field("stream_id", &stream.id)
                    .field("ref_count", &stream.ref_count)
                    .finish()
            }
            Err(_) => fmt
                .debug_struct("OpaqueStreamRef")
                .field("inner", &"<Locked>")
                .finish(),
        }
    }
}

impl Clone for OpaqueStreamRef {
    fn clone(&self) -> Self {
        // Increment the ref count
        let mut inner = self.inner.lock().unwrap();
        inner.store.resolve(self.key).ref_inc();
        inner.refs += 1;

        OpaqueStreamRef {
            inner: self.inner.clone(),
            key: self.key,
        }
    }
}

impl Drop for OpaqueStreamRef {
    fn drop(&mut self) {
        drop_stream_ref(&self.inner, self.key);
    }
}

// TODO: Move back in fn above
fn drop_stream_ref(inner: &Mutex<Inner>, key: store::Key) {
    let mut me = match inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            if ::std::thread::panicking() {
                tracing::trace!("StreamRef::drop; mutex poisoned");
                return;
            } else {
                panic!("StreamRef::drop; mutex poisoned");
            }
        }
    };

    let me = &mut *me;
    me.refs -= 1;
    let mut stream = me.store.resolve(key);
    // The refcount, the early return, and the close check share this slot.
    let slot = stream.stream_ptr();

    // One slab lookup for the refcount and the early return. Each field
    // access otherwise re-checks the key.
    // The closed-and-unreferenced case wakes the connection. Read it with
    // the refcount so that path does not look the stream up again.
    #[allow(
        unsafe_code,
        reason = "the refcount and the close check share the slot just resolved"
    )]
    let wake_conn = {
        // SAFETY: `slot` is this stream. Updating the refcount does not remove it.
        let stream = unsafe { &mut *slot };
        stream.ref_dec();

        // Another handle still owns an open stream, or close cleanup already
        // unlinked it. Cancel, wake, and final release only run for the last
        // reference of a stream that is already closed.
        if stream.ref_count > 0 && (stream.protocol_unlinked || !stream.is_closed()) {
            return;
        }
        stream.ref_count == 0 && stream.is_closed()
    };

    tracing::trace!("drop_stream_ref; stream={:?}", stream);

    let actions = &mut me.actions;

    // If the stream is not referenced and it is already
    // closed (does not have to go through logic below
    // of canceling the stream), we should notify the task
    // (connection) so that it can close properly
    if wake_conn {
        if let Some(task) = actions.task.take() {
            task.wake();
        }
    }

    let reset_pending = &me.reset_pending;
    me.counts.transition_at(stream, slot, |counts, stream| {
        maybe_cancel(stream, actions, counts, reset_pending);

        if stream.ref_count == 0 {
            // Release any recv window back to connection, no one can access
            // it anymore.
            actions
                .recv
                .release_closed_capacity(stream, &mut actions.task, counts);

            // We won't be able to reach our push promises anymore
            let mut ppp = stream.pending_push_promises.take();
            while let Some(promise) = ppp.pop(stream.store_mut()) {
                counts.transition(promise, |counts, stream| {
                    maybe_cancel(stream, actions, counts, reset_pending);
                });
            }
        }
    });
}

fn maybe_cancel(
    stream: &mut store::Ptr,
    actions: &mut Actions,
    counts: &mut Counts,
    pending: &AtomicBool,
) {
    if stream.is_canceled_interest() {
        // Server is allowed to early respond without fully consuming the client input stream
        // But per the RFC, must send a RST_STREAM(NO_ERROR) in such cases. https://www.rfc-editor.org/rfc/rfc7540#section-8.1
        // Some other http2 implementation may interpret other error code as fatal if not respected (i.e: nginx https://trac.nginx.org/nginx/ticket/2376)
        let reason = if counts.peer().is_server()
            && stream.state.is_send_closed()
            && stream.state.is_recv_streaming()
        {
            Reason::NO_ERROR
        } else {
            Reason::CANCEL
        };

        actions
            .send
            .schedule_implicit_reset(stream, reason, counts, &mut actions.task);
        actions
            .recv
            .enqueue_reset_expiration(stream, counts, pending);
    }
}

// ===== impl SendBuffer =====

impl<B> SendBuffer<B> {
    fn new() -> Self {
        let inner = Mutex::new(Buffer::new());
        SendBuffer { inner }
    }

    pub fn is_empty(&self) -> bool {
        let buf = self.inner.lock().unwrap();
        buf.is_empty()
    }
}

// ===== impl Actions =====

impl Actions {
    fn send_reset<B>(
        &mut self,
        stream: store::Ptr,
        reason: Reason,
        initiator: Initiator,
        counts: &mut Counts,
        send_buffer: &mut Buffer<Frame<B>>,
        pending: &AtomicBool,
    ) -> Result<(), crate::h2_backend::proto::error::GoAway> {
        counts.transition(stream, |counts, stream| {
            if initiator.is_library() {
                if counts.can_inc_num_local_error_resets() {
                    counts.inc_num_local_error_resets();
                } else {
                    tracing::warn!(
                        "locally-reset streams reached limit ({:?})",
                        counts.max_local_error_resets().unwrap(),
                    );
                    return Err(crate::h2_backend::proto::error::GoAway {
                        reason: Reason::ENHANCE_YOUR_CALM,
                        debug_data: "too_many_internal_resets".into(),
                    });
                }
            }

            self.send.send_reset(
                reason,
                initiator,
                send_buffer,
                stream,
                counts,
                &mut self.task,
            );
            self.recv.enqueue_reset_expiration(stream, counts, pending);
            // if a RecvStream is parked, ensure it's notified
            stream.notify_recv();

            Ok(())
        })
    }

    fn reset_on_recv_stream_err<B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
        counts: &mut Counts,
        res: Result<(), Error>,
        pending: &AtomicBool,
    ) -> Result<(), Error> {
        if let Err(Error::Reset(stream_id, reason, initiator)) = res {
            debug_assert_eq!(stream_id, stream.id);

            if counts.can_inc_num_local_error_resets() {
                counts.inc_num_local_error_resets();

                // Reset the stream.
                self.send
                    .send_reset(reason, initiator, buffer, stream, counts, &mut self.task);
                self.recv.enqueue_reset_expiration(stream, counts, pending);
                // if a RecvStream is parked, ensure it's notified
                stream.notify_recv();
                Ok(())
            } else {
                tracing::warn!(
                    "reset_on_recv_stream_err; locally-reset streams reached limit ({:?})",
                    counts.max_local_error_resets().unwrap(),
                );
                Err(Error::library_go_away_data(
                    Reason::ENHANCE_YOUR_CALM,
                    "too_many_internal_resets",
                ))
            }
        } else {
            res
        }
    }

    fn ensure_not_idle(&mut self, peer: peer::Dyn, id: StreamId) -> Result<(), Reason> {
        if peer.is_local_init(id) {
            self.send.ensure_not_idle(id)
        } else {
            self.recv.ensure_not_idle(id)
        }
    }

    fn ensure_no_conn_error(&self) -> Result<(), proto::Error> {
        if let Some(ref err) = self.conn_error {
            Err(err.clone())
        } else {
            Ok(())
        }
    }

    /// Check if we possibly could have processed and since forgotten this stream.
    ///
    /// If we send a RST_STREAM for a stream, we will eventually "forget" about
    /// the stream to free up memory. It's possible that the remote peer had
    /// frames in-flight, and by the time we receive them, our own state is
    /// gone. We *could* tear everything down by sending a GOAWAY, but it
    /// is more likely to be latency/memory constraints that caused this,
    /// and not a bad actor. So be less catastrophic, the spec allows
    /// us to send another RST_STREAM of STREAM_CLOSED.
    fn may_have_forgotten_stream(&self, peer: peer::Dyn, id: StreamId) -> bool {
        if id.is_zero() {
            return false;
        }
        if peer.is_local_init(id) {
            self.send.may_have_created_stream(id)
        } else {
            self.recv.may_have_created_stream(id)
        }
    }

    fn clear_queues(&mut self, clear_pending_accept: bool, store: &mut Store, counts: &mut Counts) {
        self.recv.clear_queues(clear_pending_accept, store, counts);
        self.send.clear_queues(store, counts);
    }
}
