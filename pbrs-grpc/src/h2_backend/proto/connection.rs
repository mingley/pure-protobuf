use crate::h2_backend::codec::UserError;
use crate::h2_backend::frame::{Reason, StreamId};
use crate::h2_backend::{client, server};

use crate::h2_backend::frame::DEFAULT_INITIAL_WINDOW_SIZE;
use crate::h2_backend::proto::*;

use bytes::Bytes;
use futures_core::Stream;
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::io::AsyncRead;

/// An H2 connection
#[derive(Debug)]
pub(crate) struct Connection<T, P, B: Buf = Bytes>
where
    P: Peer,
{
    /// Read / write frame values
    codec: Codec<T, Prioritized<B>>,

    inner: ConnectionInner<P, B>,

    /// Identifies the task whose waker was registered with the streams.
    driver_waker: Option<Waker>,
}

// Extracted part of `Connection` which does not depend on `T`. Reduces the amount of duplicated
// method instantiations.
#[derive(Debug)]
struct ConnectionInner<P, B: Buf = Bytes>
where
    P: Peer,
{
    /// Tracks the connection level state transitions.
    state: State,

    /// An error to report back once complete.
    ///
    /// This exists separately from State in order to support
    /// graceful shutdown.
    error: Option<frame::GoAway>,

    /// Pending GOAWAY frames to write.
    go_away: GoAway,

    /// Ping/pong handler
    ping_pong: PingPong,

    /// Connection settings
    settings: Settings,

    /// Stream state handler
    streams: Streams<B, P>,

    /// A `tracing` span tracking the lifetime of the connection.
    span: tracing::Span,

    /// Client or server
    _phantom: PhantomData<P>,
}

struct DynConnection<'a, B: Buf = Bytes> {
    state: &'a mut State,

    go_away: &'a mut GoAway,

    streams: DynStreams<'a, B>,

    error: &'a mut Option<frame::GoAway>,

    ping_pong: &'a mut PingPong,
}

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub next_stream_id: StreamId,
    pub initial_max_send_streams: usize,
    pub max_send_buffer_size: usize,
    pub reset_stream_duration: Duration,
    pub reset_stream_max: usize,
    pub remote_reset_stream_max: usize,
    pub local_error_reset_streams_max: Option<usize>,
    pub settings: frame::Settings,
    pub data_frame_budget: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DataFrameBudget {
    Auto,
    Configured(usize),
}

impl DataFrameBudget {
    pub(crate) fn resolve(self, connection_window: Option<WindowSize>) -> usize {
        match self {
            Self::Configured(budget) => budget,
            Self::Auto => {
                let window = connection_window.unwrap_or(DEFAULT_INITIAL_WINDOW_SIZE);
                let budget = window as usize / 2;

                budget.max(DEFAULT_DATA_FRAME_BUDGET)
            }
        }
    }
}

#[derive(Debug)]
enum State {
    /// Currently open in a sane state
    Open,

    /// The codec must be flushed
    Closing(Reason, Initiator),

    /// In a closed state
    Closed(Reason, Initiator),
}

impl<T, P, B> Connection<T, P, B>
where
    T: AsyncRead + AsyncWrite + Unpin,
    P: Peer,
    B: Buf,
{
    pub fn new(codec: Codec<T, Prioritized<B>>, config: Config) -> Connection<T, P, B> {
        fn streams_config(config: &Config) -> streams::Config {
            streams::Config {
                initial_max_send_streams: config.initial_max_send_streams,
                local_max_buffer_size: config.max_send_buffer_size,
                local_next_stream_id: config.next_stream_id,
                local_push_enabled: config.settings.is_push_enabled().unwrap_or(true),
                extended_connect_protocol_enabled: config
                    .settings
                    .is_extended_connect_protocol_enabled()
                    .unwrap_or(false),
                local_reset_duration: config.reset_stream_duration,
                local_reset_max: config.reset_stream_max,
                remote_reset_max: config.remote_reset_stream_max,
                remote_init_window_sz: DEFAULT_INITIAL_WINDOW_SIZE,
                remote_max_initiated: config
                    .settings
                    .max_concurrent_streams()
                    .map(|max| max as usize),
                local_max_error_reset_streams: config.local_error_reset_streams_max,
                data_frame_budget: config.data_frame_budget,
            }
        }
        let streams = Streams::new(streams_config(&config));
        let span = tracing::debug_span!(parent: None, "Connection", peer = %P::NAME);
        span.follows_from(tracing::Span::current());
        Connection {
            codec,
            driver_waker: None,
            inner: ConnectionInner {
                state: State::Open,
                error: None,
                go_away: GoAway::new(),
                ping_pong: PingPong::new(),
                settings: Settings::new(config.settings),
                streams,
                span,
                _phantom: PhantomData,
            },
        }
    }

    /// connection flow control
    pub(crate) fn set_target_window_size(&mut self, size: WindowSize) {
        let _res = self.inner.streams.set_target_connection_window_size(size);
        // TODO: proper error handling
        debug_assert!(_res.is_ok());
    }

    /// Send a new SETTINGS frame with an updated initial window size.
    pub(crate) fn set_initial_window_size(&mut self, size: WindowSize) -> Result<(), UserError> {
        let mut settings = frame::Settings::default();
        settings.set_initial_window_size(Some(size));
        self.inner.settings.send_settings(settings)
    }

    /// Send a new SETTINGS frame with extended CONNECT protocol enabled.
    pub(crate) fn set_enable_connect_protocol(&mut self) -> Result<(), UserError> {
        let mut settings = frame::Settings::default();
        settings.set_enable_connect_protocol(Some(1));
        self.inner.settings.send_settings(settings)
    }

    /// Returns the maximum number of concurrent streams that may be initiated
    /// by this peer.
    pub(crate) fn max_send_streams(&self) -> usize {
        self.inner.streams.max_send_streams()
    }

    /// Returns the maximum number of concurrent streams that may be initiated
    /// by the remote peer.
    pub(crate) fn max_recv_streams(&self) -> usize {
        self.inner.streams.max_recv_streams()
    }

    #[cfg(any())]
    pub fn num_wired_streams(&self) -> usize {
        self.inner.streams.num_wired_streams()
    }

    /// Returns `Ready` when the connection is ready to receive a frame.
    ///
    /// Returns `Error` as this may raise errors that are caused by delayed
    /// processing of received frames.
    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Error>> {
        // Record the connection span as the parent without entering it.
        // Entering it holds a borrow across the mutable field uses below.
        let span = tracing::trace_span!(parent: &self.inner.span, "poll_ready");
        let _e = span.enter();
        // The order of these calls don't really matter too much.
        // A live connection with no ping handle has nothing to write.
        if !self.inner.ping_pong.is_send_idle() {
            ready!(self.inner.ping_pong.send_pending_pong(cx, &mut self.codec))?;
            ready!(self.inner.ping_pong.send_pending_ping(cx, &mut self.codec))?;
        }
        // Synced settings have nothing to ACK or send.
        if !self.inner.settings.is_send_idle() {
            ready!(
                self.inner
                    .settings
                    .poll_send(cx, &mut self.codec, &mut self.inner.streams)
            )?;
        }
        ready!(self.inner.streams.send_pending_refusal(cx, &mut self.codec))?;

        Poll::Ready(Ok(()))
    }

    /// Send any pending GOAWAY frames.
    ///
    /// This will return `Some(reason)` if the connection should be closed
    /// afterwards. If this is a graceful shutdown, this returns `None`.
    fn poll_go_away(&mut self, cx: &mut Context) -> Poll<Option<io::Result<Reason>>> {
        self.inner.go_away.send_pending_go_away(cx, &mut self.codec)
    }

    pub fn go_away_from_user(&mut self, e: Reason) {
        self.inner.as_dyn().go_away_from_user(e)
    }

    fn take_error(&mut self, ours: Reason, initiator: Initiator) -> Result<(), Error> {
        let (debug_data, theirs) = self
            .inner
            .error
            .take()
            .as_ref()
            .map_or((Bytes::new(), Reason::NO_ERROR), |frame| {
                (frame.debug_data().clone(), frame.reason())
            });

        match (ours, theirs) {
            (Reason::NO_ERROR, Reason::NO_ERROR) => Ok(()),
            (ours, Reason::NO_ERROR) => Err(Error::GoAway(Bytes::new(), ours, initiator)),
            // If both sides reported an error, give their
            // error back to th user. We assume our error
            // was a consequence of their error, and less
            // important.
            (_, theirs) => Err(Error::remote_go_away(debug_data, theirs)),
        }
    }

    /// Closes the connection by transitioning to a GOAWAY state
    /// iff there are no streams or references.
    ///
    /// Returns whether streams or other references were present after the
    /// check. The open path returns the value already loaded. The close path
    /// re-reads after `go_away_now`, matching a caller that observed the
    /// post-close state.
    #[inline]
    pub fn maybe_close_connection_if_no_streams(&mut self) -> bool {
        if !self.inner.streams.has_streams_or_other_references() {
            return self.close_now_for_idle();
        }
        true
    }

    /// No stream flag was set. GOAWAY, then read the flag again.
    #[cold]
    #[inline(never)]
    fn close_now_for_idle(&mut self) -> bool {
        self.inner.as_dyn().go_away_now(Reason::NO_ERROR);
        self.inner.streams.has_streams_or_other_references()
    }

    /// Checks if there are any streams
    pub fn has_streams(&self) -> bool {
        self.inner.streams.has_streams()
    }

    /// Checks if there are any streams or references left
    #[inline]
    pub fn has_streams_or_other_references(&self) -> bool {
        // If we poll() and realize that there are no streams or references
        // then we can close the connection by transitioning to GOAWAY
        self.inner.streams.has_streams_or_other_references()
    }

    pub(crate) fn take_user_pings(&mut self) -> Option<UserPings> {
        self.inner.ping_pong.take_user_pings()
    }

    /// Advances the internal state of the connection.
    pub fn poll(&mut self, cx: &mut Context) -> Poll<Result<(), Error>> {
        // `poll2` needs `&mut self`, so the connection span cannot stay
        // borrowed for an `enter` guard. The child records that parent
        // without cloning it.
        let span = tracing::trace_span!(parent: &self.inner.span, "poll");
        let _e = span.enter();
        // A connection can move from the handshake task to a driver task.
        // The idle shortcut is valid only for the task already registered;
        // peer SETTINGS do not tell us which task owns the connection.
        let same_task = self
            .driver_waker
            .as_ref()
            .is_some_and(|waker| waker.will_wake(cx.waker()));
        if !same_task {
            self.driver_waker = Some(cx.waker().clone());
        }

        loop {
            tracing::trace!(connection.state = ?self.inner.state);
            // TODO: probably clean up this glob of code
            match self.inner.state {
                // When open, continue to poll a frame
                State::Open => {
                    let result = match self.poll2(cx) {
                        Poll::Ready(result) => result,
                        // The connection is not ready to make progress
                        Poll::Pending => {
                            // Ensure all window updates have been sent.
                            //
                            // This will also handle flushing `self.codec`
                            ready!(self.inner.streams.poll_complete(
                                cx,
                                &mut self.codec,
                                same_task
                            ))?;

                            if (self.inner.error.is_some()
                                || self.inner.go_away.should_close_on_idle())
                                && !self.inner.streams.has_streams()
                            {
                                self.inner.as_dyn().go_away_now(Reason::NO_ERROR);
                                continue;
                            }

                            return Poll::Pending;
                        }
                    };

                    self.inner.as_dyn().handle_poll2_result(result)?
                }
                State::Closing(reason, initiator) => {
                    tracing::trace!("connection closing after flush");
                    // Flush/shutdown the codec
                    ready!(self.codec.shutdown(cx))?;

                    // Transition the state to error
                    self.inner.state = State::Closed(reason, initiator);
                }
                State::Closed(reason, initiator) => {
                    return Poll::Ready(self.take_error(reason, initiator));
                }
            }
        }
    }

    fn poll2(&mut self, cx: &mut Context) -> Poll<Result<(), Error>> {
        // This happens outside of the loop to prevent needing to do a clock
        // check and then comparison of the queue possibly multiple times a
        // second (and thus, the clock wouldn't have changed enough to matter).
        self.clear_expired_reset_streams();

        loop {
            // First, ensure that the `Connection` is able to receive a frame
            //
            // The order here matters:
            // - poll_go_away may buffer a graceful shutdown GOAWAY frame
            // - If it has, we've also added a PING to be sent in poll_ready
            if let Some(reason) = ready!(self.poll_go_away(cx)?) {
                if self.inner.go_away.should_close_now() {
                    if self.inner.go_away.is_user_initiated() {
                        // A user initiated abrupt shutdown shouldn't return
                        // the same error back to the user.
                        return Poll::Ready(Ok(()));
                    } else {
                        return Poll::Ready(Err(Error::library_go_away(reason)));
                    }
                }
                // Only NO_ERROR should be waiting for idle
                debug_assert_eq!(
                    reason,
                    Reason::NO_ERROR,
                    "graceful GOAWAY should be NO_ERROR"
                );
            }
            ready!(self.poll_ready(cx))?;

            // DATA and HEADERS do not touch GOAWAY, settings, or ping state.
            // Building that view copied five references on every frame.
            match ready!(Pin::new(&mut self.codec).poll_next(cx)?) {
                Some(crate::h2_backend::frame::Frame::Data(frame)) => {
                    self.inner.streams.recv_data(frame)?;
                }
                Some(crate::h2_backend::frame::Frame::Headers(frame)) => {
                    self.inner.streams.recv_headers(frame)?;
                }
                other => match self.inner.as_dyn().recv_frame(other)? {
                    ReceivedFrame::Settings(frame) => {
                        self.inner.settings.recv_settings(
                            frame,
                            &mut self.codec,
                            &mut self.inner.streams,
                        )?;
                    }
                    ReceivedFrame::Continue => (),
                    ReceivedFrame::Done => {
                        return Poll::Ready(Ok(()));
                    }
                },
            }
        }
    }

    fn clear_expired_reset_streams(&mut self) {
        self.inner.streams.clear_expired_reset_streams();
    }
}

impl<P, B> ConnectionInner<P, B>
where
    P: Peer,
    B: Buf,
{
    fn as_dyn(&mut self) -> DynConnection<'_, B> {
        let ConnectionInner {
            state,
            go_away,
            streams,
            error,
            ping_pong,
            ..
        } = self;
        let streams = streams.as_dyn();
        DynConnection {
            state,
            go_away,
            streams,
            error,
            ping_pong,
        }
    }
}

impl<B> DynConnection<'_, B>
where
    B: Buf,
{
    fn go_away(&mut self, id: StreamId, e: Reason) {
        let frame = frame::GoAway::new(id, e);
        self.streams.send_go_away(id);
        self.go_away.go_away(frame);
    }

    fn go_away_now(&mut self, e: Reason) {
        let last_processed_id = self.streams.last_processed_id();
        let frame = frame::GoAway::new(last_processed_id, e);
        self.go_away.go_away_now(frame);
    }

    fn go_away_now_data(&mut self, e: Reason, data: Bytes) {
        let last_processed_id = self.streams.last_processed_id();
        let frame = frame::GoAway::with_debug_data(last_processed_id, e, data);
        self.go_away.go_away_now(frame);
    }

    fn go_away_from_user(&mut self, e: Reason) {
        let last_processed_id = self.streams.last_processed_id();
        let frame = frame::GoAway::new(last_processed_id, e);
        self.go_away.go_away_from_user(frame);

        // Notify all streams of reason we're abruptly closing.
        self.streams.handle_error(Error::user_go_away(e));
    }

    fn handle_poll2_result(&mut self, result: Result<(), Error>) -> Result<(), Error> {
        match result {
            // The connection has shutdown normally
            Ok(()) => {
                *self.state = State::Closing(Reason::NO_ERROR, Initiator::Library);
                Ok(())
            }
            // Attempting to read a frame resulted in a connection level
            // error. This is handled by setting a GOAWAY frame followed by
            // terminating the connection.
            Err(Error::GoAway(debug_data, reason, initiator)) => {
                self.handle_go_away(reason, debug_data, initiator);
                Ok(())
            }
            // Attempting to read a frame resulted in a stream level error.
            // Locally detected stream errors are reported to the peer with
            // RST_STREAM. Remotely initiated resets have already been applied
            // by the streams state machine and must not be echoed back.
            Err(Error::Reset(id, reason, initiator)) => {
                if initiator == Initiator::Remote {
                    tracing::trace!(?id, ?reason, ?initiator, "stream reset");
                    return Ok(());
                }

                debug_assert_eq!(initiator, Initiator::Library);
                tracing::trace!(?id, ?reason, ?initiator, "stream error");
                match self.streams.send_reset(id, reason) {
                    Ok(()) => (),
                    Err(crate::h2_backend::proto::error::GoAway { debug_data, reason }) => {
                        self.handle_go_away(reason, debug_data, Initiator::Library);
                    }
                }
                Ok(())
            }
            // Attempting to read a frame resulted in an I/O error. All
            // active streams must be reset.
            //
            // TODO: Are I/O errors recoverable?
            Err(Error::Io(kind, inner)) => {
                tracing::debug!(error = ?kind, "Connection::poll; IO error");
                let e = Error::Io(kind, inner);

                // Reset all active streams
                self.streams.handle_error(e.clone());

                // Some client implementations drop the connections without notifying its peer
                // Attempting to read after the client dropped the connection results in UnexpectedEof
                // If as a server, we don't have anything more to send, just close the connection
                // without error
                //
                // See https://github.com/hyperium/hyper/issues/3427
                if self.streams.is_buffer_empty()
                    && matches!(kind, io::ErrorKind::UnexpectedEof)
                    && (self.streams.is_server()
                        || self.error.as_ref().map(|f| f.reason() == Reason::NO_ERROR)
                            == Some(true))
                {
                    *self.state = State::Closed(Reason::NO_ERROR, Initiator::Library);
                    return Ok(());
                }

                // Return the error
                Err(e)
            }
        }
    }

    fn handle_go_away(&mut self, reason: Reason, debug_data: Bytes, initiator: Initiator) {
        let e = Error::GoAway(debug_data.clone(), reason, initiator);
        tracing::debug!(error = ?e, "Connection::poll; connection error");

        // We may have already sent a GOAWAY for this error,
        // if so, don't send another, just flush and close up.
        if self
            .go_away
            .going_away()
            .map_or(false, |frame| frame.reason() == reason)
        {
            tracing::trace!("    -> already going away");
            *self.state = State::Closing(reason, initiator);
            return;
        }

        // Reset all active streams
        self.streams.handle_error(e);
        self.go_away_now_data(reason, debug_data);
    }

    fn recv_frame(&mut self, frame: Option<Frame>) -> Result<ReceivedFrame, Error> {
        use crate::h2_backend::frame::Frame::*;
        match frame {
            Some(Headers(frame)) => {
                self.streams.recv_headers(frame)?;
            }
            Some(Data(frame)) => {
                self.streams.recv_data(frame)?;
            }
            Some(Reset(frame)) => {
                self.streams.recv_reset(frame)?;
            }
            Some(PushPromise(frame)) => {
                self.streams.recv_push_promise(*frame)?;
            }
            Some(Settings(frame)) => {
                return Ok(ReceivedFrame::Settings(*frame));
            }
            Some(GoAway(frame)) => {
                // This should prevent starting new streams,
                // but should allow continuing to process current streams
                // until they are all EOS. Once they are, State should
                // transition to GoAway.
                self.streams.recv_go_away(&frame)?;
                *self.error = Some(frame);
            }
            Some(Ping(frame)) => {
                let status = self.ping_pong.recv_ping(frame);
                if status.is_shutdown() {
                    assert!(
                        self.go_away.is_going_away(),
                        "received unexpected shutdown ping"
                    );

                    let last_processed_id = self.streams.last_processed_id();
                    self.go_away(last_processed_id, Reason::NO_ERROR);
                }
            }
            Some(WindowUpdate(frame)) => {
                self.streams.recv_window_update(frame)?;
            }
            Some(Priority(_frame)) => {
                // TODO: handle
            }
            None => {
                self.streams.recv_eof(false).expect("mutex poisoned");
                return Ok(ReceivedFrame::Done);
            }
        }
        Ok(ReceivedFrame::Continue)
    }
}

enum ReceivedFrame {
    Settings(frame::Settings),
    Continue,
    Done,
}

impl<T, B> Connection<T, client::Peer, B>
where
    T: AsyncRead + AsyncWrite,
    B: Buf,
{
    pub(crate) fn streams(&self) -> &Streams<B, client::Peer> {
        &self.inner.streams
    }
}

impl<T, B> Connection<T, server::Peer, B>
where
    T: AsyncRead + AsyncWrite + Unpin,
    B: Buf,
{
    pub fn next_incoming(&mut self) -> Option<StreamRef<B>> {
        self.inner.streams.next_incoming()
    }

    // Graceful shutdown only makes sense for server peers.
    pub fn go_away_gracefully(&mut self) {
        if self.inner.go_away.is_going_away() {
            // No reason to start a new one.
            return;
        }

        // According to http://httpwg.org/specs/rfc7540.html#GOAWAY:
        //
        // > A server that is attempting to gracefully shut down a connection
        // > SHOULD send an initial GOAWAY frame with the last stream
        // > identifier set to 2^31-1 and a NO_ERROR code. This signals to the
        // > client that a shutdown is imminent and that initiating further
        // > requests is prohibited. After allowing time for any in-flight
        // > stream creation (at least one round-trip time), the server can
        // > send another GOAWAY frame with an updated last stream identifier.
        // > This ensures that a connection can be cleanly shut down without
        // > losing requests.
        self.inner.as_dyn().go_away(StreamId::MAX, Reason::NO_ERROR);

        // We take the advice of waiting 1 RTT literally, and wait
        // for a pong before proceeding.
        self.inner.ping_pong.ping_shutdown();
    }
}

impl<T, P, B> Drop for Connection<T, P, B>
where
    P: Peer,
    B: Buf,
{
    fn drop(&mut self) {
        // Ignore errors as this indicates that the mutex is poisoned.
        let _ = self.inner.streams.recv_eof(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_data_frame_budget_scales_with_connection_window() {
        assert_eq!(
            DataFrameBudget::Auto.resolve(None),
            DEFAULT_INITIAL_WINDOW_SIZE as usize / 2
        );
        assert_eq!(
            DataFrameBudget::Auto.resolve(Some(DEFAULT_INITIAL_WINDOW_SIZE)),
            DEFAULT_INITIAL_WINDOW_SIZE as usize / 2
        );
        assert_eq!(DataFrameBudget::Auto.resolve(Some(1024 * 1024)), 512 * 1024);
    }

    #[test]
    fn auto_data_frame_budget_has_minimum() {
        assert_eq!(
            DataFrameBudget::Auto.resolve(Some(1)),
            DEFAULT_DATA_FRAME_BUDGET
        );
        assert_eq!(
            DataFrameBudget::Auto.resolve(Some(MAX_WINDOW_SIZE)),
            MAX_WINDOW_SIZE as usize / 2
        );
    }

    #[test]
    fn configured_data_frame_budget_is_unchanged() {
        assert_eq!(
            DataFrameBudget::Configured(123).resolve(Some(MAX_WINDOW_SIZE)),
            123
        );
    }
}
