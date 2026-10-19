use super::*;
use crate::h2_backend::codec::UserError;
use crate::h2_backend::frame::{DEFAULT_INITIAL_WINDOW_SIZE, PushPromiseHeaderError, Reason};
use crate::h2_backend::proto;

use http::{HeaderMap, Request, Response};

use std::cmp::Ordering;
use std::io;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

#[derive(Debug)]
pub(super) struct Recv {
    /// Initial window size of remote initiated streams
    init_window_sz: WindowSize,

    /// Connection level flow control governing received data
    flow: FlowControl,

    /// Amount of connection window capacity currently used by outstanding streams.
    in_flight_data: WindowSize,

    /// The lowest stream ID that is still idle
    next_stream_id: Result<StreamId, StreamIdOverflow>,

    /// The stream ID of the last processed stream
    last_processed_id: StreamId,

    /// Any streams with a higher ID are ignored.
    ///
    /// This starts as MAX, but is lowered when a GOAWAY is received.
    ///
    /// > After sending a GOAWAY frame, the sender can discard frames for
    /// > streams initiated by the receiver with identifiers higher than
    /// > the identified last stream.
    max_stream_id: StreamId,

    /// Streams that have pending window updates
    pending_window_updates: store::Queue<stream::NextWindowUpdate>,

    /// New streams to be accepted
    pending_accept: store::Queue<stream::NextAccept>,

    /// Accept-encoding facts, one per `pending_accept` entry, in the same order.
    accept_facts: std::collections::VecDeque<crate::wire::headers::StockFacts>,

    /// Locally reset streams that should be reaped when they expire
    pending_reset_expired: store::Queue<stream::NextResetExpire>,

    /// How long locally reset streams should ignore received frames
    reset_duration: Duration,

    /// Holds frames that are waiting to be read
    buffer: Buffer<Event>,

    /// Refused StreamId, this represents a frame that must be sent out.
    refused: Option<StreamId>,

    /// If push promises are allowed to be received.
    is_push_enabled: bool,

    /// If extended connect protocol is enabled.
    is_extended_connect_protocol_enabled: bool,

    /// Cleared when a window update must be written.
    buffer_idle: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Debug)]
pub(super) enum Event {
    /// Boxed so a queued DATA event is not copied at request size.
    Headers(Box<peer::PollMessage>),
    /// Stock POST. Authority and path are the process cache; only the scheme varies.
    ///
    /// Built in `take_request` so the 224-byte request is not queued.
    StockRequest {
        https: bool,
    },
    /// HTTP 200 with the stock response header block.
    ///
    /// The map stays boxed. The response value is built when it is polled.
    StockResponse(Box<HeaderMap>),
    Data(DataEvent),
    Trailers(Box<HeaderMap>),
    InformationalHeaders(Box<peer::PollMessage>),
}

#[derive(Debug)]
pub(super) struct DataEvent {
    pub(super) payload: Bytes,
    pub(super) is_budgeted: bool,
}

#[derive(Debug)]
pub(super) enum RecvHeaderBlockError<T> {
    Oversize(T),
    State(Error),
}

#[derive(Debug)]
pub(crate) enum Open {
    PushPromise,
    Headers,
}

/// Queue a received header block without copying a stock message.
fn queued_header(
    peer: &peer::Dyn,
    pseudo: &frame::Pseudo,
    fields: HeaderMap,
    stream_id: StreamId,
    known_stock: bool,
) -> Result<Event, Error> {
    if peer.is_server() {
        if let Some(https) = crate::h2_backend::header_pool::take_stock_https()
            .or_else(|| crate::h2_backend::server::stock_post_https(pseudo, &fields))
        {
            // Empty stock map. `Request::new` already has the same one.
            std::mem::forget(fields);
            return Ok(Event::StockRequest { https });
        }
        let message = peer.convert_poll_message(pseudo, fields, stream_id)?;
        return Ok(Event::Headers(Box::new(message)));
    }
    if known_stock && pseudo.status == Some(http::StatusCode::OK) {
        return Ok(Event::StockResponse(
            crate::h2_backend::header_pool::box_header_map(fields),
        ));
    }
    let fields = match crate::h2_backend::client::stock_response_map(pseudo, fields) {
        Ok(fields) => {
            return Ok(Event::StockResponse(
                crate::h2_backend::header_pool::box_header_map(fields),
            ));
        }
        Err(fields) => fields,
    };
    let message = peer.convert_poll_message(pseudo, fields, stream_id)?;
    Ok(Event::Headers(Box::new(message)))
}

impl Recv {
    pub fn new(
        peer: peer::Dyn,
        config: &Config,
        buffer_idle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        let next_stream_id = if peer.is_server() { 1 } else { 2 };

        let mut flow = FlowControl::new();

        // connections always have the default window size, regardless of
        // settings
        flow.inc_window(DEFAULT_INITIAL_WINDOW_SIZE)
            .expect("invalid initial remote window size");
        flow.assign_capacity(DEFAULT_INITIAL_WINDOW_SIZE).unwrap();

        Recv {
            init_window_sz: DEFAULT_INITIAL_WINDOW_SIZE,
            flow,
            in_flight_data: 0 as WindowSize,
            next_stream_id: Ok(next_stream_id.into()),
            pending_window_updates: store::Queue::new(),
            last_processed_id: StreamId::ZERO,
            max_stream_id: StreamId::MAX,
            pending_accept: store::Queue::new(),
            accept_facts: std::collections::VecDeque::with_capacity(4),
            pending_reset_expired: store::Queue::new(),
            reset_duration: config.local_reset_duration,
            buffer: Buffer::new(),
            refused: None,
            is_push_enabled: config.local_push_enabled,
            is_extended_connect_protocol_enabled: config.extended_connect_protocol_enabled,
            buffer_idle,
        }
    }

    /// Returns the initial receive window size
    pub fn init_window_sz(&self) -> WindowSize {
        self.init_window_sz
    }

    /// Returns the ID of the last processed stream
    pub fn last_processed_id(&self) -> StreamId {
        self.last_processed_id
    }

    /// Update state reflecting a new, remotely opened stream
    ///
    /// Returns the stream state if successful. `None` if refused
    pub fn open(
        &mut self,
        id: StreamId,
        mode: Open,
        counts: &mut Counts,
    ) -> Result<Option<StreamId>, Error> {
        assert!(self.refused.is_none());

        counts.peer().ensure_can_open(id, mode)?;

        let next_id = self.next_stream_id()?;
        if id < next_id {
            proto_err!(conn: "id ({:?}) < next_id ({:?})", id, next_id);
            return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
        }

        self.next_stream_id = id.next_id();

        if !counts.can_inc_num_recv_streams() {
            self.refused = Some(id);
            return Ok(None);
        }

        Ok(Some(id))
    }

    /// Transition the stream state based on receiving headers
    ///
    /// The caller ensures that the frame represents headers and not trailers.
    #[allow(
        unsafe_code,
        reason = "header receive updates the stream through the slot resolved on entry"
    )]
    pub fn recv_headers(
        &mut self,
        frame: Box<frame::Headers>,
        stream: &mut store::Ptr,
        counts: &mut Counts,
    ) -> Result<(), RecvHeaderBlockError<Option<frame::Headers>>> {
        tracing::trace!("opening stream; init_window={}", self.init_window_sz);
        // One slot for the open transition, the counted flag, and the recv queue.
        let sp = stream.stream_ptr();
        // SAFETY: `sp` is this stream for the whole receive. Queuing the
        // header does not remove the slab slot.
        let is_initial = unsafe { (*sp).state.recv_open(&frame)? };

        // Informational responses do not transition a remotely reserved stream
        // out of `ReservedRemote`. As a result, `recv_open` reports each of them
        // as initial. Only account for the stream once.
        if is_initial && unsafe { !(*sp).is_counted } {
            // TODO: be smarter about this logic
            if frame.stream_id() > self.last_processed_id {
                self.last_processed_id = frame.stream_id();
            }

            // Increment the number of concurrent streams
            counts.inc_num_recv_streams(sp);
        }

        if frame.is_over_size() {
            // A frame is over size if the decoded header block was bigger than
            // SETTINGS_MAX_HEADER_LIST_SIZE.
            //
            // > A server that receives a larger header block than it is willing
            // > to handle can send an HTTP 431 (Request Header Fields Too
            // > Large) status code [RFC6585]. A client can discard responses
            // > that it cannot process.
            //
            // So, if peer is a server, we'll send a 431. In either case,
            // an error is recorded, which will send a REFUSED_STREAM,
            // since we don't want any of the data frames either.
            tracing::debug!(
                "stream error REQUEST_HEADER_FIELDS_TOO_LARGE -- \
                 recv_headers: frame is over size; stream={:?}",
                stream.id
            );
            return if counts.peer().is_server() && is_initial {
                let mut res = frame::Headers::new(
                    stream.id,
                    frame::Pseudo::response(::http::StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE),
                    HeaderMap::new(),
                );
                res.set_end_stream();
                Err(RecvHeaderBlockError::Oversize(Some(res)))
            } else {
                Err(RecvHeaderBlockError::Oversize(None))
            };
        }

        // A cache-hit POST published `:scheme` and left the frame's pseudos
        // and map empty. Moving those Nones out and dropping them is the
        // rest of this function. That frame has no content-length and is
        // not over size, so those checks stay on the general path.
        if counts.peer().is_server()
            && frame.fields().is_empty()
            && frame.fields().capacity() == 0
        {
            if let Some(https) = crate::h2_backend::header_pool::take_stock_https() {
                let cached_facts = frame.stock_facts();
                frame::Headers::forget_empty_box(frame);
                // SAFETY: `sp` is still the stream resolved above.
                unsafe {
                    (*sp)
                        .pending_recv
                        .push_back(&mut self.buffer, Event::StockRequest { https });
                    (*sp).notify_recv();
                }
                self.accept_facts.push_back(cached_facts.unwrap_or_else(|| {
                    crate::wire::headers::stock_facts(&HeaderMap::new())
                }));
                self.pending_accept.push_resolved(stream, sp);
                return Ok(());
            }
        }

        // A stock response only stores `:status`, which is `Copy`. The map
        // is the piece the client keeps.
        if !counts.peer().is_server()
            && frame.is_stock_response()
            && frame.pseudo().status == Some(http::StatusCode::OK)
        {
            let fields = if frame.fields().is_empty() && frame.fields().capacity() == 0 {
                frame::Headers::forget_empty_box(frame);
                crate::h2_backend::header_pool::take_received_response()
                    .map(|(map, _)| map)
                    .unwrap_or_else(HeaderMap::new)
            } else {
                frame::Headers::take_fields_box(frame)
            };
            // SAFETY: `sp` is still the stream resolved above.
            unsafe {
                (*sp).pending_recv.push_back(
                    &mut self.buffer,
                    Event::StockResponse(crate::h2_backend::header_pool::box_header_map(fields)),
                );
                (*sp).notify_recv();
            }
            return Ok(());
        }

        self.recv_headers_general(frame, stream, counts)
    }

    /// A header block that is not a stock gRPC request or OK response.
    #[cold]
    #[inline(never)]
    fn recv_headers_general(
        &mut self,
        frame: Box<frame::Headers>,
        stream: &mut store::Ptr,
        counts: &mut Counts,
    ) -> Result<(), RecvHeaderBlockError<Option<frame::Headers>>> {
        if !stream.content_length.is_head() {
            use super::stream::ContentLength;

            if let Some(content_length) = frame.content_length() {
                let content_length = match frame::parse_u64(content_length.as_bytes()) {
                    Ok(v) => v,
                    Err(_) => {
                        proto_err!(stream: "could not parse content-length; stream={:?}", stream.id);
                        return Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR).into());
                    }
                };

                stream.content_length = ContentLength::Remaining(content_length);
                // END_STREAM on headers frame with non-zero content-length is malformed.
                // https://datatracker.ietf.org/doc/html/rfc9113#section-8.1.1
                if frame.is_end_stream()
                    && content_length > 0
                    && frame
                        .pseudo()
                        .status
                        .map_or(true, |status| status != 204 && status != 304)
                {
                    proto_err!(stream: "recv_headers with END_STREAM: content-length is not zero; stream={:?};", stream.id);
                    return Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR).into());
                }
            }
        }

        let stream_id = frame.stream_id();
        let cached_facts = frame.stock_facts();
        let known_stock = frame.is_known_stock();
        let (pseudo, fields) = frame::Headers::into_parts_box(frame);

        if pseudo.protocol.is_some()
            && counts.peer().is_server()
            && !self.is_extended_connect_protocol_enabled
        {
            proto_err!(stream: "cannot use :protocol if extended connect protocol is disabled; stream={:?}", stream.id);
            return Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR).into());
        }

        if pseudo.status.is_some() && counts.peer().is_server() {
            proto_err!(stream: "cannot use :status header for requests; stream={:?}", stream.id);
            return Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR).into());
        }

        if !pseudo.is_informational() {
            let accept_facts = counts.peer().is_server().then(|| {
                cached_facts.unwrap_or_else(|| crate::wire::headers::stock_facts(&fields))
            });
            let event = queued_header(&counts.peer(), &pseudo, fields, stream_id, known_stock)?;

            // The queue link and the recv wakeup share one slab lookup.
            {
                let stream = &mut **stream;
                stream.pending_recv.push_back(&mut self.buffer, event);
                stream.notify_recv();
            }

            // Only servers can receive a headers frame that initiates the stream.
            // This is verified in `Streams` before calling this function.
            if counts.peer().is_server() {
                // Correctness: never push a stream to `pending_accept` without having the
                // corresponding headers frame pushed to `stream.pending_recv`.
                if let Some(facts) = accept_facts {
                    self.accept_facts.push_back(facts);
                }
                self.pending_accept.push(stream);
            }
        } else {
            // This is an informational response (1xx status code)
            // Convert to response and store it for polling
            let message = counts
                .peer()
                .convert_poll_message(&pseudo, fields, stream_id)?;

            tracing::trace!("Received informational response: stream_id={:?}", stream_id);

            // Push the informational response onto the stream's recv buffer
            // with a special event type so it can be polled separately
            {
                let stream = &mut **stream;
                stream.pending_recv.push_back(
                    &mut self.buffer,
                    Event::InformationalHeaders(Box::new(message)),
                );
                stream.notify_recv();
            }
        }

        Ok(())
    }

    /// Called by the server to get the request
    ///
    /// # Panics
    ///
    /// Panics if `stream.pending_recv` has no `Event::Headers` queued.
    ///
    #[inline(always)]
    pub fn take_request(&mut self, stream: &mut store::Ptr) -> Request<()> {
        use super::peer::PollMessage::*;

        match stream.pending_recv.pop_front(&mut self.buffer) {
            Some(Event::StockRequest { https }) => {
                crate::h2_backend::server::cached_simple_post(https)
            }
            Some(Event::Headers(message)) => match *message {
                Server(request) => request,
                Client(_) => unreachable!("server stream queue must start with Headers"),
            },
            _ => unreachable!("server stream queue must start with Headers"),
        }
    }

    /// Called by the client to get pushed response
    pub fn poll_pushed(
        &mut self,
        cx: &Context,
        stream: &mut store::Ptr,
    ) -> Poll<Option<Result<(Request<()>, store::Key), proto::Error>>> {
        use super::peer::PollMessage::*;

        let mut ppp = stream.pending_push_promises.take();
        let pushed = ppp.pop(stream.store_mut()).map(|mut pushed| {
            match pushed.pending_recv.pop_front(&mut self.buffer) {
                Some(Event::StockRequest { https }) => (
                    crate::h2_backend::server::cached_simple_post(https),
                    pushed.key(),
                ),
                Some(Event::Headers(message)) => match *message {
                    Server(headers) => (headers, pushed.key()),
                    Client(_) => panic!("Headers not set on pushed stream"),
                },
                // When frames are pushed into the queue, it is verified that
                // the first frame is a HEADERS frame.
                _ => panic!("Headers not set on pushed stream"),
            }
        });
        stream.pending_push_promises = ppp;
        if let Some(p) = pushed {
            Poll::Ready(Some(Ok(p)))
        } else {
            let is_open = stream.state.ensure_recv_open()?;

            if is_open {
                stream.push_task = Some(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(None)
            }
        }
    }

    /// Called by the client to get the response
    pub fn poll_response(
        &mut self,
        cx: &Context,
        stream: &mut store::Ptr,
    ) -> Poll<Result<Response<()>, proto::Error>> {
        use super::peer::PollMessage::*;

        // Skip over any interim informational headers to find the main response
        loop {
            match stream.pending_recv.pop_front(&mut self.buffer) {
                Some(Event::StockResponse(fields)) => {
                    return Poll::Ready(Ok(crate::h2_backend::client::response_from_stock(
                        crate::h2_backend::header_pool::unbox_header_map(fields),
                    )));
                }
                Some(Event::Headers(message)) => match *message {
                    Client(response) => return Poll::Ready(Ok(response)),
                    Server(_) => panic!("poll_response called after response returned"),
                },
                Some(Event::InformationalHeaders(_)) => {
                    tracing::trace!(
                        "Skipping informational response in poll_response - should be consumed via poll_informational; stream_id={:?}",
                        stream.id
                    );
                    continue;
                }
                Some(_) => panic!("poll_response called after response returned"),
                None => {
                    if !stream.state.ensure_recv_open()? {
                        proto_err!(stream: "poll_response: stream={:?} is not opened;",  stream.id);
                        return Poll::Ready(Err(Error::library_reset(
                            stream.id,
                            Reason::PROTOCOL_ERROR,
                        )));
                    }

                    stream.recv_task = Some(cx.waker().clone());
                    return Poll::Pending;
                }
            }
        }
    }

    /// Called by the client to get informational responses (1xx status codes)
    pub fn poll_informational(
        &mut self,
        cx: &Context,
        stream: &mut store::Ptr,
    ) -> Poll<Option<Result<Response<()>, proto::Error>>> {
        use super::peer::PollMessage::*;

        // Try to pop the front event and check if it's an informational response
        // If it's not, we put it back
        if let Some(event) = stream.pending_recv.pop_front(&mut self.buffer) {
            match event {
                Event::StockResponse(fields) => {
                    stream
                        .pending_recv
                        .push_front(&mut self.buffer, Event::StockResponse(fields));
                    return Poll::Ready(None);
                }
                Event::StockRequest { https } => {
                    stream
                        .pending_recv
                        .push_front(&mut self.buffer, Event::StockRequest { https });
                    return Poll::Ready(None);
                }
                Event::Headers(message) => {
                    let is_final = matches!(message.as_ref(), Client(_));
                    // Final response stays at the front for poll_response.
                    // Any other headers frame is put back and is not 1xx.
                    stream
                        .pending_recv
                        .push_front(&mut self.buffer, Event::Headers(message));
                    if is_final {
                        return Poll::Ready(None);
                    }
                }
                Event::InformationalHeaders(message) => match *message {
                    Client(response) => {
                        // Found an informational response, return it
                        return Poll::Ready(Some(Ok(response)));
                    }
                    other => {
                        stream.pending_recv.push_front(
                            &mut self.buffer,
                            Event::InformationalHeaders(Box::new(other)),
                        );
                    }
                },
                other => {
                    // Not an informational response, put it back at the front
                    stream.pending_recv.push_front(&mut self.buffer, other);
                }
            }
        }

        // No informational response available at the front
        if stream.state.ensure_recv_open()? {
            // Request to get notified once more frames arrive
            stream.recv_task = Some(cx.waker().clone());
            Poll::Pending
        } else {
            // No more frames will be received
            Poll::Ready(None)
        }
    }

    /// Transition the stream based on receiving trailers
    pub fn recv_trailers(
        &mut self,
        frame: Box<frame::Headers>,
        stream: &mut store::Ptr,
    ) -> Result<(), Error> {
        // Transition the state
        stream.state.recv_close()?;

        if stream.ensure_content_length_zero().is_err() {
            proto_err!(stream: "recv_trailers: content-length is not zero; stream={:?};",  stream.id);
            return Err(Error::library_reset(stream.id, Reason::PROTOCOL_ERROR));
        }

        let trailers = if frame.is_stock_trailers() {
            if frame.fields().is_empty() && frame.fields().capacity() == 0 {
                frame::Headers::forget_empty_box(frame);
                crate::h2_backend::header_pool::take_received_ok_trailer(HeaderMap::new)
            } else {
                frame::Headers::take_fields_box(frame)
            }
        } else {
            frame::Headers::into_parts_box(frame).1
        };

        // The queue link and the recv wakeup share one slab lookup.
        {
            let stream = &mut **stream;
            stream
                .pending_recv
                .push_back(&mut self.buffer, Event::Trailers(Box::new(trailers)));
            stream.notify_recv();
        }

        Ok(())
    }

    /// Releases capacity of the connection
    pub fn release_connection_capacity(&mut self, capacity: WindowSize, task: &mut Option<Waker>) {
        tracing::trace!(
            "release_connection_capacity; size={}, connection in_flight_data={}",
            capacity,
            self.in_flight_data,
        );

        // Decrement in-flight data
        self.in_flight_data -= capacity;

        // Assign capacity to connection
        // TODO: proper error handling
        let _res = self.flow.assign_capacity(capacity);
        debug_assert!(_res.is_ok());

        if self.flow.unclaimed_capacity().is_some() {
            super::clear_buffer_idle(&self.buffer_idle);
            if let Some(task) = task.take() {
                task.wake();
            }
        }
    }

    /// Releases capacity back to the connection & stream
    pub fn release_capacity(
        &mut self,
        capacity: WindowSize,
        stream: &mut store::Ptr,
        task: &mut Option<Waker>,
    ) -> Result<(), UserError> {
        tracing::trace!("release_capacity; size={}", capacity);

        // The too-big check and the stream-window update share one slab
        // lookup. Doing the update after the connection release looked the
        // stream up again.
        let queue_update = {
            let stream = &mut **stream;
            if capacity > stream.in_flight_recv_data {
                return Err(UserError::ReleaseCapacityTooBig);
            }
            stream.in_flight_recv_data -= capacity;
            // TODO: proper error handling
            let _res = stream.recv_flow.assign_capacity(capacity);
            debug_assert!(_res.is_ok());
            stream.recv_flow.unclaimed_capacity().is_some()
        };

        self.release_connection_capacity(capacity, task);

        if queue_update {
            // Queue the stream for sending the WINDOW_UPDATE frame.
            super::clear_buffer_idle(&self.buffer_idle);
            self.pending_window_updates.push(stream);

            if let Some(task) = task.take() {
                task.wake();
            }
        }

        Ok(())
    }

    /// Release any unclaimed capacity for a closed stream.
    pub fn release_closed_capacity(
        &mut self,
        stream: &mut store::Ptr,
        task: &mut Option<Waker>,
        counts: &mut Counts,
    ) {
        debug_assert_eq!(stream.ref_count, 0);

        if stream.in_flight_recv_data != 0 {
            tracing::trace!(
                "auto-release closed stream ({:?}) capacity: {:?}",
                stream.id,
                stream.in_flight_recv_data,
            );

            self.release_connection_capacity(stream.in_flight_recv_data, task);
            stream.in_flight_recv_data = 0;
        }

        self.clear_recv_buffer(stream, task, counts);
    }

    /// Set the "target" connection window size.
    ///
    /// By default, all new connections start with 64kb of window size. As
    /// streams used and release capacity, we will send WINDOW_UPDATEs for the
    /// connection to bring it back up to the initial "target".
    ///
    /// Setting a target means that we will try to tell the peer about
    /// WINDOW_UPDATEs so the peer knows it has about `target` window to use
    /// for the whole connection.
    ///
    /// The `task` is an optional parked task for the `Connection` that might
    /// be blocked on needing more window capacity.
    pub fn set_target_connection_window(
        &mut self,
        target: WindowSize,
        task: &mut Option<Waker>,
    ) -> Result<(), Reason> {
        tracing::trace!(
            "set_target_connection_window; target={}; available={}, reserved={}",
            target,
            self.flow.available(),
            self.in_flight_data,
        );

        // The current target connection window is our `available` plus any
        // in-flight data reserved by streams.
        //
        // Update the flow controller with the difference between the new
        // target and the current target.
        let current = self
            .flow
            .available()
            .add(self.in_flight_data)?
            .checked_size();
        if target > current {
            self.flow.assign_capacity(target - current)?;
        } else {
            self.flow.claim_capacity(current - target)?;
        }

        // If changing the target capacity means we gained a bunch of capacity,
        // enough that we went over the update threshold, then schedule sending
        // a connection WINDOW_UPDATE.
        if self.flow.unclaimed_capacity().is_some() {
            super::clear_buffer_idle(&self.buffer_idle);
            if let Some(task) = task.take() {
                task.wake();
            }
        }
        Ok(())
    }

    pub(crate) fn apply_local_settings(
        &mut self,
        settings: &frame::Settings,
        store: &mut Store,
    ) -> Result<(), proto::Error> {
        if let Some(val) = settings.is_extended_connect_protocol_enabled() {
            self.is_extended_connect_protocol_enabled = val;
        }

        if let Some(target) = settings.initial_window_size() {
            let old_sz = self.init_window_sz;
            self.init_window_sz = target;

            tracing::trace!("update_initial_window_size; new={}; old={}", target, old_sz,);

            // Per RFC 7540 §6.9.2:
            //
            // In addition to changing the flow-control window for streams that are
            // not yet active, a SETTINGS frame can alter the initial flow-control
            // window size for streams with active flow-control windows (that is,
            // streams in the "open" or "half-closed (remote)" state). When the
            // value of SETTINGS_INITIAL_WINDOW_SIZE changes, a receiver MUST adjust
            // the size of all stream flow-control windows that it maintains by the
            // difference between the new value and the old value.
            //
            // A change to `SETTINGS_INITIAL_WINDOW_SIZE` can cause the available
            // space in a flow-control window to become negative. A sender MUST
            // track the negative flow-control window and MUST NOT send new
            // flow-controlled frames until it receives WINDOW_UPDATE frames that
            // cause the flow-control window to become positive.

            match target.cmp(&old_sz) {
                Ordering::Less => {
                    // We must decrease the (local) window on every open stream.
                    let dec = old_sz - target;
                    tracing::trace!("decrementing all windows; dec={}", dec);

                    store.try_for_each(|mut stream| {
                        stream
                            .recv_flow
                            .dec_recv_window(dec)
                            .map_err(proto::Error::library_go_away)?;
                        Ok::<_, proto::Error>(())
                    })?;
                }
                Ordering::Greater => {
                    // We must increase the (local) window on every open stream.
                    let inc = target - old_sz;
                    tracing::trace!("incrementing all windows; inc={}", inc);
                    store.try_for_each(|mut stream| {
                        // XXX: Shouldn't the peer have already noticed our
                        // overflow and sent us a GOAWAY?
                        stream
                            .recv_flow
                            .inc_window(inc)
                            .map_err(proto::Error::library_go_away)?;
                        stream
                            .recv_flow
                            .assign_capacity(inc)
                            .map_err(proto::Error::library_go_away)?;
                        Ok::<_, proto::Error>(())
                    })?;
                }
                Ordering::Equal => (),
            }
        }

        Ok(())
    }

    pub fn is_end_stream(&self, stream: &store::Ptr) -> bool {
        if !stream.state.is_recv_end_stream() {
            return false;
        }

        stream.pending_recv.is_empty()
    }

    #[inline(never)]
    #[allow(
        unsafe_code,
        reason = "DATA receive updates the stream through the slot the caller already resolved"
    )]
    pub fn recv_data(
        &mut self,
        frame: frame::Data,
        stream: &mut store::Ptr,
        slot: *mut Stream,
    ) -> Result<(), Error> {
        // One length for the window and the padding check. Padding is the
        // gap between the flow-controlled size and the payload the user sees.
        let payload_len = frame.payload().len();
        let flow_len = frame.flow_controlled_len();

        // This should have been enforced at the codec::FramedRead layer, so
        // this is just a sanity check.
        if flow_len > MAX_WINDOW_SIZE as usize {
            data_frame_exceeds_window();
        }

        let sz = flow_len as WindowSize;

        // The caller already resolved this slot. `consume_connection_window`
        // only touches the connection flow, so the pointer stays valid.
        // SAFETY: `slot` is the stream for this receive and is not removed here.
        unsafe {
            let stream = &mut *slot;
            let is_ignoring_frame = stream.state.is_local_error();

            if !is_ignoring_frame && !stream.state.is_recv_streaming() {
                return Err(unexpected_data(stream.id));
            }

            if is_ignoring_frame {
                let id = stream.id;
                return self.ignore_reset_data(sz, id);
            }

            // Ensure that there is enough capacity on the connection before acting
            // on the stream.
            self.consume_connection_window(sz)?;

            if stream.recv_flow.window_size() < sz {
                // http://httpwg.org/specs/rfc7540.html#WINDOW_UPDATE
                // > A receiver MAY respond with a stream error (Section 5.4.2) or
                // > connection error (Section 5.4.1) of type FLOW_CONTROL_ERROR if
                // > it is unable to accept a frame.
                //
                // So, for violating the **stream** window, we can send either a
                // stream or connection error. We've opted to send a stream
                // error.
                return Err(Error::library_reset(stream.id, Reason::FLOW_CONTROL_ERROR));
            }

            // use payload len, padding doesn't count for content-length
            if stream.dec_content_length(payload_len).is_err() {
                return Err(content_length_overflow(stream.id, payload_len));
            }

            if frame.is_end_stream() {
                if stream.ensure_content_length_zero().is_err() {
                    return Err(content_length_underflow(stream.id, payload_len));
                }

                if stream.state.recv_close().is_err() {
                    return Err(recv_close_failed(stream.id));
                }
            }

            // Received a frame, but no one cared about it. fix issue#648
            if !stream.is_recv {
                let id = stream.id;
                return self.release_unwatched_data(sz, id);
            }

            // Update stream level flow control
            stream
                .recv_flow
                .send_data(sz)
                .map_err(proto::Error::library_go_away)?;

            // Track the data as in-flight
            stream.in_flight_recv_data += sz;
        }

        // Auto-release padding overhead (pad_len field + padding bytes),
        // since the user only sees the data payload via `payload()`.
        let padding = (flow_len - payload_len) as WindowSize;
        if padding > 0 {
            self.release_padding(padding, stream);
        }

        // An empty DATA frame without END_STREAM has no effect on the HTTP
        // message. Padding has already been accounted for and released above,
        // so there is no event to pass to the user.
        if payload_len == 0 && !frame.is_end_stream() {
            return Ok(());
        }

        let is_budgeted = !frame.is_end_stream();
        let event = Event::Data(DataEvent {
            payload: frame.into_payload(),
            is_budgeted,
        });

        // SAFETY: `slot` is still the stream resolved by the caller.
        unsafe {
            (*slot).pending_recv.push_back(&mut self.buffer, event);
            (*slot).notify_recv();
        }

        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn ignore_reset_data(&mut self, sz: WindowSize, id: StreamId) -> Result<(), Error> {
        tracing::trace!(
            "recv_data; frame ignored on locally reset {:?} for some time",
            id,
        );
        self.ignore_data(sz)
    }

    #[cold]
    #[inline(never)]
    fn release_unwatched_data(&mut self, sz: WindowSize, id: StreamId) -> Result<(), Error> {
        tracing::trace!(
            "recv_data; frame ignored on stream release {:?} for some time",
            id,
        );
        self.release_connection_capacity(sz, &mut None);
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn release_padding(&mut self, padding: WindowSize, stream: &mut store::Ptr) {
        tracing::trace!(
            "recv_data; auto-releasing padding of {:?} for {:?}",
            padding,
            stream.id,
        );
        let _res = self.release_capacity(padding, stream, &mut None);
        // cannot fail, we JUST added more in_flight data above.
        debug_assert!(_res.is_ok());
    }

    pub fn ignore_data(&mut self, sz: WindowSize) -> Result<(), Error> {
        // Ensure that there is enough capacity on the connection...
        self.consume_connection_window(sz)?;

        // Since we are ignoring this frame,
        // we aren't returning the frame to the user. That means they
        // have no way to release the capacity back to the connection. So
        // we have to release it automatically.
        //
        // This call doesn't send a WINDOW_UPDATE immediately, just marks
        // the capacity as available to be reclaimed. When the available
        // capacity meets a threshold, a WINDOW_UPDATE is then sent.
        self.release_connection_capacity(sz, &mut None);
        Ok(())
    }

    pub fn consume_connection_window(&mut self, sz: WindowSize) -> Result<(), Error> {
        if self.flow.window_size() < sz {
            tracing::debug!(
                "connection error FLOW_CONTROL_ERROR -- window_size ({:?}) < sz ({:?});",
                self.flow.window_size(),
                sz,
            );
            return Err(Error::library_go_away(Reason::FLOW_CONTROL_ERROR));
        }

        // Update connection level flow control
        self.flow.send_data(sz).map_err(Error::library_go_away)?;

        // Track the data as in-flight
        self.in_flight_data += sz;
        Ok(())
    }

    pub fn recv_push_promise(
        &mut self,
        frame: frame::PushPromise,
        stream: &mut store::Ptr,
    ) -> Result<(), Error> {
        stream.state.reserve_remote()?;
        if frame.is_over_size() {
            // A frame is over size if the decoded header block was bigger than
            // SETTINGS_MAX_HEADER_LIST_SIZE.
            //
            // > A server that receives a larger header block than it is willing
            // > to handle can send an HTTP 431 (Request Header Fields Too
            // > Large) status code [RFC6585]. A client can discard responses
            // > that it cannot process.
            //
            // So, if peer is a server, we'll send a 431. In either case,
            // an error is recorded, which will send a PROTOCOL_ERROR,
            // since we don't want any of the data frames either.
            tracing::debug!(
                "stream error PROTOCOL_ERROR -- recv_push_promise: \
                 headers frame is over size; promised_id={:?};",
                frame.promised_id(),
            );
            return Err(Error::library_reset(
                frame.promised_id(),
                Reason::PROTOCOL_ERROR,
            ));
        }

        let promised_id = frame.promised_id();
        let (pseudo, fields) = frame.into_parts();
        let req =
            crate::h2_backend::server::Peer::convert_poll_message(&pseudo, fields, promised_id)?;

        if let Err(e) = frame::PushPromise::validate_request(&req) {
            use PushPromiseHeaderError::*;
            match e {
                NotSafeAndCacheable => proto_err!(
                    stream:
                    "recv_push_promise: method {} is not safe and cacheable; promised_id={:?}",
                    req.method(),
                    promised_id,
                ),
                InvalidContentLength(e) => proto_err!(
                    stream:
                    "recv_push_promise; promised request has invalid content-length {:?}; promised_id={:?}",
                    e,
                    promised_id,
                ),
            }
            return Err(Error::library_reset(promised_id, Reason::PROTOCOL_ERROR));
        }

        use super::peer::PollMessage::*;
        stream
            .pending_recv
            .push_back(&mut self.buffer, Event::Headers(Box::new(Server(req))));
        stream.notify_recv();
        stream.notify_push();
        Ok(())
    }

    /// Ensures that `id` is not in the `Idle` state.
    pub fn ensure_not_idle(&self, id: StreamId) -> Result<(), Reason> {
        if let Ok(next) = self.next_stream_id {
            if id >= next {
                tracing::debug!(
                    "stream ID implicitly closed, PROTOCOL_ERROR; stream={:?}",
                    id
                );
                return Err(Reason::PROTOCOL_ERROR);
            }
        }
        // if next_stream_id is overflowed, that's ok.

        Ok(())
    }

    /// Handle remote sending an explicit RST_STREAM.
    pub fn recv_reset(
        &mut self,
        frame: frame::Reset,
        stream: &mut Stream,
        counts: &mut Counts,
    ) -> Result<(), Error> {
        // Reseting a stream that the user hasn't accepted is possible,
        // but should be done with care. These streams will continue
        // to take up memory in the accept queue, but will no longer be
        // counted as "concurrent" streams.
        //
        // So, we have a separate limit for these.
        //
        // See https://github.com/hyperium/hyper/issues/2877
        if stream.is_pending_accept {
            if counts.can_inc_num_remote_reset_streams() {
                counts.inc_num_remote_reset_streams();
            } else {
                tracing::warn!(
                    "recv_reset; remotely-reset pending-accept streams reached limit ({:?})",
                    counts.max_remote_reset_streams(),
                );
                return Err(Error::library_go_away_data(
                    Reason::ENHANCE_YOUR_CALM,
                    "too_many_resets",
                ));
            }
        }

        // Notify the stream
        stream.state.recv_reset(frame, stream.is_pending_send);

        stream.notify_send();
        stream.notify_recv();
        stream.notify_push();

        Ok(())
    }

    /// Handle a connection-level error
    pub fn handle_error(&mut self, err: &proto::Error, stream: &mut Stream) {
        // Receive an error
        stream.state.handle_error(err);

        // If a receiver is waiting, notify it
        stream.notify_send();
        stream.notify_recv();
        stream.notify_push();
    }

    pub fn go_away(&mut self, last_processed_id: StreamId) {
        assert!(self.max_stream_id >= last_processed_id);
        self.max_stream_id = last_processed_id;
    }

    pub fn recv_eof(&mut self, stream: &mut Stream) {
        stream.state.recv_eof();
        stream.notify_send();
        stream.notify_recv();
        stream.notify_push();
    }

    pub(super) fn clear_recv_buffer(
        &mut self,
        stream: &mut Stream,
        task: &mut Option<Waker>,
        counts: &mut Counts,
    ) {
        let mut to_release: WindowSize = 0;
        while let Some(event) = stream.pending_recv.pop_front(&mut self.buffer) {
            if let Event::Data(data) = &event {
                if data.is_budgeted {
                    counts.release_data_frame(data.payload.len());
                }
                to_release = to_release
                    .saturating_add(data.payload.len() as WindowSize)
                    .min(stream.in_flight_recv_data);
            }
        }
        // Release flow control capacity. Cases:
        // * User read data but hasn't released: buf=0, in_flight>0 -> release 0
        // * User released without reading: buf>0, in_flight=0 -> release 0
        // * Normal drop without reading: buf=in_flight -> full release
        if to_release > 0 {
            stream.in_flight_recv_data -= to_release;
            self.release_connection_capacity(to_release, task);
        }
    }

    /// Get the max ID of streams we can receive.
    ///
    /// This gets lowered if we send a GOAWAY frame.
    pub fn max_stream_id(&self) -> StreamId {
        self.max_stream_id
    }

    pub fn next_stream_id(&self) -> Result<StreamId, Error> {
        if let Ok(id) = self.next_stream_id {
            Ok(id)
        } else {
            Err(Error::library_go_away(Reason::PROTOCOL_ERROR))
        }
    }

    pub fn may_have_created_stream(&self, id: StreamId) -> bool {
        if let Ok(next_id) = self.next_stream_id {
            // Peer::is_local_init should have been called beforehand
            debug_assert_eq!(id.is_server_initiated(), next_id.is_server_initiated(),);
            id < next_id
        } else {
            true
        }
    }

    pub(super) fn maybe_reset_next_stream_id(&mut self, id: StreamId) {
        if let Ok(next_id) = self.next_stream_id {
            // !Peer::is_local_init should have been called beforehand
            debug_assert_eq!(id.is_server_initiated(), next_id.is_server_initiated());
            if id >= next_id {
                self.next_stream_id = id.next_id();
            }
        }
    }

    /// Returns true if the remote peer can reserve a stream with the given ID.
    pub fn ensure_can_reserve(&self) -> Result<(), Error> {
        if !self.is_push_enabled {
            proto_err!(conn: "recv_push_promise: push is disabled");
            return Err(Error::library_go_away(Reason::PROTOCOL_ERROR));
        }

        Ok(())
    }

    /// Add a locally reset stream to queue to be eventually reaped.
    ///
    /// `pending` is set when a stream is actually queued so the connection
    /// can skip the reap lock while the queue is empty.
    pub fn enqueue_reset_expiration(
        &mut self,
        stream: &mut store::Ptr,
        counts: &mut Counts,
        pending: &std::sync::atomic::AtomicBool,
    ) {
        if !stream.state.is_local_error() || stream.is_pending_reset_expiration() {
            return;
        }

        if counts.can_inc_num_reset_streams() {
            counts.inc_num_reset_streams();
            tracing::trace!("enqueue_reset_expiration; added {:?}", stream.id);
            self.pending_reset_expired.push(stream);
            pending.store(true, std::sync::atomic::Ordering::Release);
        } else {
            tracing::trace!(
                "enqueue_reset_expiration; dropped {:?}, over max_concurrent_reset_streams",
                stream.id
            );
        }
    }

    pub(super) fn reset_queue_is_empty(&self) -> bool {
        self.pending_reset_expired.is_empty()
    }

    /// Send any pending refusals.
    pub fn send_pending_refusal<T, B>(
        &mut self,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> io::Result<BufferStatus>
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        if let Some(stream_id) = self.refused {
            if !dst.has_send_capacity() {
                return Ok(BufferStatus::CodecFull);
            }

            // Create the RST_STREAM frame
            let frame = frame::Reset::new(stream_id, Reason::REFUSED_STREAM);

            // Buffer the frame
            dst.buffer(frame.into()).expect("invalid RST_STREAM frame");
        }

        self.refused = None;

        Ok(BufferStatus::Complete)
    }

    pub(super) fn has_pending_refusal(&self) -> bool {
        self.refused.is_some()
    }

    pub fn clear_expired_reset_streams(&mut self, store: &mut Store, counts: &mut Counts) {
        if !self.pending_reset_expired.is_empty() {
            let now = Instant::now();
            let reset_duration = self.reset_duration;
            while let Some(stream) = self.pending_reset_expired.pop_if(store, |stream| {
                let reset_at = stream.reset_at.expect("reset_at must be set if in queue");
                // rust-lang/rust#86470 tracks a bug in the standard library where `Instant`
                // subtraction can panic (because, on some platforms, `Instant` isn't actually
                // monotonic). We use a saturating operation to avoid this panic here.
                now.saturating_duration_since(reset_at) > reset_duration
            }) {
                counts.transition_after(stream, true);
            }
        }
    }

    pub fn clear_queues(
        &mut self,
        clear_pending_accept: bool,
        store: &mut Store,
        counts: &mut Counts,
    ) {
        self.clear_stream_window_update_queue(store, counts);
        self.clear_all_reset_streams(store, counts);

        if clear_pending_accept {
            self.clear_all_pending_accept(store, counts);
        }
    }

    fn clear_stream_window_update_queue(&mut self, store: &mut Store, counts: &mut Counts) {
        while let Some(stream) = self.pending_window_updates.pop(store) {
            counts.transition(stream, |_, stream| {
                tracing::trace!("clear_stream_window_update_queue; stream={:?}", stream.id);
            })
        }
    }

    /// Called on EOF
    fn clear_all_reset_streams(&mut self, store: &mut Store, counts: &mut Counts) {
        while let Some(stream) = self.pending_reset_expired.pop(store) {
            counts.transition_after(stream, true);
        }
    }

    fn clear_all_pending_accept(&mut self, store: &mut Store, counts: &mut Counts) {
        while let Some(stream) = self.pending_accept.pop(store) {
            counts.transition_after(stream, false);
        }
    }

    #[inline]
    pub fn buffer_pending<T, B>(
        &mut self,
        store: &mut Store,
        counts: &mut Counts,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> io::Result<BufferStatus>
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        // Most polls have nothing to advertise. Skip both update walks.
        if self.flow.unclaimed_capacity().is_none() && self.pending_window_updates.is_empty() {
            return Ok(BufferStatus::Idle);
        }
        // Send any pending connection level window updates
        if self.send_connection_window_update(dst)? == BufferStatus::CodecFull {
            return Ok(BufferStatus::CodecFull);
        }

        // Send any pending stream level window updates
        if self.send_stream_window_updates(store, counts, dst)? == BufferStatus::CodecFull {
            return Ok(BufferStatus::CodecFull);
        }

        Ok(BufferStatus::Complete)
    }

    /// Send connection level window update
    fn send_connection_window_update<T, B>(
        &mut self,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> io::Result<BufferStatus>
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        if let Some(incr) = self.flow.unclaimed_capacity() {
            let frame = frame::WindowUpdate::new(StreamId::zero(), incr);

            // Ensure the codec has capacity
            if !dst.has_send_capacity() {
                return Ok(BufferStatus::CodecFull);
            }

            // Buffer the WINDOW_UPDATE frame
            dst.buffer(frame.into())
                .expect("invalid WINDOW_UPDATE frame");

            // Update flow control
            self.flow
                .inc_window(incr)
                .expect("unexpected flow control state");
        }

        Ok(BufferStatus::Complete)
    }

    /// Send stream level window update
    pub fn send_stream_window_updates<T, B>(
        &mut self,
        store: &mut Store,
        counts: &mut Counts,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> io::Result<BufferStatus>
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        loop {
            // Ensure the codec has capacity
            if !dst.has_send_capacity() {
                return Ok(BufferStatus::CodecFull);
            }

            // Get the next stream
            let stream = match self.pending_window_updates.pop(store) {
                Some(stream) => stream,
                None => return Ok(BufferStatus::Complete),
            };

            counts.transition(stream, |_, stream| {
                tracing::trace!("pending_window_updates -- pop; stream={:?}", stream.id);
                debug_assert!(!stream.is_pending_window_update);

                if !stream.state.is_recv_streaming() {
                    // No need to send window updates on the stream if the stream is
                    // no longer receiving data.
                    //
                    // TODO: is this correct? We could possibly send a window
                    // update on a ReservedRemote stream if we already know
                    // we want to stream the data faster...
                    return;
                }

                // TODO: de-dup
                if let Some(incr) = stream.recv_flow.unclaimed_capacity() {
                    // Create the WINDOW_UPDATE frame
                    let frame = frame::WindowUpdate::new(stream.id, incr);

                    // Buffer it
                    dst.buffer(frame.into())
                        .expect("invalid WINDOW_UPDATE frame");

                    // Update flow control
                    stream
                        .recv_flow
                        .inc_window(incr)
                        .expect("unexpected flow control state");
                }
            })
        }
    }

    pub fn next_incoming(&mut self, store: &mut Store) -> Option<(store::Key, bool)> {
        // The accept queue already resolved the stream. The reset flag and the
        // refcount use that slot, so the caller does not look it up again.
        let Some((stream, sp)) = self.pending_accept.pop_slot(store) else {
            return None;
        };
        crate::wire::headers::publish_accept_facts(self.accept_facts.pop_front());
        #[allow(
            unsafe_code,
            reason = "the accepted stream's reset flag and refcount share the popped slot"
        )]
        // SAFETY: `pop_slot` resolved this occupied slot. The flag read and
        // refcount increment do not remove it.
        let remote_reset = unsafe {
            tracing::trace!(
                "next_incoming; id={:?}, state={:?}",
                (*sp).id,
                (*sp).state
            );
            let remote_reset = (*sp).state.is_remote_reset();
            assert!((*sp).ref_count < usize::MAX);
            (*sp).ref_count += 1;
            remote_reset
        };
        Some((stream.key(), remote_reset))
    }

    /// The caller matches this event immediately. Out of line, the bytes
    /// landed in this return slot and were copied out again.
    #[inline(always)]
    pub fn poll_data(
        &mut self,
        cx: &Context,
        stream: &mut Stream,
    ) -> Poll<Option<Result<DataEvent, proto::Error>>> {
        // Headers or trailers at the front are not data. Popping them and
        // inserting them again only to return `None` allocates a slab slot
        // for the same frame. Leave the queue alone.
        match stream.pending_recv.front(&self.buffer) {
            Some(Event::Data(_)) => {}
            Some(_) => {
                // Notify the recv task. This is done just in case
                // `poll_trailers` was called.
                //
                // It is very likely that `notify_recv` will just be a no-op (as
                // the task will be None), so this isn't really much of a
                // performance concern. It also means we don't have to track
                // state to see if `poll_trailers` was called before `poll_data`
                // returned `None`.
                stream.notify_recv();
                return Poll::Ready(None);
            }
            None => return self.schedule_recv(cx, stream),
        }
        match stream.pending_recv.pop_front(&mut self.buffer) {
            Some(Event::Data(data)) => Poll::Ready(Some(Ok(data))),
            // `front` just observed Data. A different value means the queue
            // changed; put it back rather than dropping a frame.
            Some(event) => {
                stream.pending_recv.push_front(&mut self.buffer, event);
                stream.notify_recv();
                Poll::Ready(None)
            }
            None => self.schedule_recv(cx, stream),
        }
    }

    pub fn poll_trailers(
        &mut self,
        cx: &Context,
        stream: &mut Stream,
    ) -> Poll<Option<Result<HeaderMap, proto::Error>>> {
        match stream.pending_recv.pop_front(&mut self.buffer) {
            Some(Event::Trailers(trailers)) => Poll::Ready(Some(Ok(*trailers))),
            Some(event) => {
                // Frame is not trailers.. not ready to poll trailers yet.
                stream.pending_recv.push_front(&mut self.buffer, event);
                stream.recv_task = Some(cx.waker().clone());
                Poll::Pending
            }
            None => self.schedule_recv(cx, stream),
        }
    }

    fn schedule_recv<T>(
        &mut self,
        cx: &Context,
        stream: &mut Stream,
    ) -> Poll<Option<Result<T, proto::Error>>> {
        if stream.state.ensure_recv_open()? {
            // Request to get notified once more frames arrive
            stream.recv_task = Some(cx.waker().clone());
            Poll::Pending
        } else {
            // No more frames will be received
            Poll::Ready(None)
        }
    }
}

#[cold]
#[inline(never)]
fn data_frame_exceeds_window() -> ! {
    panic!("DATA frame larger than the maximum flow-control window");
}

#[cold]
#[inline(never)]
fn unexpected_data(id: StreamId) -> Error {
    // TODO: There are cases where this can be a stream error of
    // STREAM_CLOSED instead...
    // Receiving a DATA frame when not expecting one is a protocol error.
    proto_err!(conn: "unexpected DATA frame; stream={:?}", id);
    Error::library_go_away(Reason::PROTOCOL_ERROR)
}

#[cold]
#[inline(never)]
fn content_length_overflow(id: StreamId, len: usize) -> Error {
    proto_err!(stream:
        "recv_data: content-length overflow; stream={:?}; len={:?}",
        id,
        len,
    );
    Error::library_reset(id, Reason::PROTOCOL_ERROR)
}

#[cold]
#[inline(never)]
fn content_length_underflow(id: StreamId, len: usize) -> Error {
    proto_err!(stream:
        "recv_data: content-length underflow; stream={:?}; len={:?}",
        id,
        len,
    );
    Error::library_reset(id, Reason::PROTOCOL_ERROR)
}

#[cold]
#[inline(never)]
fn recv_close_failed(id: StreamId) -> Error {
    proto_err!(conn: "recv_data: failed to transition to closed state; stream={:?}", id);
    Error::library_go_away(Reason::PROTOCOL_ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_recv_buffer_caps_capacity_before_overflow() {
        const FRAME_LEN: usize = 1 << 20;
        const FRAME_COUNT: usize = (u32::MAX as usize / FRAME_LEN) + 1;

        let config = Config {
            initial_max_send_streams: 0,
            local_max_buffer_size: 0,
            local_next_stream_id: 2.into(),
            local_push_enabled: false,
            extended_connect_protocol_enabled: false,
            local_reset_duration: Duration::ZERO,
            local_reset_max: 0,
            remote_reset_max: 0,
            remote_init_window_sz: DEFAULT_INITIAL_WINDOW_SIZE,
            remote_max_initiated: None,
            local_max_error_reset_streams: None,
            data_frame_budget: DEFAULT_DATA_FRAME_BUDGET,
        };
        let mut recv = Recv::new(
            peer::Dyn::Server,
            &config,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut store = Store::new();
        let mut stream = store.insert(
            StreamId::from(1),
            Stream::new(StreamId::from(1), 0, DEFAULT_INITIAL_WINDOW_SIZE),
        );
        let data = Bytes::from(vec![0; FRAME_LEN]);

        for _ in 0..FRAME_COUNT {
            stream.pending_recv.push_back(
                &mut recv.buffer,
                Event::Data(DataEvent {
                    payload: data.clone(),
                    is_budgeted: true,
                }),
            );
        }
        stream.in_flight_recv_data = DEFAULT_INITIAL_WINDOW_SIZE;
        recv.in_flight_data = DEFAULT_INITIAL_WINDOW_SIZE;

        let mut counts = Counts::new(
            peer::Dyn::Server,
            &config,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        recv.clear_recv_buffer(&mut stream, &mut None, &mut counts);

        assert!(stream.pending_recv.is_empty());
        assert_eq!(stream.in_flight_recv_data, 0);
        assert_eq!(recv.in_flight_data, 0);
    }
}

// ===== impl Open =====

impl Open {
    pub fn is_push_promise(&self) -> bool {
        matches!(*self, Self::PushPromise)
    }
}

// ===== impl RecvHeaderBlockError =====

impl<T> From<Error> for RecvHeaderBlockError<T> {
    fn from(err: Error) -> Self {
        RecvHeaderBlockError::State(err)
    }
}
