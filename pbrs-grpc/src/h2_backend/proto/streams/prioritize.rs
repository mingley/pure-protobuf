use super::store::Resolve;
use super::*;

use crate::h2_backend::frame::Reason;

use crate::h2_backend::codec::UserError;
use crate::h2_backend::codec::UserError::*;

use std::{
    cmp::{self, Ordering},
    fmt, io, mem,
    task::Waker,
};

/// # Warning
///
/// Queued streams are ordered by stream ID, as we need to ensure that
/// lower-numbered streams are sent headers before higher-numbered ones.
/// This is because "idle" stream IDs – those which have been initiated but
/// have yet to receive frames – will be implicitly closed on receipt of a
/// frame on a higher stream ID. If these queues was not ordered by stream
/// IDs, some mechanism would be necessary to ensure that the lowest-numbered]
/// idle stream is opened first.
#[derive(Debug)]
pub(super) struct Prioritize {
    /// Queue of streams waiting for socket capacity to send a frame.
    pending_send: store::Queue<stream::NextSend>,

    /// Queue of streams waiting for window capacity to produce data.
    pending_capacity: store::Queue<stream::NextSendCapacity>,

    /// Streams waiting for capacity due to max concurrency
    ///
    /// The `SendRequest` handle is `Clone`. This enables initiating requests
    /// from many tasks. However, offering this capability while supporting
    /// backpressure at some level is tricky. If there are many `SendRequest`
    /// handles and a single stream becomes available, which handle gets
    /// assigned that stream? Maybe that handle is no longer ready to send a
    /// request.
    ///
    /// The strategy used is to allow each `SendRequest` handle one buffered
    /// request. A `SendRequest` handle is ready to send a request if it has no
    /// associated buffered requests. This is the same strategy as `mpsc` in the
    /// futures library.
    pending_open: store::Queue<stream::NextOpen>,

    /// Connection level flow control governing sent data
    flow: FlowControl,

    /// Stream ID of the last stream opened.
    last_opened_id: StreamId,

    /// What `DATA` frame is currently being sent in the codec.
    in_flight_data_frame: InFlightData,

    /// The maximum amount of bytes a stream should buffer.
    max_buffer_size: usize,

    /// Cleared when a frame is scheduled. The connection skips its send
    /// lock while this stays set.
    buffer_idle: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Debug, Eq, PartialEq)]
enum InFlightData {
    /// There is no `DATA` frame in flight.
    Nothing,
    /// There is a `DATA` frame in flight belonging to the given stream.
    DataFrame(store::Key),
    /// There was a `DATA` frame, but the stream's queue was since cleared.
    Drop,
}

pub(crate) struct Prioritized<B> {
    /// Bytes still owned by this frame. `limit` is how many of them this
    /// write may send; the tail stays in `inner` for a later frame.
    ///
    /// A `bytes::Take` is a `usize` wider than this, and that width was
    /// copied on every queued frame.
    inner: B,
    limit: u32,
    end_of_stream: bool,
    stream: store::Key,
}

// Scratch ownership for one staging pass, never stored in the engine. Visible
// bytes do not bound an opaque backing owner's size; bound the owner count.
const RETIRED_DATA_LIMIT: usize = 16;

pub(super) struct RetiredData<B> {
    frames: [Option<frame::Data<Prioritized<B>>>; RETIRED_DATA_LIMIT],
    len: usize,
}

impl<B> RetiredData<B> {
    pub(super) fn new() -> Self {
        Self {
            frames: [const { None }; RETIRED_DATA_LIMIT],
            len: 0,
        }
    }

    pub(super) fn is_full(&self) -> bool {
        self.len == RETIRED_DATA_LIMIT
    }

    pub(super) fn push(&mut self, frame: frame::Data<Prioritized<B>>) {
        let slot = self
            .frames
            .get_mut(self.len)
            .expect("retirement capacity checked before taking frame");
        debug_assert!(slot.is_none());
        *slot = Some(frame);
        self.len += 1;
    }

    pub(super) fn last(&self) -> &frame::Data<Prioritized<B>> {
        self.frames
            .get(self.len.checked_sub(1).expect("retired frame present"))
            .and_then(Option::as_ref)
            .expect("occupied retirement slot")
    }

    fn pop(&mut self) -> frame::Data<Prioritized<B>> {
        self.len = self.len.checked_sub(1).expect("retired frame present");
        self.frames
            .get_mut(self.len)
            .and_then(Option::take)
            .expect("occupied retirement slot")
    }

    pub(super) fn has_tail(&self) -> bool
    where
        B: Buf,
    {
        self.last().payload().inner.has_remaining()
    }
}

// ===== impl Prioritize =====

impl Prioritize {
    pub fn new(
        config: &Config,
        buffer_idle: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Prioritize {
        let mut flow = FlowControl::new();

        flow.inc_window(config.remote_init_window_sz)
            .expect("invalid initial window size");

        // TODO: proper error handling
        let _res = flow.assign_capacity(config.remote_init_window_sz);
        debug_assert!(_res.is_ok());

        tracing::trace!("Prioritize::new; flow={:?}", flow);

        Prioritize {
            pending_send: store::Queue::new(),
            pending_capacity: store::Queue::new(),
            pending_open: store::Queue::new(),
            flow,
            last_opened_id: StreamId::ZERO,
            in_flight_data_frame: InFlightData::Nothing,
            max_buffer_size: config.local_max_buffer_size,
            buffer_idle,
        }
    }

    pub(crate) fn max_buffer_size(&self) -> usize {
        self.max_buffer_size
    }

    /// Queue a frame to be sent to the remote
    #[allow(
        unsafe_code,
        reason = "the stream pointer stays valid while this frame is queued"
    )]
    pub fn queue_frame<B>(
        &mut self,
        frame: Frame<B>,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
        sp: *mut Stream,
        task: &mut Option<Waker>,
    ) {
        // SAFETY: `sp` is the slot the caller already resolved. Queuing the
        // frame does not remove it. The ready flags are the same ones
        // `schedule_send` reads through a second slab index.
        unsafe {
            (*sp).pending_send.push_back(buffer, frame);
            if (*sp).is_pending_open || (*sp).is_pending_push {
                return;
            }
        }
        // Before the wake, so a connection poll cannot observe idle.
        super::clear_buffer_idle(&self.buffer_idle);
        // `push` would index this same slot again for the queued flag.
        self.pending_send.push_resolved(stream, sp);
        if let Some(task) = task.take() {
            task.wake();
        }
    }

    pub fn schedule_send(&mut self, stream: &mut store::Ptr, task: &mut Option<Waker>) {
        // If the stream is waiting to be opened, nothing more to do.
        if stream.is_send_ready() {
            // Before the wake, so a connection poll cannot observe idle.
            super::clear_buffer_idle(&self.buffer_idle);
            // Queue the stream
            self.pending_send.push(stream);

            // Notify the connection.
            if let Some(task) = task.take() {
                task.wake();
            }
        }
    }

    pub fn queue_open(&mut self, stream: &mut store::Ptr, sp: *mut Stream, counts: &mut Counts) {
        super::clear_buffer_idle(&self.buffer_idle);
        // `push` would index the slot the caller already resolved.
        if self.pending_open.push_resolved(stream, sp) {
            counts.inc_pending_send_streams();
        }
    }

    /// Send a data frame
    #[allow(
        unsafe_code,
        reason = "one slab lookup for the stream fields this send reads and writes"
    )]
    pub fn send_data<B>(
        &mut self,
        pending: &mut Option<frame::Data<B>>,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
        mut sp: *mut Stream,
        counts: &mut Counts,
        task: &mut Option<Waker>,
    ) -> Result<(), UserError>
    where
        B: Buf,
    {
        // Validation borrows the caller's owner. Rejection or a payload
        // callback panic leaves it there until both outer guards are gone.
        let frame = pending.as_ref().expect("pending DATA before acceptance");
        let sz = frame.payload().remaining();

        if sz > MAX_WINDOW_SIZE as usize {
            return Err(UserError::PayloadTooBig);
        }

        let sz = sz as WindowSize;

        // SAFETY: `sp` is the slot the caller already resolved. Capacity
        // assignment and queueing do not remove it.
        unsafe {
            if !(*sp).state.is_send_streaming() {
                if (*sp).state.is_closed() {
                    return Err(InactiveStreamId);
                } else {
                    return Err(UnexpectedFrameType);
                }
            }

            // Update the buffered data counter
            (*sp).buffered_send_data += sz as usize;

            // Implicitly request more send capacity if not enough has been
            // requested yet.
            if ((*sp).requested_send_capacity as usize) < (*sp).buffered_send_data {
                // Update the target requested capacity
                (*sp).requested_send_capacity =
                    cmp::min((*sp).buffered_send_data, WindowSize::MAX as usize) as WindowSize;

                // `try_assign_capacity` will queue the stream to `pending_capacity` if the capcaity
                // cannot be assigned at the time it is called. `sp` is that slot.
                self.try_assign_capacity_at(stream, sp);
            }

            if frame.is_end_stream() {
                (*sp).state.send_close();
                sp = self.reserve_capacity_at(0, stream, sp, counts);
            }

            // The `buffered_send_data == 0` check is here so that, if a zero
            // length data frame is queued to the front (there is no previously
            // queued data), it gets sent out immediately even if there is no
            // available send window.
            //
            // Sending out zero length data frames can be done to signal
            // end-of-stream.
            //
            // All rejecting checks and capacity callbacks precede transfer.
            // Queue insertion owns the frame before any connection wake.
            let frame = pending.take().expect("accepted DATA still owned by caller");
            if (*sp).send_flow.available() > 0 || (*sp).buffered_send_data == 0 {
                // The stream currently has capacity to send the data frame, so
                // queue it up and notify the connection task.
                self.queue_frame(frame.into(), buffer, stream, sp, task);
            } else {
                // The stream has no capacity to send the frame now, save it but
                // don't notify the connection task. Once additional capacity
                // becomes available, the frame will be flushed.
                (*sp).pending_send.push_back(buffer, frame.into());
            }
        }

        Ok(())
    }

    /// [`Self::reserve_capacity`] when the caller already resolved the slot.
    ///
    /// An end-of-stream send has just made the requested capacity match the
    /// buffered amount. That no-op used to look the stream up twice.
    #[allow(
        unsafe_code,
        reason = "the buffered and requested capacities are read through the slot the caller already resolved"
    )]
    pub fn reserve_capacity_at(
        &mut self,
        capacity: WindowSize,
        stream: &mut store::Ptr,
        sp: *mut Stream,
        counts: &mut Counts,
    ) -> *mut Stream {
        // SAFETY: `sp` is this stream. The equal case does not remove it.
        // The other cases re-enter through `reserve_capacity`.
        let total = unsafe { (capacity as usize) + (*sp).buffered_send_data };
        // SAFETY: the same live resolved slot remains occupied after the read above.
        if total == unsafe { (*sp).requested_send_capacity as usize } {
            return sp;
        }
        self.reserve_capacity(capacity, stream, counts);
        // The safe reserve path can reborrow this slot. Physical stability
        // alone does not keep the old pointer valid for the caller.
        stream.stream_ptr()
    }

    /// Request capacity to send data
    pub fn reserve_capacity(
        &mut self,
        capacity: WindowSize,
        stream: &mut store::Ptr,
        counts: &mut Counts,
    ) {
        // Actual capacity is `capacity` + the current amount of buffered data.
        // If it were less, then we could never send out the buffered data.
        let capacity = (capacity as usize) + stream.buffered_send_data;

        match capacity.cmp(&(stream.requested_send_capacity as usize)) {
            Ordering::Equal => {
                // Nothing to do
            }
            Ordering::Less => {
                // Update the target requested capacity
                stream.requested_send_capacity = capacity as WindowSize;

                // Currently available capacity assigned to the stream
                let available = stream.send_flow.available().as_size();

                // If the stream has more assigned capacity than requested, reclaim
                // some for the connection
                if available as usize > capacity {
                    let diff = available - capacity as WindowSize;

                    // TODO: proper error handling
                    let _res = stream.send_flow.claim_capacity(diff);
                    debug_assert!(_res.is_ok());

                    self.assign_connection_capacity(diff, stream, counts);
                }
            }
            Ordering::Greater => {
                // If trying to *add* capacity, but the stream send side is closed,
                // there's nothing to be done.
                if stream.state.is_send_closed() {
                    return;
                }

                // Update the target requested capacity
                stream.requested_send_capacity =
                    cmp::min(capacity, WindowSize::MAX as usize) as WindowSize;

                // Try to assign additional capacity to the stream. If none is
                // currently available, the stream will be queued to receive some
                // when more becomes available.
                self.try_assign_capacity(stream);
            }
        }
    }

    pub fn recv_stream_window_update(
        &mut self,
        inc: WindowSize,
        stream: &mut store::Ptr,
    ) -> Result<(), Reason> {
        let span = tracing::trace_span!(
            "recv_stream_window_update",
            ?stream.id,
            ?stream.state,
            inc,
            flow = ?stream.send_flow
        );
        let _e = span.enter();

        if stream.state.is_send_closed() && stream.buffered_send_data == 0 {
            // We can't send any data, so don't bother doing anything else.
            return Ok(());
        }

        // Update the stream level flow control.
        stream.send_flow.inc_window(inc)?;

        // If the stream is waiting on additional capacity, then this will
        // assign it (if available on the connection) and notify the producer
        self.try_assign_capacity(stream);

        Ok(())
    }

    pub fn recv_connection_window_update(
        &mut self,
        inc: WindowSize,
        store: &mut Store,
        counts: &mut Counts,
    ) -> Result<(), Reason> {
        // Update the connection's window
        self.flow.inc_window(inc)?;

        self.assign_connection_capacity(inc, store, counts);
        Ok(())
    }

    /// Reclaim all capacity assigned to the stream and re-assign it to the
    /// connection
    pub fn reclaim_all_capacity(&mut self, stream: &mut store::Ptr, counts: &mut Counts) {
        let available = stream.send_flow.available().as_size();
        if available > 0 {
            // TODO: proper error handling
            let _res = stream.send_flow.claim_capacity(available);
            debug_assert!(_res.is_ok());
            // Re-assign all capacity to the connection
            self.assign_connection_capacity(available, stream, counts);
        }
    }

    /// The deadline path also redistributes credit, but its callbacks must not
    /// run while the engine is locked. The ordinary send path stays unchanged.
    pub fn reclaim_terminal_capacity(
        &mut self,
        stream: &mut store::Ptr,
        counts: &mut Counts,
        wake: &mut Vec<Waker>,
    ) {
        let available = stream.send_flow.available().as_size();
        if available == 0 {
            return;
        }
        let reclaimed = stream.send_flow.claim_capacity(available);
        debug_assert!(reclaimed.is_ok());
        let assigned = self.flow.assign_capacity(available);
        debug_assert!(assigned.is_ok());
        while self.flow.available() > 0 {
            let Some(other) = self.pending_capacity.pop(stream) else {
                return;
            };
            if !(other.state.is_send_streaming() || other.buffered_send_data > 0) {
                continue;
            }
            counts.transition(other, |_, other| {
                let before = other.capacity(self.max_buffer_size);
                let task = other.take_send_task_for_terminal();
                self.try_assign_capacity(other);
                if other.capacity(self.max_buffer_size) > before {
                    wake.extend(task);
                } else {
                    other.restore_send_task_after_terminal(task);
                }
            });
        }
    }

    /// Reclaim just reserved capacity, not buffered capacity, and re-assign
    /// it to the connection
    pub fn reclaim_reserved_capacity(&mut self, stream: &mut store::Ptr, counts: &mut Counts) {
        // only reclaim reserved capacity that isn't already buffered
        if stream.send_flow.available().as_size() as usize > stream.buffered_send_data {
            let reserved =
                stream.send_flow.available().as_size() - stream.buffered_send_data as WindowSize;

            // Panic safety: due to how `reserved` is computed it can't be greater
            // than what's available.
            stream
                .send_flow
                .claim_capacity(reserved)
                .expect("window size should be greater than reserved");

            self.assign_connection_capacity(reserved, stream, counts);
        }
    }

    pub fn clear_pending_capacity(&mut self, store: &mut Store, counts: &mut Counts) {
        let span = tracing::trace_span!("clear_pending_capacity");
        let _e = span.enter();
        while let Some(stream) = self.pending_capacity.pop(store) {
            counts.transition(stream, |_, stream| {
                tracing::trace!(?stream.id, "clear_pending_capacity");
            })
        }
    }

    pub fn assign_connection_capacity<R>(
        &mut self,
        inc: WindowSize,
        store: &mut R,
        counts: &mut Counts,
    ) where
        R: Resolve,
    {
        let span = tracing::trace_span!("assign_connection_capacity", inc);
        let _e = span.enter();

        // TODO: proper error handling
        let _res = self.flow.assign_capacity(inc);
        debug_assert!(_res.is_ok());

        // Assign newly acquired capacity to streams pending capacity.
        while self.flow.available() > 0 {
            let stream = match self.pending_capacity.pop(store) {
                Some(stream) => stream,
                None => return,
            };

            // Streams pending capacity may have been reset before capacity
            // became available. In that case, the stream won't want any
            // capacity, and so we shouldn't "transition" on it, but just evict
            // it and continue the loop.
            if !(stream.state.is_send_streaming() || stream.buffered_send_data > 0) {
                continue;
            }

            counts.transition(stream, |_, stream| {
                // Try to assign capacity to the stream. This will also re-queue the
                // stream if there isn't enough connection level capacity to fulfill
                // the capacity request.
                self.try_assign_capacity(stream);
            })
        }
    }

    /// Request capacity to send data
    fn try_assign_capacity(&mut self, stream: &mut store::Ptr) {
        let sp = stream.stream_ptr();
        self.try_assign_capacity_at(stream, sp);
    }

    /// [`Self::try_assign_capacity`] through a slot the caller already resolved.
    ///
    /// The open check, the window store, and both queue links used to look
    /// that slot up again.
    #[allow(
        unsafe_code,
        reason = "capacity and the send-queue flags are updated through the slot the caller already resolved"
    )]
    fn try_assign_capacity_at(&mut self, stream: &mut store::Ptr, sp: *mut Stream) {
        // SAFETY: `sp` is `stream.stream_ptr()`. Assigning capacity and linking
        // the queues do not remove the slab slot.
        unsafe {
            // Streams over the max concurrent count should not have capacity assign to avoid starving the connection
            // capacity for open streams
            if (*sp).is_pending_open {
                return;
            }

            let total_requested = (*sp).requested_send_capacity;

            // Total requested should never go below actual assigned
            // (Note: the window size can go lower than assigned)
            debug_assert!((*sp).send_flow.available() <= total_requested as usize);

            // The amount of additional capacity that the stream requests.
            // Don't assign more than the window has available!
            let additional = cmp::min(
                total_requested - (*sp).send_flow.available().as_size(),
                // Can't assign more than what is available
                (*sp).send_flow.window_size() - (*sp).send_flow.available().as_size(),
            );
            if additional == 0 {
                // Nothing more to do
                return;
            }

            // The stream may have been reset or closed since capacity was requested.
            if !(*sp).state.is_send_streaming() && (*sp).buffered_send_data == 0 {
                return;
            }

            // The amount of currently available capacity on the connection
            let conn_available = self.flow.available().as_size();

            // First check if capacity is immediately available
            if conn_available > 0 {
                // The amount of capacity to assign to the stream
                // TODO: Should prioritization factor into this?
                let assign = cmp::min(conn_available, additional);

                // Assign the capacity to the stream
                (*sp).assign_capacity(assign, self.max_buffer_size);

                // Claim the capacity from the connection
                // TODO: proper error handling
                let _res = self.flow.claim_capacity(assign);
                debug_assert!(_res.is_ok());
            }

            if (*sp).send_flow.available() < (*sp).requested_send_capacity as usize
                && (*sp).send_flow.has_unavailable()
            {
                // The stream requires additional capacity and the stream's
                // window has available capacity, but the connection window
                // does not.
                //
                // In this case, the stream needs to be queued up for when the
                // connection has more capacity.
                self.pending_capacity.push_resolved(stream, sp);
            }

            // If data is buffered and the stream is send ready, then
            // schedule the stream for execution
            if (*sp).buffered_send_data > 0 && (*sp).is_send_ready() {
                // TODO: This assertion isn't *exactly* correct. There can still be
                // buffered send data while the stream's pending send queue is
                // empty. This can happen when a large data frame is in the process
                // of being **partially** sent. Once the window has been sent, the
                // data frame will be returned to the prioritization layer to be
                // re-scheduled.
                //
                // That said, it would be nice to figure out how to make this
                // assertion correctly.
                //
                // debug_assert!(!stream.pending_send.is_empty());

                super::clear_buffer_idle(&self.buffer_idle);
                self.pending_send.push_resolved(stream, sp);
            }
        }
    }

    /// No queued frame and no data frame sitting in the codec.
    pub(super) fn is_idle(&self) -> bool {
        matches!(self.in_flight_data_frame, InFlightData::Nothing)
            && self.pending_open.is_empty()
            && self.pending_send.is_empty()
    }

    /// A queued deadline/reset can invalidate only the unencoded tail of a
    /// DATA frame. Transfer that tail out of the locked engine before dropping.
    pub fn take_discarded_frame<T, B>(
        &mut self,
        dst: &mut Codec<T, Prioritized<B>>,
    ) -> Option<frame::Data<Prioritized<B>>>
    where
        B: Buf,
    {
        if !matches!(self.in_flight_data_frame, InFlightData::Drop) {
            return None;
        }
        let frame = dst.take_last_data_frame()?;
        self.in_flight_data_frame = InFlightData::Nothing;
        Some(frame)
    }

    pub fn buffer_pending<T, B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        store: &mut Store,
        counts: &mut Counts,
        dst: &mut Codec<T, Prioritized<B>>,
        retired: &mut RetiredData<B>,
        discarded: &mut Vec<Frame<B>>,
    ) -> io::Result<BufferStatus>
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        // Nothing in flight and nothing queued. Reclaim would observe an empty
        // codec slot, and both pops would return immediately.
        if self.is_idle() {
            return Ok(BufferStatus::Idle);
        }
        // Reclaim any frame that has previously been written
        self.reclaim_frame(buffer, store, dst, retired);

        // The max frame length
        let max_frame_len = dst.max_send_frame_size();

        loop {
            if retired.is_full() {
                return Ok(BufferStatus::RetiredFull);
            }
            if !dst.has_send_capacity() {
                return Ok(BufferStatus::CodecFull);
            }

            // Steady streams never queue an open. The pop is a separate
            // function so this loop does not pay its prologue when the queue
            // is empty.
            if !self.pending_open.is_empty() {
                if let Some((mut stream, sp)) = self.pop_pending_open(store, counts) {
                    self.pending_send.push_front_resolved(&mut stream, sp);
                    self.try_assign_capacity_at(&mut stream, sp);
                }
            }

            // `pop_frame` writes the frame into the codec. Returning it
            // copied the 56-byte enum just so this loop could match DATA.
            if self.pop_frame(buffer, store, max_frame_len, counts, dst, discarded) {
                // Only a fully encoded small DATA frame occupies the codec
                // slot. A larger payload stays in `next` and a header block
                // never sets it, so reclaim would only observe None.
                if dst.has_last_data_frame() {
                    self.reclaim_frame(buffer, store, dst, retired);
                }
            } else {
                // The queue is drained. Headers leave nothing in flight, so
                // this poll is idle and the connection can skip the next one.
                return Ok(if self.is_idle() {
                    BufferStatus::Idle
                } else {
                    BufferStatus::Complete
                });
            }
        }
    }

    pub fn reclaim_written_frame<T, B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        store: &mut Store,
        dst: &mut Codec<T, Prioritized<B>>,
        retired: &mut RetiredData<B>,
    ) -> bool
    where
        B: Buf,
    {
        self.reclaim_frame(buffer, store, dst, retired)
    }

    /// The codec finished this data frame. Clear the in-flight mark.
    /// The frame slab is not involved.
    pub(super) fn reclaim_consumed<B>(&mut self, frame: &frame::Data<Prioritized<B>>)
    where
        B: Buf,
    {
        let key = frame.payload().stream;
        match mem::replace(&mut self.in_flight_data_frame, InFlightData::Nothing) {
            InFlightData::Nothing => panic!("wasn't expecting a frame to reclaim"),
            InFlightData::Drop => {}
            InFlightData::DataFrame(found) => {
                debug_assert_eq!(found, key);
            }
        }
    }

    pub(super) fn reclaim_partial<B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        store: &mut Store,
        retired: &mut RetiredData<B>,
    ) -> bool
    where
        B: Buf,
    {
        self.reclaim_frame_inner(buffer, store, retired)
    }

    /// Tries to reclaim a pending data frame from the codec.
    ///
    /// Returns true if a frame was reclaimed.
    ///
    /// When a data frame is written to the codec, it may not be written in its
    /// entirety (large chunks are split up into potentially many data frames).
    /// In this case, the stream needs to be reprioritized.
    fn reclaim_frame<T, B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        store: &mut Store,
        dst: &mut Codec<T, Prioritized<B>>,
        retired: &mut RetiredData<B>,
    ) -> bool
    where
        B: Buf,
    {
        // First check if there are any data chunks to take back
        if let Some(frame) = dst.take_last_data_frame() {
            retired.push(frame);
            self.reclaim_frame_inner(buffer, store, retired)
        } else {
            false
        }
    }

    fn reclaim_frame_inner<B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        store: &mut Store,
        retired: &mut RetiredData<B>,
    ) -> bool
    where
        B: Buf,
    {
        let frame = retired.last();
        let key = frame.payload().stream;
        let eos = frame.payload().end_of_stream;

        match mem::replace(&mut self.in_flight_data_frame, InFlightData::Nothing) {
            InFlightData::Nothing => panic!("wasn't expecting a frame to reclaim"),
            InFlightData::Drop => {
                return false;
            }
            InFlightData::DataFrame(k) => {
                debug_assert_eq!(k, key);
            }
        }

        if !retired.has_tail() {
            return false;
        }
        // Resolve before removing the frame from the outer owner batch.
        let mut stream = store.resolve(key);
        let mut frame = retired.pop().map(|prioritized| prioritized.inner);
        if eos {
            frame.set_end_stream(true);
        }
        self.push_back_frame(frame.into(), buffer, &mut stream);
        true
    }

    /// Push the frame to the front of the stream's deque, scheduling the
    /// stream if needed.
    fn push_back_frame<B>(
        &mut self,
        frame: Frame<B>,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
    ) {
        // Push the frame to the front of the stream's deque
        stream.pending_send.push_front(buffer, frame);

        // If needed, schedule the sender
        if stream.send_flow.available() > 0 {
            debug_assert!(!stream.pending_send.is_empty());
            self.pending_send.push(stream);
        }
    }

    pub fn clear_queue<B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
        discarded: &mut Vec<Frame<B>>,
    ) {
        let span = tracing::trace_span!("clear_queue", ?stream.id);
        let _e = span.enter();

        // A payload can own a caller-defined destructor. Detach the frames
        // here; the outer caller owns this Vec before taking either engine
        // lock, so payload destruction also follows both guards on unwind.
        while let Some(frame) = stream.pending_send.pop_front(buffer) {
            discarded.push(frame);
        }

        stream.buffered_send_data = 0;
        stream.requested_send_capacity = 0;
        if let InFlightData::DataFrame(key) = self.in_flight_data_frame {
            if stream.key() == key {
                // This stream could get cleaned up now - don't allow the buffered frame to get reclaimed.
                self.in_flight_data_frame = InFlightData::Drop;
            }
        }
    }

    /// Detach unsent payloads before a terminal response. Initial response
    /// headers may still be queued, so native trailers must keep those in order.
    /// The caller drops detached owners after releasing both engine locks.
    pub fn discard_queued<B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
        preserve_headers: bool,
        discarded: &mut Vec<Frame<B>>,
    ) {
        let mut previous = mem::replace(&mut stream.pending_send, super::buffer::Deque::new());
        while let Some(frame) = previous.pop_front(buffer) {
            if preserve_headers && !matches!(&frame, Frame::Data(_)) {
                stream.pending_send.push_back(buffer, frame);
            } else {
                discarded.push(frame);
            }
        }
        stream.buffered_send_data = 0;
        stream.requested_send_capacity = 0;
        if let InFlightData::DataFrame(key) = self.in_flight_data_frame {
            if stream.key() == key {
                // The codec must finish the current HTTP/2 frame's advertised
                // length. Once that finishes, do not requeue its unsent tail.
                self.in_flight_data_frame = InFlightData::Drop;
            }
        }
    }

    pub fn clear_pending_send(&mut self, store: &mut Store, counts: &mut Counts) {
        while let Some(mut stream) = self.pending_send.pop(store) {
            let is_pending_reset = stream.is_pending_reset_expiration();
            if let Some(reason) = stream.state.get_scheduled_reset() {
                stream.set_reset(reason, Initiator::Library);
            }
            counts.transition_after(stream, is_pending_reset);
        }
    }

    pub fn clear_pending_open(&mut self, store: &mut Store, counts: &mut Counts) {
        while let Some(stream) = self.pending_open.pop(store) {
            counts.dec_pending_send_streams();
            let is_pending_reset = stream.is_pending_reset_expiration();
            counts.transition_after(stream, is_pending_reset);
            counts.wake_send_capacity();
        }
    }

    #[allow(
        unsafe_code,
        reason = "the popped stream's slab slot is read through one pointer instead of reindexing"
    )]
    fn pop_frame<T, B>(
        &mut self,
        buffer: &mut Buffer<Frame<B>>,
        store: &mut Store,
        max_len: usize,
        counts: &mut Counts,
        dst: &mut Codec<T, Prioritized<B>>,
        discarded: &mut Vec<Frame<B>>,
    ) -> bool
    where
        T: AsyncWrite + Unpin,
        B: Buf,
    {
        loop {
            match self.pending_send.pop_slot(store) {
                Some((mut stream, sp)) => {
                    // It's possible that this stream, besides having data to send,
                    // is also queued to send a reset, and thus is already in the queue
                    // to wait for "some time" after a reset.
                    //
                    // To be safe, we just always ask the stream.
                    // SAFETY: `pop_slot` resolved this occupied slot. This arm does not
                    // insert into the slab or remove the stream until
                    // `transition_after`, and `sp` is not used after that.
                    let is_pending_reset = unsafe { (*sp).is_pending_reset_expiration() };

                    // SAFETY: sp is the occupied slot resolved by the current send operation.
                    // Queue/flow updates do not grow the stream slab; close cleanup is last.
                    let frame = match unsafe { (*sp).pending_send.pop_front(buffer) } {
                        // Headers are a pointer, but the frame enum is padded to
                        // the data variant. Holding that value across the
                        // requeue and close checks copied it into `buffer`.
                        Some(Frame::Headers(frame)) => {
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if cfg!(debug_assertions) && unsafe { (*sp).state.is_idle() } {
                                // SAFETY: sp is the occupied slot resolved by the current send operation.
                                // Queue/flow updates do not grow the stream slab; close cleanup is last.
                                debug_assert!(unsafe { (*sp).id } > self.last_opened_id);
                                // SAFETY: sp is the occupied slot resolved by the current send operation.
                                // Queue/flow updates do not grow the stream slab; close cleanup is last.
                                self.last_opened_id = unsafe { (*sp).id };
                            }
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if unsafe {
                                !(*sp).pending_send.is_empty() || (*sp).state.is_scheduled_reset()
                            } {
                                self.pending_send.push_resolved(&mut stream, sp);
                            }
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            let closing = unsafe {
                                (*sp).state.is_closed()
                                    && !((*sp).protocol_unlinked && (*sp).ref_count > 0)
                            };
                            if closing {
                                counts.transition_after(stream, is_pending_reset);
                            }
                            dst.buffer_headers(frame).expect("invalid frame");
                            return true;
                        }
                        Some(Frame::Data(mut frame)) => {
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if let Some(reason) = unsafe { (*sp).state.get_scheduled_reset() } {
                                // If a reset is scheduled due to cancellation or
                                // an error, discard buffered DATA and let the `None`
                                // arm emit the RST_STREAM on the next iteration.
                                //
                                // NO_ERROR is excluded. Per RFC 9113 §8.1, a NO_ERROR
                                // stream reset may only be sent after a complete
                                // response, which requires sending all queued DATA.
                                if reason != Reason::NO_ERROR {
                                    self.discard_reset_data(
                                        sp,
                                        buffer,
                                        &mut stream,
                                        counts,
                                        frame,
                                        discarded,
                                    );
                                    continue;
                                }
                            }

                            // Get the amount of capacity remaining for stream's
                            // window.
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            let stream_capacity = unsafe { (*sp).send_flow.available() };
                            let sz = frame.payload().remaining();

                            // Zero length data frames always have capacity to
                            // be sent.
                            if sz > 0 && stream_capacity == 0 {
                                // The stream has no more capacity, this can
                                // happen if the remote reduced the stream
                                // window. Buffer the frame and wait for an update.
                                self.requeue_data(sp, buffer, frame);
                                continue;
                            }

                            // Only send up to the max frame length
                            let len = cmp::min(sz, max_len);

                            // Only send up to the stream's window capacity
                            let len =
                                cmp::min(len, stream_capacity.as_size() as usize) as WindowSize;

                            // There *must* be be enough connection level
                            // capacity at this point.
                            debug_assert!(len <= self.flow.window_size());

                            // Check if the stream level window the peer knows is available. In some
                            // scenarios, maybe the window we know is available but the window which
                            // peer knows is not.
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if len > 0 && unsafe { len > (*sp).send_flow.window_size() } {
                                self.requeue_data(sp, buffer, frame);
                                continue;
                            }

                            // Update the flow control
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            unsafe { (*sp).send_data(len, self.max_buffer_size) };

                            // Assign the capacity back to the connection that
                            // was just consumed from the stream in the previous
                            // line.
                            // TODO: proper error handling
                            let _res = self.flow.assign_capacity(len);
                            debug_assert!(_res.is_ok());

                            // TODO: proper error handling
                            let _res = self.flow.send_data(len);
                            debug_assert!(_res.is_ok());

                            // Wrap the frame's data payload to ensure that the
                            // correct amount of data gets written.
                            let eos = frame.is_end_stream();
                            let len = len as usize;

                            if frame.payload().remaining() > len {
                                frame.set_end_stream(false);
                            }

                            // The requeue and close checks do not read this
                            // payload. Building the prioritized frame before
                            // them left a 56-byte value live across the calls,
                            // and `buffer_data` then copied it again.
                            let key = stream.key();
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if cfg!(debug_assertions) && unsafe { (*sp).state.is_idle() } {
                                // SAFETY: sp is the occupied slot resolved by the current send operation.
                                // Queue/flow updates do not grow the stream slab; close cleanup is last.
                                debug_assert!(unsafe { (*sp).id } > self.last_opened_id);
                                // SAFETY: sp is the occupied slot resolved by the current send operation.
                                // Queue/flow updates do not grow the stream slab; close cleanup is last.
                                self.last_opened_id = unsafe { (*sp).id };
                            }
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if unsafe {
                                !(*sp).pending_send.is_empty() || (*sp).state.is_scheduled_reset()
                            } {
                                self.pending_send.push_resolved(&mut stream, sp);
                            }
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            let closing = unsafe {
                                (*sp).state.is_closed()
                                    && !((*sp).protocol_unlinked && (*sp).ref_count > 0)
                            };
                            if closing {
                                counts.transition_after(stream, is_pending_reset);
                            }
                            debug_assert_eq!(self.in_flight_data_frame, InFlightData::Nothing);
                            self.in_flight_data_frame = InFlightData::DataFrame(key);
                            let data = frame.map(|buf| Prioritized {
                                inner: buf,
                                limit: len as u32,
                                end_of_stream: eos,
                                stream: key,
                            });
                            // A 1 KiB body is chained. Writing it here keeps
                            // the frame out of `buffer_data`'s argument slot.
                            if dst.wants_chain(len) {
                                dst.buffer_chained_data(data);
                            } else {
                                dst.buffer_data(data).expect("invalid frame");
                            }
                            return true;
                        }
                        Some(Frame::PushPromise(pp)) => {
                            self.open_push_promise(&mut stream, counts, pp)
                        }
                        Some(frame) => frame.map(|_| {
                            unreachable!(
                                "Frame::map closure will only be called \
                                 on DATA frames."
                            )
                        }),
                        None => {
                            // SAFETY: sp is the occupied slot resolved by the current send operation.
                            // Queue/flow updates do not grow the stream slab; close cleanup is last.
                            if let Some(reason) = unsafe { (*sp).state.get_scheduled_reset() } {
                                self.emit_scheduled_reset(sp, reason)
                            } else {
                                self.drop_empty_send(sp, counts, stream, is_pending_reset);
                                continue;
                            }
                        }
                    };

                    // SAFETY: sp is the occupied slot resolved by the current send operation.
                    // Queue/flow updates do not grow the stream slab; close cleanup is last.
                    if cfg!(debug_assertions) && unsafe { (*sp).state.is_idle() } {
                        // SAFETY: sp is the occupied slot resolved by the current send operation.
                        // Queue/flow updates do not grow the stream slab; close cleanup is last.
                        debug_assert!(unsafe { (*sp).id } > self.last_opened_id);
                        // SAFETY: sp is the occupied slot resolved by the current send operation.
                        // Queue/flow updates do not grow the stream slab; close cleanup is last.
                        self.last_opened_id = unsafe { (*sp).id };
                    }

                    // SAFETY: sp is the occupied slot resolved by the current send operation.
                    // Queue/flow updates do not grow the stream slab; close cleanup is last.
                    if unsafe { !(*sp).pending_send.is_empty() || (*sp).state.is_scheduled_reset() }
                    {
                        // TODO: Only requeue the sender IF it is ready to send
                        // the next frame. i.e. don't requeue it if the next
                        // frame is a data frame and the stream does not have
                        // any more capacity.
                        // `push` would index the slot `sp` already names.
                        self.pending_send.push_resolved(&mut stream, sp);
                    }

                    // `transition_after` indexes the slab again. An open
                    // stream only needs the closed check, and `sp` is still
                    // the slot `pop` resolved. Close cleanup consumes `stream`.
                    // SAFETY: sp is the occupied slot resolved by the current send operation.
                    // Queue/flow updates do not grow the stream slab; close cleanup is last.
                    let closing = unsafe {
                        (*sp).state.is_closed() && !((*sp).protocol_unlinked && (*sp).ref_count > 0)
                    };
                    if closing {
                        counts.transition_after(stream, is_pending_reset);
                    }

                    // DATA and HEADERS were already matched above. Encoding
                    // them through `buffer` would match the same 56-byte
                    // enum again.
                    match frame {
                        Frame::Data(data) => {
                            debug_assert_eq!(self.in_flight_data_frame, InFlightData::Nothing);
                            self.in_flight_data_frame =
                                InFlightData::DataFrame(data.payload().stream);
                            dst.buffer_data(data).expect("invalid frame");
                        }
                        Frame::Headers(headers) => {
                            dst.buffer_headers(headers).expect("invalid frame");
                        }
                        other => {
                            dst.buffer(other).expect("invalid frame");
                        }
                    }
                    return true;
                }
                None => return false,
            }
        }
    }

    /// A reset is already scheduled. Put the data back and clear the queue.
    #[cold]
    #[inline(never)]
    #[allow(
        unsafe_code,
        reason = "requeue a data frame through the stream pointer already resolved"
    )]
    fn discard_reset_data<B>(
        &mut self,
        sp: *mut Stream,
        buffer: &mut Buffer<Frame<B>>,
        stream: &mut store::Ptr,
        counts: &mut Counts,
        frame: frame::Data<B>,
        discarded: &mut Vec<Frame<B>>,
    ) where
        B: Buf,
    {
        // SAFETY: sp is the occupied slot resolved by the current send operation.
        // Queue/flow updates do not grow the stream slab; close cleanup is last.
        unsafe { (*sp).pending_send.push_front(buffer, frame.into()) };
        self.clear_queue(buffer, stream, discarded);
        self.reclaim_all_capacity(stream, counts);
        self.pending_send.push(stream);
    }

    /// The stream window cannot take this data frame yet.
    #[cold]
    #[inline(never)]
    #[allow(
        unsafe_code,
        reason = "requeue a data frame through the stream pointer already resolved"
    )]
    fn requeue_data<B>(
        &mut self,
        sp: *mut Stream,
        buffer: &mut Buffer<Frame<B>>,
        frame: frame::Data<B>,
    ) where
        B: Buf,
    {
        // SAFETY: sp is the occupied slot resolved by the current send operation.
        // Queue/flow updates do not grow the stream slab; close cleanup is last.
        unsafe { (*sp).pending_send.push_front(buffer, frame.into()) };
    }

    /// A pushed stream is leaving `pending_push`.
    #[cold]
    #[inline(never)]
    fn open_push_promise<B>(
        &mut self,
        stream: &mut store::Ptr,
        counts: &mut Counts,
        pp: Box<frame::PushPromise>,
    ) -> Frame<Prioritized<B>>
    where
        B: Buf,
    {
        let mut pushed = stream.store_mut().find_mut(&pp.promised_id()).unwrap();
        pushed.is_pending_push = false;
        if !pushed.pending_send.is_empty() {
            if counts.can_inc_num_send_streams() {
                counts.inc_num_send_streams(&mut pushed);
                self.pending_send.push(&mut pushed);
            } else {
                let sp = pushed.stream_ptr();
                self.queue_open(&mut pushed, sp, counts);
            }
        }
        Frame::PushPromise(pp)
    }

    /// The send queue was empty. Emit a scheduled reset, or drop the stream.
    #[cold]
    #[inline(never)]
    #[allow(
        unsafe_code,
        reason = "read the resolved stream slot to emit a scheduled reset"
    )]
    fn emit_scheduled_reset<B>(&mut self, sp: *mut Stream, reason: Reason) -> Frame<Prioritized<B>>
    where
        B: Buf,
    {
        // SAFETY: sp is the occupied slot resolved by the current send operation.
        // Queue/flow updates do not grow the stream slab; close cleanup is last.
        unsafe { (*sp).set_reset(reason, Initiator::Library) };
        // SAFETY: sp is the occupied slot resolved by the current send operation.
        // Queue/flow updates do not grow the stream slab; close cleanup is last.
        Frame::Reset(frame::Reset::new(unsafe { (*sp).id }, reason))
    }

    /// No frames remain, and no reset is scheduled.
    #[cold]
    #[inline(never)]
    #[allow(
        unsafe_code,
        reason = "the resolved stream slot is still the one pop selected"
    )]
    fn drop_empty_send(
        &mut self,
        sp: *mut Stream,
        counts: &mut Counts,
        stream: store::Ptr,
        is_pending_reset: bool,
    ) {
        // SAFETY: sp is the occupied slot resolved by the current send operation.
        // Queue/flow updates do not grow the stream slab; close cleanup is last.
        debug_assert!(unsafe { (*sp).state.is_closed() });
        counts.transition_after(stream, is_pending_reset);
    }

    #[inline(never)]
    #[allow(
        unsafe_code,
        reason = "the opened stream is woken through the slot pop_slot just resolved"
    )]
    fn pop_pending_open<'s>(
        &mut self,
        store: &'s mut Store,
        counts: &mut Counts,
    ) -> Option<(store::Ptr<'s>, *mut Stream)> {
        // Steady state never queues an open. Skip the concurrency check.
        if self.pending_open.is_empty() {
            return None;
        }
        if counts.can_inc_num_send_streams() {
            if let Some((stream, sp)) = self.pending_open.pop_slot(store) {
                counts.dec_pending_send_streams();
                counts.inc_num_send_streams_at(sp);
                // SAFETY: `sp` is the stream `pop_slot` just resolved. Waking
                // the sender does not remove it.
                unsafe { (*sp).notify_send() };
                return Some((stream, sp));
            }
        }

        None
    }
}

// ===== impl Prioritized =====

impl<B> Buf for Prioritized<B>
where
    B: Buf,
{
    #[inline]
    fn remaining(&self) -> usize {
        cmp::min(self.inner.remaining(), self.limit as usize)
    }

    #[inline]
    fn chunk(&self) -> &[u8] {
        let bytes = self.inner.chunk();
        let n = cmp::min(bytes.len(), self.limit as usize);
        &bytes[..n]
    }

    #[inline]
    fn chunks_vectored<'a>(&'a self, dst: &mut [std::io::IoSlice<'a>]) -> usize {
        if self.limit as usize >= self.inner.remaining() {
            return self.inner.chunks_vectored(dst);
        }
        if dst.is_empty() || self.limit == 0 {
            return 0;
        }
        // The frame ends inside a later chunk. One limited slice is a valid
        // prefix; the next poll continues after `advance`.
        dst[0] = std::io::IoSlice::new(self.chunk());
        1
    }

    #[inline]
    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.limit as usize);
        self.inner.advance(cnt);
        self.limit -= cnt as u32;
    }
}

impl<B: Buf> fmt::Debug for Prioritized<B> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Prioritized")
            .field("remaining", &self.inner.remaining())
            .field("end_of_stream", &self.end_of_stream)
            .field("stream", &self.stream)
            .finish()
    }
}
