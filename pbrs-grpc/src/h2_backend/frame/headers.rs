use super::{util, StreamDependency, StreamId};
use crate::h2_backend::ext::Protocol;
use crate::h2_backend::frame::{Error, Frame, Head, Kind};
use crate::h2_backend::hpack::{self, BytesStr};

use http::header::{self, HeaderName, HeaderValue};
use http::{uri, HeaderMap, Method, Request, StatusCode, Uri};

use bytes::{Buf, BufMut, BytesMut};

use std::fmt;
use std::io::Cursor;
use std::ops::ControlFlow;

type EncodeBuf<'a> = bytes::buf::Limit<&'a mut BytesMut>;

const MAX_HEADER_LIST_ABUSE_MULTIPLIER: usize = 4;

/// Header frame
///
/// This could be either a request or a response.
#[derive(Eq, PartialEq)]
pub struct Headers {
    /// The ID of the stream with which this frame is associated.
    stream_id: StreamId,

    /// The stream dependency information, if any.
    stream_dep: Option<StreamDependency>,

    /// The header block fragment
    header_block: HeaderBlock,

    /// The associated flags
    flags: HeadersFlag,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct HeadersFlag(u8);

#[derive(Eq, PartialEq)]
pub struct PushPromise {
    /// The ID of the stream with which this frame is associated.
    stream_id: StreamId,

    /// The ID of the stream being reserved by this PushPromise.
    promised_id: StreamId,

    /// The header block fragment
    header_block: HeaderBlock,

    /// The associated flags
    flags: PushPromiseFlag,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct PushPromiseFlag(u8);

#[derive(Debug)]
pub struct Continuation {
    /// Stream ID of continuation frame
    stream_id: StreamId,

    header_block: EncodingHeaderBlock,
}

// TODO: These fields shouldn't be `pub`
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Pseudo {
    // Request
    pub method: Option<Method>,
    pub scheme: Option<BytesStr>,
    pub authority: Option<BytesStr>,
    pub path: Option<BytesStr>,
    pub protocol: Option<Protocol>,

    // Response
    pub status: Option<StatusCode>,
}

#[derive(Debug)]
pub struct Iter {
    /// Pseudo headers
    pseudo: Option<Pseudo>,

    /// Header fields
    fields: header::IntoIter<HeaderValue>,
}

#[derive(Debug, PartialEq, Eq)]
struct HeaderBlock {
    /// The decoded header fields
    fields: HeaderMap,

    /// Precomputed size of all of our header fields, for perf reasons
    field_size: usize,

    /// Set to true if decoding went over the max header list size.
    is_over_size: bool,

    /// `content-length` was inserted. Lets the receive path skip the hashed
    /// lookup on the stock gRPC blocks, which never carry that field.
    has_content_length: bool,

    /// Stock gRPC fields. Classified once so the connection-header walk and
    /// the replay identity walk are not both paid.
    stock: StandardMap,

    /// Accept-encoding facts for a fully decoded block. A cache hit copies
    /// this instead of walking the fields again.
    stock_facts: Option<crate::wire::headers::StockFacts>,

    /// Receive blocks start empty. The first `load` is the only one that
    /// may replay a cached block; later CONTINUATION bytes must decode.
    pristine: bool,

    /// Pseudo headers, these are broken out as they must be sent as part of the
    /// headers frame.
    pseudo: Pseudo,
}

#[derive(Debug)]
struct EncodingHeaderBlock {
    hpack: BytesMut,
}

const END_STREAM: u8 = 0x1;
const END_HEADERS: u8 = 0x4;
const PADDED: u8 = 0x8;
const PRIORITY: u8 = 0x20;
const ALL: u8 = END_STREAM | END_HEADERS | PADDED | PRIORITY;

// ===== impl Headers =====

fn bare_ok_trailer(fields: &HeaderMap) -> bool {
    if fields.len() != 1 {
        return false;
    }
    let Some((name, value)) = fields.iter().next() else {
        return false;
    };
    let bytes = value.as_bytes();
    name == GRPC_STATUS_NAME && bytes.len() == 1 && bytes[0] == b'0'
}

/// Stock trailers, including the empty block left when the map stays parked.
fn trailer_stock(fields: &HeaderMap) -> StandardMap {
    if bare_ok_trailer(fields)
        || (fields.is_empty()
            && crate::h2_backend::header_pool::is_parked(
                crate::h2_backend::header_pool::Slot::Trailers,
            ))
    {
        StandardMap::Trailers
    } else {
        StandardMap::None
    }
}

/// Empty receive block, initialized in the heap allocation.
///
/// A stack `Headers` is 288 bytes. `Box::new` copies that temporary into the
/// box. Each field write is a store into the allocation.
#[allow(
    unsafe_code,
    reason = "initialize an empty Headers directly in its heap allocation"
)]
fn box_empty_headers(
    stream_id: StreamId,
    stream_dep: Option<StreamDependency>,
    flags: HeadersFlag,
) -> Box<Headers> {
    let mut uninit = Box::<Headers>::new_uninit();
    let ptr = uninit.as_mut_ptr();
    // SAFETY: every field is written before `assume_init`. None of the writes
    // can panic, so a partially initialized value is never dropped.
    unsafe {
        std::ptr::addr_of_mut!((*ptr).stream_id).write(stream_id);
        std::ptr::addr_of_mut!((*ptr).stream_dep).write(stream_dep);
        std::ptr::addr_of_mut!((*ptr).flags).write(flags);
        let block = std::ptr::addr_of_mut!((*ptr).header_block);
        std::ptr::addr_of_mut!((*block).fields).write(HeaderMap::new());
        std::ptr::addr_of_mut!((*block).field_size).write(0);
        std::ptr::addr_of_mut!((*block).is_over_size).write(false);
        std::ptr::addr_of_mut!((*block).has_content_length).write(false);
        std::ptr::addr_of_mut!((*block).stock).write(StandardMap::None);
        std::ptr::addr_of_mut!((*block).stock_facts).write(None);
        std::ptr::addr_of_mut!((*block).pristine).write(true);
        let pseudo = std::ptr::addr_of_mut!((*block).pseudo);
        std::ptr::addr_of_mut!((*pseudo).method).write(None);
        std::ptr::addr_of_mut!((*pseudo).scheme).write(None);
        std::ptr::addr_of_mut!((*pseudo).authority).write(None);
        std::ptr::addr_of_mut!((*pseudo).path).write(None);
        std::ptr::addr_of_mut!((*pseudo).protocol).write(None);
        std::ptr::addr_of_mut!((*pseudo).status).write(None);
        uninit.assume_init()
    }
}

/// A built header block, initialized in the heap allocation.
///
/// `Box::new` copies the finished 288-byte frame. The pseudo block and map
/// are moved once, into the allocation.
#[allow(
    unsafe_code,
    reason = "initialize a Headers value directly in its heap allocation"
)]
fn box_filled_headers(
    stream_id: StreamId,
    stream_dep: Option<StreamDependency>,
    flags: HeadersFlag,
    pseudo: Pseudo,
    fields: HeaderMap,
    stock: StandardMap,
    pristine: bool,
) -> Box<Headers> {
    let mut uninit = Box::<Headers>::new_uninit();
    let ptr = uninit.as_mut_ptr();
    // SAFETY: every field is written before `assume_init`. Moving `pseudo`
    // and `fields` in does not panic, so a partial value is never dropped.
    unsafe {
        std::ptr::addr_of_mut!((*ptr).stream_id).write(stream_id);
        std::ptr::addr_of_mut!((*ptr).stream_dep).write(stream_dep);
        std::ptr::addr_of_mut!((*ptr).flags).write(flags);
        let block = std::ptr::addr_of_mut!((*ptr).header_block);
        std::ptr::addr_of_mut!((*block).fields).write(fields);
        std::ptr::addr_of_mut!((*block).field_size).write(0);
        std::ptr::addr_of_mut!((*block).is_over_size).write(false);
        std::ptr::addr_of_mut!((*block).has_content_length).write(false);
        std::ptr::addr_of_mut!((*block).stock).write(stock);
        std::ptr::addr_of_mut!((*block).stock_facts).write(None);
        std::ptr::addr_of_mut!((*block).pristine).write(pristine);
        std::ptr::addr_of_mut!((*block).pseudo).write(pseudo);
        uninit.assume_init()
    }
}

/// POST request frame, initialized in the heap allocation.
///
/// Writing the pseudo block as one value copies 160 bytes into the box.
/// Each field is stored on its own.
#[allow(
    unsafe_code,
    reason = "initialize a POST Headers value directly in its heap allocation"
)]
fn box_post_request(
    stream_id: StreamId,
    flags: HeadersFlag,
    scheme: Option<BytesStr>,
    authority: Option<BytesStr>,
    path: BytesStr,
    fields: HeaderMap,
    stock: StandardMap,
) -> Box<Headers> {
    let mut uninit = Box::<Headers>::new_uninit();
    let ptr = uninit.as_mut_ptr();
    // SAFETY: every field is written before `assume_init`. None of the writes
    // can panic, so a partially initialized value is never dropped.
    unsafe {
        std::ptr::addr_of_mut!((*ptr).stream_id).write(stream_id);
        std::ptr::addr_of_mut!((*ptr).stream_dep).write(None);
        std::ptr::addr_of_mut!((*ptr).flags).write(flags);
        let block = std::ptr::addr_of_mut!((*ptr).header_block);
        std::ptr::addr_of_mut!((*block).fields).write(fields);
        std::ptr::addr_of_mut!((*block).field_size).write(0);
        std::ptr::addr_of_mut!((*block).is_over_size).write(false);
        std::ptr::addr_of_mut!((*block).has_content_length).write(false);
        std::ptr::addr_of_mut!((*block).stock).write(stock);
        std::ptr::addr_of_mut!((*block).stock_facts).write(None);
        std::ptr::addr_of_mut!((*block).pristine).write(false);
        let pseudo = std::ptr::addr_of_mut!((*block).pseudo);
        std::ptr::addr_of_mut!((*pseudo).method).write(Some(Method::POST));
        std::ptr::addr_of_mut!((*pseudo).scheme).write(scheme);
        std::ptr::addr_of_mut!((*pseudo).authority).write(authority);
        std::ptr::addr_of_mut!((*pseudo).path).write(Some(path));
        std::ptr::addr_of_mut!((*pseudo).protocol).write(None);
        std::ptr::addr_of_mut!((*pseudo).status).write(None);
        uninit.assume_init()
    }
}

/// A trailer block, initialized in the heap allocation.
///
/// The pseudo-header set is empty. Writing those `None`s in place avoids
/// building a 288-byte `Headers` and copying it into the box.
#[allow(
    unsafe_code,
    reason = "initialize a trailer Headers value directly in its heap allocation"
)]
fn box_trailer_headers(
    stream_id: StreamId,
    flags: HeadersFlag,
    fields: HeaderMap,
    stock: StandardMap,
) -> Box<Headers> {
    let mut uninit = Box::<Headers>::new_uninit();
    let ptr = uninit.as_mut_ptr();
    // SAFETY: every field is written before `assume_init`. Moving `fields`
    // in does not panic, so a partial value is never dropped.
    unsafe {
        std::ptr::addr_of_mut!((*ptr).stream_id).write(stream_id);
        std::ptr::addr_of_mut!((*ptr).stream_dep).write(None);
        std::ptr::addr_of_mut!((*ptr).flags).write(flags);
        let block = std::ptr::addr_of_mut!((*ptr).header_block);
        std::ptr::addr_of_mut!((*block).fields).write(fields);
        std::ptr::addr_of_mut!((*block).field_size).write(0);
        std::ptr::addr_of_mut!((*block).is_over_size).write(false);
        std::ptr::addr_of_mut!((*block).has_content_length).write(false);
        std::ptr::addr_of_mut!((*block).stock).write(stock);
        std::ptr::addr_of_mut!((*block).stock_facts).write(None);
        std::ptr::addr_of_mut!((*block).pristine).write(false);
        let pseudo = std::ptr::addr_of_mut!((*block).pseudo);
        std::ptr::addr_of_mut!((*pseudo).method).write(None);
        std::ptr::addr_of_mut!((*pseudo).scheme).write(None);
        std::ptr::addr_of_mut!((*pseudo).authority).write(None);
        std::ptr::addr_of_mut!((*pseudo).path).write(None);
        std::ptr::addr_of_mut!((*pseudo).protocol).write(None);
        std::ptr::addr_of_mut!((*pseudo).status).write(None);
        uninit.assume_init()
    }
}

/// A response block with only `:status` set, initialized in the heap allocation.
///
/// `Pseudo::response` is a 160-byte value that is mostly empty. Storing the
/// status directly avoids copying that temporary into the box.
#[allow(
    unsafe_code,
    reason = "initialize a response Headers value directly in its heap allocation"
)]
fn box_response_headers(
    stream_id: StreamId,
    status: http::StatusCode,
    fields: HeaderMap,
    flags: HeadersFlag,
) -> Box<Headers> {
    let mut uninit = Box::<Headers>::new_uninit();
    let ptr = uninit.as_mut_ptr();
    // SAFETY: every field is written before `assume_init`. Moving `fields`
    // in does not panic, so a partial value is never dropped.
    unsafe {
        std::ptr::addr_of_mut!((*ptr).stream_id).write(stream_id);
        std::ptr::addr_of_mut!((*ptr).stream_dep).write(None);
        std::ptr::addr_of_mut!((*ptr).flags).write(flags);
        let block = std::ptr::addr_of_mut!((*ptr).header_block);
        std::ptr::addr_of_mut!((*block).fields).write(fields);
        std::ptr::addr_of_mut!((*block).field_size).write(0);
        std::ptr::addr_of_mut!((*block).is_over_size).write(false);
        std::ptr::addr_of_mut!((*block).has_content_length).write(false);
        std::ptr::addr_of_mut!((*block).stock).write(StandardMap::None);
        std::ptr::addr_of_mut!((*block).stock_facts).write(None);
        std::ptr::addr_of_mut!((*block).pristine).write(false);
        let pseudo = std::ptr::addr_of_mut!((*block).pseudo);
        std::ptr::addr_of_mut!((*pseudo).method).write(None);
        std::ptr::addr_of_mut!((*pseudo).scheme).write(None);
        std::ptr::addr_of_mut!((*pseudo).authority).write(None);
        std::ptr::addr_of_mut!((*pseudo).path).write(None);
        std::ptr::addr_of_mut!((*pseudo).protocol).write(None);
        std::ptr::addr_of_mut!((*pseudo).status).write(Some(status));
        uninit.assume_init()
    }
}

impl Headers {
    /// Create a new HEADERS frame
    pub fn new(stream_id: StreamId, pseudo: Pseudo, fields: HeaderMap) -> Self {
        Headers {
            stream_id,
            stream_dep: None,
            header_block: HeaderBlock {
                // Encode never reads this. The receive path fills it while
                // decoding, which is the only place the size limit is enforced.
                field_size: 0,
                fields,
                is_over_size: false,
                has_content_length: false,
                stock: StandardMap::None,
                stock_facts: None,
                pristine: false,
                pseudo,
            },
            flags: HeadersFlag::default(),
        }
    }

    /// POST request with no `:protocol`, written straight into the returned frame.
    ///
    /// `Pseudo::request_post` returned a 160-byte value that the client then
    /// moved into `Headers::new` and again into the `Result`. Building the
    /// frame here lets those fields land in the caller's return slot.
    ///
    /// The caller has already rejected HTTP/2 when both scheme and authority
    /// are missing. A relative HTTP/1 URI still gets scheme `http`.
    #[inline(always)]
    pub(crate) fn post_request(
        stream_id: StreamId,
        uri: Uri,
        fields: HeaderMap,
        end_of_stream: bool,
        stock_request: bool,
    ) -> Self {
        let has_authority = uri.authority().is_some();
        let path = if let Some(v) = uri.path_and_query() {
            let raw = v.as_str();
            if raw.is_empty() {
                Pseudo::post_empty_path()
            } else {
                BytesStr::reused(crate::h2_backend::hpack::header::ReusedStr::Path, raw)
            }
        } else {
            Pseudo::post_empty_path()
        };
        let scheme = match uri.scheme() {
            Some(scheme) if *scheme == uri::Scheme::HTTP => Some(BytesStr::from_static("http")),
            Some(scheme) => Some(Pseudo::post_scheme(scheme.clone())),
            None if !has_authority => Some(BytesStr::from_static("http")),
            None => None,
        };
        let authority = if has_authority {
            uri.authority().map(|authority| {
                BytesStr::reused(
                    crate::h2_backend::hpack::header::ReusedStr::Authority,
                    authority.as_str(),
                )
            })
        } else {
            None
        };
        let mut flags = HeadersFlag::default();
        if end_of_stream {
            flags.set_end_stream();
        }
        Headers {
            stream_id,
            stream_dep: None,
            header_block: HeaderBlock {
                field_size: 0,
                fields,
                is_over_size: false,
                has_content_length: false,
                stock: if stock_request {
                    StandardMap::Request
                } else {
                    StandardMap::None
                },
                stock_facts: None,
                pristine: false,
                pseudo: Pseudo {
                    method: Some(Method::POST),
                    scheme,
                    authority,
                    path: Some(path),
                    protocol: None,
                    status: None,
                },
            },
            flags,
        }
    }

    /// [`post_request`](Self::post_request) already in the queue box.
    ///
    /// `send_headers` boxes the returned frame and copies all 288 bytes.
    /// The fields are stored into the allocation instead.
    pub(crate) fn post_request_boxed(
        stream_id: StreamId,
        uri: Uri,
        fields: HeaderMap,
        end_of_stream: bool,
        stock_request: bool,
    ) -> Box<Self> {
        let has_authority = uri.authority().is_some();
        let path = if let Some(v) = uri.path_and_query() {
            let raw = v.as_str();
            if raw.is_empty() {
                Pseudo::post_empty_path()
            } else {
                BytesStr::reused(crate::h2_backend::hpack::header::ReusedStr::Path, raw)
            }
        } else {
            Pseudo::post_empty_path()
        };
        let scheme = match uri.scheme() {
            Some(scheme) if *scheme == uri::Scheme::HTTP => Some(BytesStr::from_static("http")),
            Some(scheme) => Some(Pseudo::post_scheme(scheme.clone())),
            None if !has_authority => Some(BytesStr::from_static("http")),
            None => None,
        };
        let authority = if has_authority {
            uri.authority().map(|authority| {
                BytesStr::reused(
                    crate::h2_backend::hpack::header::ReusedStr::Authority,
                    authority.as_str(),
                )
            })
        } else {
            None
        };
        let mut flags = HeadersFlag::default();
        if end_of_stream {
            flags.set_end_stream();
        }
        let stock = if stock_request {
            StandardMap::Request
        } else {
            StandardMap::None
        };
        box_post_request(stream_id, flags, scheme, authority, path, fields, stock)
    }

    /// [`new`](Self::new) with the frame already in its queue box.
    ///
    /// Returning `Headers` and boxing at `send_headers` copies the 288-byte
    /// frame. The response path writes it into the allocation.
    pub fn new_boxed(stream_id: StreamId, pseudo: Pseudo, fields: HeaderMap) -> Box<Self> {
        box_filled_headers(
            stream_id,
            None,
            HeadersFlag::default(),
            pseudo,
            fields,
            StandardMap::None,
            false,
        )
    }

    /// Response HEADERS with only `:status`, already in the queue box.
    pub fn response_boxed(
        stream_id: StreamId,
        status: http::StatusCode,
        fields: HeaderMap,
    ) -> Box<Self> {
        box_response_headers(stream_id, status, fields, HeadersFlag::default())
    }

    pub fn trailers(stream_id: StreamId, fields: HeaderMap) -> Self {
        let mut flags = HeadersFlag::default();
        flags.set_end_stream();
        // A bare OK trailer is the block we replay. Marking it here skips
        // the field walk at encode time.
        let stock = trailer_stock(&fields);

        Headers {
            stream_id,
            stream_dep: None,
            header_block: HeaderBlock {
                field_size: 0,
                fields,
                is_over_size: false,
                has_content_length: false,
                stock,
                stock_facts: None,
                pristine: false,
                pseudo: Pseudo::default(),
            },
            flags,
        }
    }

    /// [`trailers`](Self::trailers) already in the queue box.
    pub fn trailers_boxed(stream_id: StreamId, fields: HeaderMap) -> Box<Self> {
        let mut flags = HeadersFlag::default();
        flags.set_end_stream();
        let stock = trailer_stock(&fields);
        box_trailer_headers(stream_id, flags, fields, stock)
    }

    /// Loads the header frame but doesn't actually do HPACK decoding.
    ///
    /// HPACK decoding is done in the `load_hpack` step.
    pub fn load(head: Head, mut src: BytesMut) -> Result<(Box<Self>, BytesMut), Error> {
        let flags = HeadersFlag(head.flag());
        let mut pad = 0;

        if head.stream_id().is_zero() {
            return Err(Error::InvalidStreamId);
        }

        // Read the padding length
        if flags.is_padded() {
            if src.is_empty() {
                return Err(Error::MalformedMessage);
            }
            pad = src[0] as usize;

            // Drop the padding
            src.advance(1);
        }

        // Read the stream dependency
        let stream_dep = if flags.is_priority() {
            if src.len() < 5 {
                return Err(Error::MalformedMessage);
            }
            let stream_dep = StreamDependency::load(&src[..5])?;

            if stream_dep.dependency_id() == head.stream_id() {
                return Err(Error::InvalidDependencyId);
            }

            // Drop the next 5 bytes
            src.advance(5);

            Some(stream_dep)
        } else {
            None
        };

        if pad > 0 {
            if pad > src.len() {
                return Err(Error::TooMuchPadding);
            }

            let len = src.len() - pad;
            src.truncate(len);
        }

        // `Box::new` of this value still materializes the 288-byte block on
        // the stack and copies it into the allocation. Write the fields into
        // the heap slot instead.
        let headers = box_empty_headers(head.stream_id(), stream_dep, flags);

        Ok((headers, src))
    }

    pub fn load_hpack(
        &mut self,
        src: &mut BytesMut,
        max_header_list_size: usize,
        decoder: &mut hpack::Decoder,
    ) -> Result<(), Error> {
        self.header_block.load(src, max_header_list_size, decoder)
    }

    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    pub fn is_end_headers(&self) -> bool {
        self.flags.is_end_headers()
    }

    pub fn set_end_headers(&mut self) {
        self.flags.set_end_headers();
    }

    pub fn is_end_stream(&self) -> bool {
        self.flags.is_end_stream()
    }

    pub fn set_end_stream(&mut self) {
        self.flags.set_end_stream()
    }

    pub fn is_over_size(&self) -> bool {
        self.header_block.is_over_size
    }

    /// `content-length` when the decoded block contains one.
    ///
    /// Absent on every stock gRPC block, so the receive path does not hash
    /// the name on that miss.
    pub fn content_length(&self) -> Option<&HeaderValue> {
        if self.header_block.has_content_length {
            self.header_block.fields.get(header::CONTENT_LENGTH)
        } else {
            None
        }
    }

    /// The gRPC layer already classified this block.
    #[inline]
    pub(crate) fn is_known_stock(&self) -> bool {
        !matches!(self.header_block.stock, StandardMap::None)
    }

    /// Remember a stock field set. `true` means those names are legal on the
    /// wire, so `Send::check_headers` can skip its walk.
    ///
    /// Stock blocks return from [`Self::is_known_stock`] and never enter here.
    #[cold]
    #[inline(never)]
    pub(crate) fn note_stock_fields(&mut self) -> bool {
        // The gRPC layer built this block from the stock pool and marked it.
        // The names and values are the ones `fields_match` would accept.
        if self.header_block.stock != StandardMap::None {
            return true;
        }
        let kind = match self.header_block.fields.len() {
            4 if fields_match(&self.header_block.fields, REQUEST_BITS, 4) => StandardMap::Request,
            2 if fields_match(&self.header_block.fields, RESPONSE_BITS, 2) => {
                StandardMap::Response
            }
            1 if fields_match(&self.header_block.fields, STATUS, 1) => StandardMap::Trailers,
            _ => return false,
        };
        self.header_block.stock = kind;
        true
    }

    pub(crate) fn preset_stock_request(&mut self) {
        self.header_block.stock = StandardMap::Request;
    }

    pub(crate) fn preset_stock_response(&mut self) {
        self.header_block.stock = StandardMap::Response;
    }

    pub(crate) fn stock_facts(&self) -> Option<crate::wire::headers::StockFacts> {
        self.header_block.stock_facts
    }

    pub fn into_parts(self) -> (Pseudo, HeaderMap) {
        (self.header_block.pseudo, self.header_block.fields)
    }

    /// Move the pseudo-headers and the map out of a boxed frame.
    ///
    /// Dereferencing the box copies all 288 bytes onto the stack. The only
    /// parts with destructors are the two values returned here.
    #[allow(
        unsafe_code,
        reason = "move two fields out of a boxed header block without copying the rest"
    )]
    pub(crate) fn into_parts_box(this: Box<Self>) -> (Pseudo, HeaderMap) {
        const _: () = {
            assert!(!std::mem::needs_drop::<StreamId>());
            assert!(!std::mem::needs_drop::<Option<StreamDependency>>());
            assert!(!std::mem::needs_drop::<HeadersFlag>());
            assert!(!std::mem::needs_drop::<StandardMap>());
            assert!(!std::mem::needs_drop::<Option<crate::wire::headers::StockFacts>>());
        };
        let ptr = Box::into_raw(this);
        // SAFETY: `ptr` came from `Box::into_raw`. `pseudo` and `fields` are
        // moved to the caller. Every other field has no destructor, which
        // the asserts above lock in.
        unsafe {
            let pseudo = std::ptr::read(std::ptr::addr_of!((*ptr).header_block.pseudo));
            let fields = std::ptr::read(std::ptr::addr_of!((*ptr).header_block.fields));
            std::alloc::dealloc(ptr.cast(), std::alloc::Layout::new::<Self>());
            (pseudo, fields)
        }
    }

    #[cfg(any())]
    pub fn pseudo_mut(&mut self) -> &mut Pseudo {
        &mut self.header_block.pseudo
    }

    pub(crate) fn pseudo(&self) -> &Pseudo {
        &self.header_block.pseudo
    }

    /// Whether it has status 1xx
    pub(crate) fn is_informational(&self) -> bool {
        self.header_block.pseudo.is_informational()
    }

    pub fn fields(&self) -> &HeaderMap {
        &self.header_block.fields
    }

    pub fn into_fields(self) -> HeaderMap {
        self.header_block.fields
    }

    pub fn encode(
        mut self,
        encoder: &mut hpack::Encoder,
        dst: &mut EncodeBuf<'_>,
    ) -> Option<Continuation> {
        // At this point, the `is_end_headers` flag should always be set
        debug_assert!(self.flags.is_end_headers());

        // Get the HEADERS frame head
        let head = self.head();

        // A repeated stock block is already encoded. Write those bytes into
        // the frame instead of copying them into the scratch buffer first.
        if !encoder.has_size_update() {
            if let Some((kind, bytes)) = replayed_standard(
                encoder,
                &self.header_block.pseudo,
                &self.header_block.fields,
                self.header_block.stock,
            ) {
                // `BytesMut::remaining_mut` is `isize::MAX - len`, so the
                // limit's remaining budget is just `limit`. The write still
                // reserves when the buffer itself is short.
                let total = bytes.len() + head.encode_len();
                let lim = bytes::buf::Limit::limit(dst);
                if total <= lim {
                    {
                        let inner = bytes::buf::Limit::get_mut(dst);
                        encode_head_into(head, bytes.len(), inner);
                        inner.extend_from_slice(bytes);
                    }
                    bytes::buf::Limit::set_limit(dst, lim - total);
                    // An empty block was sent because the stock map is already
                    // parked. Recycling it would replace that map with nothing.
                    if !self.header_block.fields.is_empty() {
                        if let Some(slot) = kind.slot() {
                            crate::h2_backend::header_pool::recycle(
                                slot,
                                std::mem::take(&mut self.header_block.fields),
                            );
                        }
                    }
                    // Stock responses and trailers own no pseudo strings. Skipping
                    // the drop avoids five `None` checks on every replayed block.
                    if matches!(kind, StandardMap::Response | StandardMap::Trailers) {
                        debug_assert!(
                            self.header_block.pseudo.method.is_none()
                                && self.header_block.pseudo.scheme.is_none()
                                && self.header_block.pseudo.authority.is_none()
                                && self.header_block.pseudo.path.is_none()
                                && self.header_block.pseudo.protocol.is_none()
                        );
                        std::mem::forget(self.header_block.pseudo);
                    }
                    return None;
                }
            }
        }

        if self.header_block.fields.is_empty() {
            if let Some(slot) = self.header_block.stock.slot() {
                if crate::h2_backend::header_pool::is_parked(slot) {
                    self.header_block.fields =
                        crate::h2_backend::header_pool::take(slot, HeaderMap::new);
                }
            }
        }
        self.encode_unreplayed(encoder, dst, head)
    }

    /// Encode a boxed block without copying it onto the stack.
    ///
    /// The stock replay path only needs the head and the saved bytes. Moving
    /// the 288-byte frame out of the box first copies fields this path does
    /// not read.
    #[allow(
        unsafe_code,
        reason = "recycle the map out of a boxed header block without copying the frame"
    )]
    pub(crate) fn encode_box(
        mut this: Box<Self>,
        encoder: &mut hpack::Encoder,
        dst: &mut EncodeBuf<'_>,
    ) -> Option<Continuation> {
        debug_assert!(this.flags.is_end_headers());
        let head = this.head();
        if !encoder.has_size_update() {
            if let Some((kind, bytes)) = replayed_standard(
                encoder,
                &this.header_block.pseudo,
                &this.header_block.fields,
                this.header_block.stock,
            ) {
                let total = bytes.len() + head.encode_len();
                let lim = bytes::buf::Limit::limit(dst);
                if total <= lim {
                    {
                        let inner = bytes::buf::Limit::get_mut(dst);
                        encode_head_into(head, bytes.len(), inner);
                        inner.extend_from_slice(bytes);
                    }
                    bytes::buf::Limit::set_limit(dst, lim - total);
                    let ptr = Box::into_raw(this);
                    // SAFETY: same leftover fields as `into_parts_box`. A parked
                    // stock map stays in its slot, so an empty frame map is not
                    // written back. Response and trailer pseudos are empty and
                    // intentionally not dropped.
                    unsafe {
                        let fields_ptr = std::ptr::addr_of_mut!((*ptr).header_block.fields);
                        if (*fields_ptr).is_empty() {
                            if (*fields_ptr).capacity() != 0 {
                                std::ptr::drop_in_place(fields_ptr);
                            }
                        } else if let Some(slot) = kind.slot() {
                            let fields = std::ptr::read(fields_ptr);
                            crate::h2_backend::header_pool::recycle(slot, fields);
                        } else {
                            std::ptr::drop_in_place(fields_ptr);
                        }
                        let pseudo_ptr = std::ptr::addr_of_mut!((*ptr).header_block.pseudo);
                        if !matches!(kind, StandardMap::Response | StandardMap::Trailers) {
                            // Request pseudos own the authority and path. Drop
                            // them where they sit instead of copying the 152-byte
                            // block out and dropping the copy.
                            std::ptr::drop_in_place(pseudo_ptr);
                        }
                        std::alloc::dealloc(ptr.cast(), std::alloc::Layout::new::<Self>());
                    }
                    return None;
                }
            }
        }
        if this.header_block.fields.is_empty() {
            if let Some(slot) = this.header_block.stock.slot() {
                if crate::h2_backend::header_pool::is_parked(slot) {
                    this.header_block.fields =
                        crate::h2_backend::header_pool::take(slot, HeaderMap::new);
                }
            }
        }
        (*this).encode_unreplayed(encoder, dst, head)
    }

    /// A block that did not replay. Out of line so the hit path does not
    /// reserve a copy of the header block.
    #[inline(never)]
    fn encode_unreplayed(
        self,
        encoder: &mut hpack::Encoder,
        dst: &mut EncodeBuf<'_>,
        head: Head,
    ) -> Option<Continuation> {
        self.header_block
            .into_encoding(encoder)
            .encode(&head, dst, Some(encoder), 0, |_| {})
    }

    fn head(&self) -> Head {
        Head::new(Kind::Headers, self.flags.into(), self.stream_id)
    }
}

impl<T> From<Headers> for Frame<T> {
    fn from(src: Headers) -> Self {
        Frame::Headers(Box::new(src))
    }
}

impl fmt::Debug for Headers {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut builder = f.debug_struct("Headers");
        builder
            .field("stream_id", &self.stream_id)
            .field("flags", &self.flags);

        if let Some(ref protocol) = self.header_block.pseudo.protocol {
            builder.field("protocol", protocol);
        }

        if let Some(ref dep) = self.stream_dep {
            builder.field("stream_dep", dep);
        }

        // `fields` and `pseudo` purposefully not included
        builder.finish()
    }
}

// ===== util =====

#[derive(Debug, PartialEq, Eq)]
pub struct ParseU64Error;

pub fn parse_u64(src: &[u8]) -> Result<u64, ParseU64Error> {
    if src.len() > 19 {
        // At danger for overflow...
        return Err(ParseU64Error);
    }

    let mut ret = 0;

    for &d in src {
        if d < b'0' || d > b'9' {
            return Err(ParseU64Error);
        }

        ret *= 10;
        ret += (d - b'0') as u64;
    }

    Ok(ret)
}

// ===== impl PushPromise =====

#[derive(Debug)]
pub enum PushPromiseHeaderError {
    InvalidContentLength(Result<u64, ParseU64Error>),
    NotSafeAndCacheable,
}

impl PushPromise {
    pub fn new(
        stream_id: StreamId,
        promised_id: StreamId,
        pseudo: Pseudo,
        fields: HeaderMap,
    ) -> Self {
        PushPromise {
            flags: PushPromiseFlag::default(),
            header_block: HeaderBlock {
                field_size: 0,
                fields,
                is_over_size: false,
                has_content_length: false,
                stock: StandardMap::None,
                stock_facts: None,
                pristine: false,
                pseudo,
            },
            promised_id,
            stream_id,
        }
    }

    pub fn validate_request(req: &Request<()>) -> Result<(), PushPromiseHeaderError> {
        use PushPromiseHeaderError::*;
        // The spec has some requirements for promised request headers
        // [https://httpwg.org/specs/rfc7540.html#PushRequests]

        // A promised request "that indicates the presence of a request body
        // MUST reset the promised stream with a stream error"
        if let Some(content_length) = req.headers().get(header::CONTENT_LENGTH) {
            let parsed_length = parse_u64(content_length.as_bytes());
            if parsed_length != Ok(0) {
                return Err(InvalidContentLength(parsed_length));
            }
        }
        // "The server MUST include a method in the :method pseudo-header field
        // that is safe and cacheable"
        if !Self::safe_and_cacheable(req.method()) {
            return Err(NotSafeAndCacheable);
        }

        Ok(())
    }

    fn safe_and_cacheable(method: &Method) -> bool {
        // Cacheable: https://httpwg.org/specs/rfc7231.html#cacheable.methods
        // Safe: https://httpwg.org/specs/rfc7231.html#safe.methods
        method == Method::GET || method == Method::HEAD
    }

    pub fn fields(&self) -> &HeaderMap {
        &self.header_block.fields
    }

    #[cfg(any())]
    pub fn into_fields(self) -> HeaderMap {
        self.header_block.fields
    }

    /// Loads the push promise frame but doesn't actually do HPACK decoding.
    ///
    /// HPACK decoding is done in the `load_hpack` step.
    pub fn load(head: Head, mut src: BytesMut) -> Result<(Self, BytesMut), Error> {
        let flags = PushPromiseFlag(head.flag());
        let mut pad = 0;

        if head.stream_id().is_zero() {
            return Err(Error::InvalidStreamId);
        }

        // Read the padding length
        if flags.is_padded() {
            if src.is_empty() {
                return Err(Error::MalformedMessage);
            }

            // TODO: Ensure payload is sized correctly
            pad = src[0] as usize;

            // Drop the padding
            src.advance(1);
        }

        if src.len() < 5 {
            return Err(Error::MalformedMessage);
        }

        let (promised_id, _) = StreamId::parse(&src[..4]);
        // Drop promised_id bytes
        src.advance(4);

        if pad > 0 {
            if pad > src.len() {
                return Err(Error::TooMuchPadding);
            }

            let len = src.len() - pad;
            src.truncate(len);
        }

        let frame = PushPromise {
            flags,
            header_block: HeaderBlock {
                fields: HeaderMap::new(),
                field_size: 0,
                is_over_size: false,
                has_content_length: false,
                stock: StandardMap::None,
                stock_facts: None,
                pristine: true,
                pseudo: Pseudo::default(),
            },
            promised_id,
            stream_id: head.stream_id(),
        };
        Ok((frame, src))
    }

    pub fn load_hpack(
        &mut self,
        src: &mut BytesMut,
        max_header_list_size: usize,
        decoder: &mut hpack::Decoder,
    ) -> Result<(), Error> {
        self.header_block.load(src, max_header_list_size, decoder)
    }

    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    pub fn promised_id(&self) -> StreamId {
        self.promised_id
    }

    pub fn is_end_headers(&self) -> bool {
        self.flags.is_end_headers()
    }

    pub fn set_end_headers(&mut self) {
        self.flags.set_end_headers();
    }

    pub fn is_over_size(&self) -> bool {
        self.header_block.is_over_size
    }

    pub fn encode(
        self,
        encoder: &mut hpack::Encoder,
        dst: &mut EncodeBuf<'_>,
    ) -> Option<Continuation> {
        // At this point, the `is_end_headers` flag should always be set
        debug_assert!(self.flags.is_end_headers());

        let head = self.head();
        let promised_id = self.promised_id;

        self.header_block
            .into_encoding(encoder)
            .encode(&head, dst, Some(encoder), 4, |dst| {
                dst.put_u32(promised_id.into());
            })
    }

    fn head(&self) -> Head {
        Head::new(Kind::PushPromise, self.flags.into(), self.stream_id)
    }

    /// Consume `self`, returning the parts of the frame
    pub fn into_parts(self) -> (Pseudo, HeaderMap) {
        (self.header_block.pseudo, self.header_block.fields)
    }
}

impl<T> From<PushPromise> for Frame<T> {
    fn from(src: PushPromise) -> Self {
        Frame::PushPromise(Box::new(src))
    }
}

impl fmt::Debug for PushPromise {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PushPromise")
            .field("stream_id", &self.stream_id)
            .field("promised_id", &self.promised_id)
            .field("flags", &self.flags)
            // `fields` and `pseudo` purposefully not included
            .finish()
    }
}

// ===== impl Continuation =====

impl Continuation {
    fn head(&self) -> Head {
        Head::new(Kind::Continuation, END_HEADERS, self.stream_id)
    }

    pub fn encode(self, dst: &mut EncodeBuf<'_>) -> Option<Continuation> {
        // Get the CONTINUATION frame head
        let head = self.head();

        self.header_block.encode(&head, dst, None, 0, |_| {})
    }
}

// ===== impl Pseudo =====

impl Pseudo {
    #[inline(never)]
    pub fn request(method: Method, uri: Uri, protocol: Option<Protocol>) -> Self {
        // CONNECT and OPTIONS are rare. The POST path is inlined at the
        // client so this function stays the cold prologue.
        if protocol.is_some() || method != Method::POST {
            return Self::request_general(method, uri, protocol);
        }
        Self::request_post(uri)
    }

    /// POST with no `:protocol`. Inlined into the client request builder so
    /// the 160-byte value is created where `Headers::new` consumes it.
    #[inline(always)]
    pub(crate) fn request_post(uri: Uri) -> Self {
        // Borrow the URI. `Parts::from` moved the path and authority out and
        // dropped them after the reused `Bytes` was cloned.
        let path = if let Some(v) = uri.path_and_query() {
            let raw = v.as_str();
            if raw.is_empty() {
                Self::post_empty_path()
            } else {
                BytesStr::reused(crate::h2_backend::hpack::header::ReusedStr::Path, raw)
            }
        } else {
            Self::post_empty_path()
        };
        let scheme = match uri.scheme() {
            Some(scheme) if *scheme == uri::Scheme::HTTP => Some(BytesStr::from_static("http")),
            Some(scheme) => Some(Self::post_scheme(scheme.clone())),
            None => None,
        };
        let mut pseudo = Pseudo {
            method: Some(Method::POST),
            scheme,
            authority: None,
            path: Some(path),
            protocol: None,
            status: None,
        };
        if let Some(authority) = uri.authority() {
            pseudo.set_authority(BytesStr::reused(
                crate::h2_backend::hpack::header::ReusedStr::Authority,
                authority.as_str(),
            ));
        }
        pseudo
    }

    #[cold]
    #[inline(never)]
    fn post_empty_path() -> BytesStr {
        BytesStr::from_static("/")
    }

    #[cold]
    #[inline(never)]
    fn post_scheme(scheme: uri::Scheme) -> BytesStr {
        if scheme == uri::Scheme::HTTPS {
            BytesStr::from_static("https")
        } else {
            BytesStr::from(scheme.as_str())
        }
    }

    #[cold]
    #[inline(never)]
    fn request_general(method: Method, uri: Uri, protocol: Option<Protocol>) -> Self {
        let parts = uri::Parts::from(uri);

        let (scheme, path) = if method == Method::CONNECT && protocol.is_none() {
            (None, None)
        } else {
            let path = parts
                .path_and_query
                .map(|v| {
                    BytesStr::reused(
                        crate::h2_backend::hpack::header::ReusedStr::Path,
                        v.as_str(),
                    )
                })
                .unwrap_or(BytesStr::from_static(""));

            let path = if !path.is_empty() {
                path
            } else if method == Method::OPTIONS {
                BytesStr::from_static("*")
            } else {
                BytesStr::from_static("/")
            };

            (parts.scheme, Some(path))
        };

        let mut pseudo = Pseudo {
            method: Some(method),
            scheme: None,
            authority: None,
            path,
            protocol,
            status: None,
        };

        if let Some(scheme) = scheme {
            pseudo.set_scheme(scheme);
        }

        if let Some(authority) = parts.authority {
            pseudo.set_authority(BytesStr::reused(
                crate::h2_backend::hpack::header::ReusedStr::Authority,
                authority.as_str(),
            ));
        }

        pseudo
    }

    pub fn response(status: StatusCode) -> Self {
        Pseudo {
            method: None,
            scheme: None,
            authority: None,
            path: None,
            protocol: None,
            status: Some(status),
        }
    }

    #[cfg(any())]
    pub fn set_status(&mut self, value: StatusCode) {
        self.status = Some(value);
    }

    pub fn set_scheme(&mut self, scheme: uri::Scheme) {
        let bytes_str = match scheme.as_str() {
            "http" => BytesStr::from_static("http"),
            "https" => BytesStr::from_static("https"),
            s => BytesStr::from(s),
        };
        self.scheme = Some(bytes_str);
    }

    #[cfg(any())]
    pub fn set_protocol(&mut self, protocol: Protocol) {
        self.protocol = Some(protocol);
    }

    pub fn set_authority(&mut self, authority: BytesStr) {
        self.authority = Some(authority);
    }

    /// Whether it has status 1xx
    pub(crate) fn is_informational(&self) -> bool {
        self.status
            .map_or(false, |status| status.is_informational())
    }
}

// ===== impl EncodingHeaderBlock =====

impl EncodingHeaderBlock {
    fn encode<F>(
        mut self,
        head: &Head,
        dst: &mut EncodeBuf<'_>,
        encoder: Option<&mut hpack::Encoder>,
        prefix_len: usize,
        f: F,
    ) -> Option<Continuation>
    where
        F: FnOnce(&mut EncodeBuf<'_>),
    {
        let payload_len = self.hpack.len() + prefix_len;
        // A block that fits in this frame can carry its real length in the
        // head. The zero-length head plus a later patch is the continuation
        // path, used when the block is larger than the frame.
        if payload_len + head.encode_len() <= dst.remaining_mut() {
            head.encode(payload_len, dst);
            f(dst);
            dst.put_slice(&self.hpack);
            if let Some(encoder) = encoder {
                encoder.return_scratch(self.hpack);
            }
            return None;
        }

        let head_pos = dst.get_ref().len();

        // The block does not fit. Write the head with length 0, then the
        // body that fits, and patch the length once that prefix is known.
        head.encode(0, dst);

        let payload_pos = dst.get_ref().len();

        f(dst);

        // Now, encode the header payload
        let continuation = if self.hpack.len() > dst.remaining_mut() {
            let head_part = self.hpack.split_to(dst.remaining_mut());
            dst.put_slice(&head_part);

            Some(Continuation {
                stream_id: head.stream_id(),
                header_block: self,
            })
        } else {
            dst.put_slice(&self.hpack);
            // The block is fully written, so the buffer can be reused by the
            // next frame on this connection.
            if let Some(encoder) = encoder {
                encoder.return_scratch(self.hpack);
            }

            None
        };

        // Compute the header block length
        let payload_len = (dst.get_ref().len() - payload_pos) as u64;

        // Write the frame length
        let payload_len_be = payload_len.to_be_bytes();
        assert!(payload_len_be[0..5].iter().all(|b| *b == 0));
        (dst.get_mut()[head_pos..head_pos + 3]).copy_from_slice(&payload_len_be[5..]);

        if continuation.is_some() {
            // There will be continuation frames, so the `is_end_headers` flag
            // must be unset
            debug_assert!(dst.get_ref()[head_pos + 4] & END_HEADERS == END_HEADERS);

            dst.get_mut()[head_pos + 4] -= END_HEADERS;
        }

        continuation
    }
}

// ===== impl Iter =====

impl Iterator for Iter {
    type Item = hpack::Header<Option<HeaderName>>;

    fn next(&mut self) -> Option<Self::Item> {
        use crate::h2_backend::hpack::Header::*;

        if let Some(ref mut pseudo) = self.pseudo {
            if let Some(method) = pseudo.method.take() {
                return Some(Method(method));
            }

            if let Some(scheme) = pseudo.scheme.take() {
                return Some(Scheme(scheme));
            }

            if let Some(authority) = pseudo.authority.take() {
                return Some(Authority(authority));
            }

            if let Some(path) = pseudo.path.take() {
                return Some(Path(path));
            }

            if let Some(protocol) = pseudo.protocol.take() {
                return Some(Protocol(protocol));
            }

            if let Some(status) = pseudo.status.take() {
                return Some(Status(status));
            }
        }

        self.pseudo = None;

        self.fields
            .next()
            .map(|(name, value)| Field { name, value })
    }
}

// ===== impl HeadersFlag =====

impl HeadersFlag {
    pub fn empty() -> HeadersFlag {
        HeadersFlag(0)
    }

    pub fn load(bits: u8) -> HeadersFlag {
        HeadersFlag(bits & ALL)
    }

    pub fn is_end_stream(&self) -> bool {
        self.0 & END_STREAM == END_STREAM
    }

    pub fn set_end_stream(&mut self) {
        self.0 |= END_STREAM;
    }

    pub fn is_end_headers(&self) -> bool {
        self.0 & END_HEADERS == END_HEADERS
    }

    pub fn set_end_headers(&mut self) {
        self.0 |= END_HEADERS;
    }

    pub fn is_padded(&self) -> bool {
        self.0 & PADDED == PADDED
    }

    pub fn is_priority(&self) -> bool {
        self.0 & PRIORITY == PRIORITY
    }
}

impl Default for HeadersFlag {
    /// Returns a `HeadersFlag` value with `END_HEADERS` set.
    fn default() -> Self {
        HeadersFlag(END_HEADERS)
    }
}

impl From<HeadersFlag> for u8 {
    fn from(src: HeadersFlag) -> u8 {
        src.0
    }
}

impl fmt::Debug for HeadersFlag {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        util::debug_flags(fmt, self.0)
            .flag_if(self.is_end_headers(), "END_HEADERS")
            .flag_if(self.is_end_stream(), "END_STREAM")
            .flag_if(self.is_padded(), "PADDED")
            .flag_if(self.is_priority(), "PRIORITY")
            .finish()
    }
}

// ===== impl PushPromiseFlag =====

impl PushPromiseFlag {
    pub fn empty() -> PushPromiseFlag {
        PushPromiseFlag(0)
    }

    pub fn load(bits: u8) -> PushPromiseFlag {
        PushPromiseFlag(bits & ALL)
    }

    pub fn is_end_headers(&self) -> bool {
        self.0 & END_HEADERS == END_HEADERS
    }

    pub fn set_end_headers(&mut self) {
        self.0 |= END_HEADERS;
    }

    pub fn is_padded(&self) -> bool {
        self.0 & PADDED == PADDED
    }
}

impl Default for PushPromiseFlag {
    /// Returns a `PushPromiseFlag` value with `END_HEADERS` set.
    fn default() -> Self {
        PushPromiseFlag(END_HEADERS)
    }
}

impl From<PushPromiseFlag> for u8 {
    fn from(src: PushPromiseFlag) -> u8 {
        src.0
    }
}

impl fmt::Debug for PushPromiseFlag {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        util::debug_flags(fmt, self.0)
            .flag_if(self.is_end_headers(), "END_HEADERS")
            .flag_if(self.is_padded(), "PADDED")
            .finish()
    }
}

// ===== HeaderBlock =====

/// A header map the connection sends again on the next RPC.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum StandardMap {
    None,
    Request,
    Response,
    Trailers,
}

const CT: u8 = 1;
const TE: u8 = 2;
const ACCEPT: u8 = 4;
const UA: u8 = 8;
const STATUS: u8 = 16;
const REQUEST_BITS: u8 = CT | TE | ACCEPT | UA;
const RESPONSE_BITS: u8 = CT | ACCEPT;

const GRPC_ACCEPT_NAME: HeaderName = HeaderName::from_static("grpc-accept-encoding");
const GRPC_STATUS_NAME: HeaderName = HeaderName::from_static("grpc-status");
#[cfg(not(feature = "zstd"))]
const ACCEPT_VALUE: &[u8] = b"identity,gzip,deflate";
#[cfg(feature = "zstd")]
const ACCEPT_VALUE: &[u8] = b"identity,gzip,deflate,zstd";
const UA_VALUE: &[u8] = concat!("pbrs-grpc/", env!("CARGO_PKG_VERSION")).as_bytes();

impl StandardMap {
    fn slot(self) -> Option<crate::h2_backend::header_pool::Slot> {
        match self {
            StandardMap::Request => Some(crate::h2_backend::header_pool::Slot::Request),
            StandardMap::Response => Some(crate::h2_backend::header_pool::Slot::Response),
            StandardMap::Trailers => Some(crate::h2_backend::header_pool::Slot::Trailers),
            StandardMap::None => None,
        }
    }
}

fn standard_header_bit(name: &HeaderName, value: &HeaderValue) -> u8 {
    let bytes = value.as_bytes();
    if name == header::CONTENT_TYPE && bytes == b"application/grpc" {
        CT
    } else if name == header::TE && bytes == b"trailers" {
        TE
    } else if name == GRPC_ACCEPT_NAME && bytes == ACCEPT_VALUE {
        ACCEPT
    } else if name == header::USER_AGENT && bytes == UA_VALUE {
        UA
    } else if name == GRPC_STATUS_NAME && bytes == b"0" {
        STATUS
    } else {
        0
    }
}

/// Identity of a stock block worth replaying without rebuilding the key.
enum StableStandard {
    Request {
        https: bool,
        path: Vec<u8>,
        authority: Vec<u8>,
    },
    Response {
        status: u16,
    },
    Trailers,
}

fn fields_match(fields: &HeaderMap, expect_bits: u8, expect_count: usize) -> bool {
    if fields.len() != expect_count {
        return false;
    }
    let mut bits = 0u8;
    for (name, value) in fields {
        let bit = standard_header_bit(name, value);
        if bit == 0 || bits & bit != 0 {
            return false;
        }
        bits |= bit;
    }
    bits == expect_bits
}

/// Nine-byte frame head, written with `BytesMut`'s copy instead of `Limit`'s
/// chunk walk.
fn encode_head_into(head: Head, payload_len: usize, dst: &mut bytes::BytesMut) {
    let len = payload_len as u32;
    let id: u32 = head.stream_id().into();
    let prefix = [
        (len >> 16) as u8,
        (len >> 8) as u8,
        len as u8,
        head.kind() as u8,
        head.flag(),
        (id >> 24) as u8,
        (id >> 16) as u8,
        (id >> 8) as u8,
        id as u8,
    ];
    dst.extend_from_slice(&prefix);
}

/// A previously encoded stock block. The field walk runs only after the path,
/// authority, or status already matches, so a miss does not scan.
fn replayed_standard<'a>(
    encoder: &'a hpack::Encoder,
    pseudo: &Pseudo,
    fields: &HeaderMap,
    stock: StandardMap,
) -> Option<(StandardMap, &'a [u8])> {
    // The gRPC layer already classified this block. Replay from the pseudo
    // values and skip the method, protocol, and field-bit checks.
    match stock {
        StandardMap::Request => {
            let https = match pseudo.scheme.as_ref().map(|scheme| scheme.as_str()) {
                Some("https") => true,
                Some("http") => false,
                _ => return None,
            };
            let path = pseudo.path.as_ref()?.as_str().as_bytes();
            let authority = pseudo.authority.as_ref()?.as_str().as_bytes();
            if let Some(bytes) = encoder.replay_request(https, path, authority) {
                return Some((StandardMap::Request, bytes));
            }
        }
        StandardMap::Response => {
            if let Some(status) = pseudo.status {
                if let Some(bytes) = encoder.replay_response(status.as_u16()) {
                    return Some((StandardMap::Response, bytes));
                }
            }
        }
        StandardMap::Trailers => {
            if let Some(bytes) = encoder.replay_trailers() {
                return Some((StandardMap::Trailers, bytes));
            }
        }
        StandardMap::None => {}
    }
    if let Some(method) = pseudo.method.as_ref() {
        if method != Method::POST || pseudo.protocol.is_some() || pseudo.status.is_some() {
            return None;
        }
        let https = match pseudo.scheme.as_ref().map(|scheme| scheme.as_str()) {
            Some("https") => true,
            Some("http") => false,
            _ => return None,
        };
        let path = pseudo.path.as_ref()?.as_str().as_bytes();
        let authority = pseudo.authority.as_ref()?.as_str().as_bytes();
        let bytes = encoder.replay_request(https, path, authority)?;
        if stock != StandardMap::Request && !fields_match(fields, REQUEST_BITS, 4) {
            return None;
        }
        return Some((StandardMap::Request, bytes));
    }

    if let Some(status) = pseudo.status {
        if pseudo.scheme.is_some()
            || pseudo.authority.is_some()
            || pseudo.path.is_some()
            || pseudo.protocol.is_some()
        {
            return None;
        }
        let bytes = encoder.replay_response(status.as_u16())?;
        if stock != StandardMap::Response && !fields_match(fields, RESPONSE_BITS, 2) {
            return None;
        }
        return Some((StandardMap::Response, bytes));
    }

    if pseudo.scheme.is_some()
        || pseudo.authority.is_some()
        || pseudo.path.is_some()
        || pseudo.protocol.is_some()
    {
        return None;
    }
    let bytes = encoder.replay_trailers()?;
    if stock != StandardMap::Trailers && !fields_match(fields, STATUS, 1) {
        return None;
    }
    Some((StandardMap::Trailers, bytes))
}

/// Copies a previously encoded stock block into `dst`.
fn copy_stable_standard(
    encoder: &hpack::Encoder,
    pseudo: &Pseudo,
    fields: &HeaderMap,
    dst: &mut BytesMut,
    stock: StandardMap,
) -> Option<StandardMap> {
    let (kind, bytes) = replayed_standard(encoder, pseudo, fields, stock)?;
    dst.extend_from_slice(bytes);
    Some(kind)
}

fn stable_standard_id(kind: StandardMap, pseudo: &Pseudo) -> Option<StableStandard> {
    match kind {
        StandardMap::None => None,
        StandardMap::Request => {
            if pseudo.method.as_ref() != Some(&Method::POST)
                || pseudo.protocol.is_some()
                || pseudo.status.is_some()
            {
                return None;
            }
            let https = match pseudo.scheme.as_ref().map(|scheme| scheme.as_str()) {
                Some("https") => true,
                Some("http") => false,
                _ => return None,
            };
            let path = pseudo.path.as_ref()?.as_str().as_bytes();
            let authority = pseudo.authority.as_ref()?.as_str().as_bytes();
            if path.len() > 128 || authority.len() > 128 {
                return None;
            }
            Some(StableStandard::Request {
                https,
                path: path.to_vec(),
                authority: authority.to_vec(),
            })
        }
        StandardMap::Response => {
            if pseudo.scheme.is_some()
                || pseudo.authority.is_some()
                || pseudo.path.is_some()
                || pseudo.protocol.is_some()
                || pseudo.method.is_some()
            {
                return None;
            }
            Some(StableStandard::Response {
                status: pseudo.status?.as_u16(),
            })
        }
        StandardMap::Trailers => {
            if pseudo.method.is_some()
                || pseudo.scheme.is_some()
                || pseudo.authority.is_some()
                || pseudo.path.is_some()
                || pseudo.protocol.is_some()
                || pseudo.status.is_some()
            {
                return None;
            }
            Some(StableStandard::Trailers)
        }
    }
}

impl HeaderBlock {
    /// A cache hit returns before HPACK runs. The miss keeps its own frame so
    /// a repeated block does not reserve the decoder.
    #[inline(never)]
    fn load(
        &mut self,
        src: &mut BytesMut,
        max_header_list_size: usize,
        decoder: &mut hpack::Decoder,
    ) -> Result<(), Error> {
        // A new receive block is empty. The flag replaces the field walk on
        // the hit; a miss still records that the block is no longer empty.
        let fresh = self.pristine;
        // A repeated block that does not change the dynamic table decodes to
        // the same headers. The saved bytes were checked when stored, so a
        // hit does not walk the block again.
        if fresh {
            crate::h2_backend::header_pool::clear_stock_https();
            if let Some((kind, decoded)) = decoder.lookup_saved(src) {
                match kind {
                    hpack::BlockKind::Response(grpc_proto) => {
                        let fields = match crate::h2_backend::header_pool::take_received_response()
                        {
                            Some((spare, spare_proto)) if spare_proto == grpc_proto => spare,
                            Some((other, other_proto)) => {
                                crate::h2_backend::header_pool::recycle_received_response(
                                    other,
                                    other_proto,
                                );
                                decoded.fields.clone()
                            }
                            None => decoded.fields.clone(),
                        };
                        crate::h2_backend::header_pool::replace_map(&mut self.fields, fields);
                        self.pseudo.status = decoded.status;
                        self.field_size = decoded.field_size;
                        self.stock_facts = decoded.stock_facts;
                    }
                    hpack::BlockKind::Trailers => {
                        let fields = crate::h2_backend::header_pool::take_received_ok_trailer(
                            || decoded.fields.clone(),
                        );
                        crate::h2_backend::header_pool::replace_map(&mut self.fields, fields);
                        self.field_size = decoded.field_size;
                        self.stock_facts = decoded.stock_facts;
                    }
                    hpack::BlockKind::Request => {
                        // Stock requests publish the real map separately and
                        // leave the frame's map empty. A cache-hit POST also
                        // leaves the pseudos empty: the accept path rebuilds
                        // them from the process cache, so cloning them here
                        // only to drop them is wasted.
                        if let Some(shared) = &decoded.shared_fields {
                            crate::h2_backend::header_pool::publish_shared_request(
                                std::sync::Arc::clone(shared),
                            );
                        }
                        let https = match decoded.stock_https.get() {
                            2 => Some(false),
                            3 => Some(true),
                            1 => None,
                            _ => {
                                let https = crate::h2_backend::server::stock_https_of(
                                    decoded.method.as_ref(),
                                    decoded.scheme.as_ref(),
                                    decoded.authority.as_ref(),
                                    decoded.path.as_ref(),
                                );
                                decoded.stock_https.set(match https {
                                    None => 1,
                                    Some(false) => 2,
                                    Some(true) => 3,
                                });
                                https
                            }
                        };
                        if let Some(https) = https {
                            crate::h2_backend::header_pool::publish_stock_https(https);
                        } else {
                            self.pseudo.method = decoded.method.clone();
                            self.pseudo.scheme = decoded.scheme.clone();
                            self.pseudo.authority = decoded.authority.clone();
                            self.pseudo.path = decoded.path.clone();
                        }
                        self.pseudo.protocol = None;
                        self.pseudo.status = decoded.status;
                        self.field_size = decoded.field_size;
                        self.stock_facts = decoded.stock_facts;
                    }
                    hpack::BlockKind::Other => {
                        let saved = decoded.clone();
                        if saved.fields.capacity() == 0 && self.fields.capacity() == 0 {
                            std::mem::forget(saved.fields);
                        } else {
                            crate::h2_backend::header_pool::replace_map(
                                &mut self.fields,
                                saved.fields,
                            );
                        }
                        self.pseudo.method = saved.method;
                        self.pseudo.scheme = saved.scheme;
                        self.pseudo.authority = saved.authority;
                        self.pseudo.path = saved.path;
                        self.pseudo.protocol = saved.protocol;
                        self.pseudo.status = saved.status;
                        self.field_size = saved.field_size;
                        self.stock_facts = saved.stock_facts;
                    }
                }
                self.pristine = false;
                src.clear();
                return Ok(());
            }
        }
        self.load_uncached(src, max_header_list_size, decoder, fresh)
    }

    #[cold]
    #[inline(never)]
    fn load_uncached(
        &mut self,
        src: &mut BytesMut,
        max_header_list_size: usize,
        decoder: &mut hpack::Decoder,
        fresh: bool,
    ) -> Result<(), Error> {
        let neutral = fresh && hpack::block_leaves_table_unchanged(src);
        let record = neutral;
        let raw = if record { Some(src.to_vec()) } else { None };

        let mut reg = !self.fields.is_empty();
        let mut malformed = false;
        let mut header_list_way_too_large = false;
        let mut headers_size = self.calculate_header_list_size();
        let max_header_list_abuse_size =
            max_header_list_size.saturating_mul(MAX_HEADER_LIST_ABUSE_MULTIPLIER);

        let mut cursor = Cursor::new(src);

        // If the header frame is malformed, we still have to continue decoding
        // the headers. A malformed header frame is a stream level error, but
        // the hpack state is connection level. In order to maintain correct
        // state for other streams, the hpack decoding process must complete.
        let res = decoder.decode(&mut cursor, |header| {
            self.apply_decoded(
                header,
                &mut reg,
                &mut malformed,
                &mut header_list_way_too_large,
                &mut headers_size,
                max_header_list_size,
                max_header_list_abuse_size,
            )
        });

        if fresh {
            self.note_filled();
        }

        match res {
            Ok(()) => {}
            Err(e) => {
                tracing::trace!("hpack decoding error; err={:?}", e);
                return Err(e.into());
            }
        }

        if header_list_way_too_large {
            tracing::trace!("header list way too large; aborting connection");
            return Err(Error::HeaderListWayTooLarge);
        }

        if malformed {
            tracing::trace!("malformed message");
            return Err(Error::MalformedMessage);
        }

        if fresh {
            self.stock_facts = Some(crate::wire::headers::stock_facts(&self.fields));
        }
        if let Some(raw) = raw {
            let shared_fields = if self.pseudo.method.is_some()
                && self.stock_facts.is_some_and(|facts| facts.stock)
                && self.fields.len() == 4
            {
                Some(std::sync::Arc::new(self.fields.clone()))
            } else {
                None
            };
            decoder.store_block(
                raw,
                hpack::SavedHeaders {
                    fields: self.fields.clone(),
                    method: self.pseudo.method.clone(),
                    scheme: self.pseudo.scheme.clone(),
                    authority: self.pseudo.authority.clone(),
                    path: self.pseudo.path.clone(),
                    protocol: self.pseudo.protocol.clone(),
                    status: self.pseudo.status,
                    field_size: self.field_size,
                    stock_facts: self.stock_facts,
                    shared_fields,
                    stock_https: std::cell::Cell::new(0),
                },
            );
        }

        Ok(())
    }

    /// A partial decode that stored a header is no longer a cache candidate.
    fn note_filled(&mut self) {
        if self.fields.is_empty()
            && self.pseudo.method.is_none()
            && self.pseudo.scheme.is_none()
            && self.pseudo.authority.is_none()
            && self.pseudo.path.is_none()
            && self.pseudo.protocol.is_none()
            && self.pseudo.status.is_none()
        {
            return;
        }
        self.pristine = false;
    }

    fn apply_decoded(
        &mut self,
        header: hpack::Header,
        reg: &mut bool,
        malformed: &mut bool,
        header_list_way_too_large: &mut bool,
        headers_size: &mut usize,
        max_header_list_size: usize,
        max_header_list_abuse_size: usize,
    ) -> ControlFlow<()> {
        use crate::h2_backend::hpack::Header::*;

        macro_rules! too_big {
            () => {{
                let size = *headers_size;
                if size > max_header_list_abuse_size {
                    tracing::trace!("load_hpack; header list size over abuse max");
                    *header_list_way_too_large = true;
                    ControlFlow::Break(())
                } else {
                    if size >= max_header_list_size && !self.is_over_size {
                        tracing::trace!("load_hpack; header list size over max");
                        self.is_over_size = true;
                    }
                    ControlFlow::Continue(())
                }
            }};
        }

        macro_rules! set_saved_pseudo {
            ($field:ident, $val:expr) => {{
                if *reg {
                    tracing::trace!("load_hpack; header malformed -- pseudo not at head of block");
                    *malformed = true;
                } else if self.pseudo.$field.is_some() {
                    tracing::trace!("load_hpack; header malformed -- repeated pseudo");
                    *malformed = true;
                } else {
                    let __val = $val;
                    *headers_size +=
                        decoded_header_size(stringify!($field).len() + 1, __val.as_str().len());
                    if too_big!().is_break() {
                        return ControlFlow::Break(());
                    }
                    if !self.is_over_size {
                        self.pseudo.$field = Some(__val);
                    }
                }
            }};
        }

        match header {
            Field { name, value } => {
                if name == header::CONNECTION
                    || name == header::TRANSFER_ENCODING
                    || name == header::UPGRADE
                    || name == "keep-alive"
                    || name == "proxy-connection"
                {
                    tracing::trace!("load_hpack; connection level header");
                    *malformed = true;
                } else if name == header::TE && value != "trailers" {
                    tracing::trace!(
                        "load_hpack; TE header not set to trailers; val={:?}",
                        value
                    );
                    *malformed = true;
                } else {
                    *reg = true;
                    let header_size = decoded_header_size(name.as_str().len(), value.len());
                    *headers_size += header_size;
                    if too_big!().is_break() {
                        return ControlFlow::Break(());
                    }
                    if !self.is_over_size {
                        self.field_size += header_size;
                        let saw_length = name == header::CONTENT_LENGTH;
                        if self.fields.try_append(name, value).is_err() {
                            self.is_over_size = true;
                        } else if saw_length {
                            self.has_content_length = true;
                        }
                    }
                }
            }
            Authority(v) => set_saved_pseudo!(authority, v),
            Method(v) => set_saved_pseudo!(method, v),
            Scheme(v) => set_saved_pseudo!(scheme, v),
            Path(v) => set_saved_pseudo!(path, v),
            Protocol(v) => set_saved_pseudo!(protocol, v),
            Status(v) => set_saved_pseudo!(status, v),
        }
        ControlFlow::Continue(())
    }

    fn into_encoding(self, encoder: &mut hpack::Encoder) -> EncodingHeaderBlock {
        let mut hpack = encoder.take_scratch();
        hpack.clear();
        // Stock gRPC blocks repeat. Once stored, replay skips the key fill.
        if !encoder.has_size_update() {
            if let Some(kind) =
                copy_stable_standard(encoder, &self.pseudo, &self.fields, &mut hpack, self.stock)
            {
                let fields = self.fields;
                if let Some(slot) = kind.slot() {
                    crate::h2_backend::header_pool::recycle(slot, fields);
                }
                return EncodingHeaderBlock { hpack };
            }
        }
        // The same headers on a stable table encode to the same bytes.
        // The key buffer lives on the encoder, so a hit does not allocate.
        let mut recycle = StandardMap::None;
        let keyed = if encoder.has_size_update() {
            false
        } else {
            encoder.clear_key();
            Self::fill_encoding_key(
                encoder.key_buf(),
                &self.pseudo,
                &self.fields,
                &mut recycle,
            )
        };
        if keyed {
            if let Some(bytes) = encoder.cached_block() {
                hpack.extend_from_slice(bytes);
                // The block is cached, so this map is not needed for encoding.
                // A standard gRPC block goes back to the pool instead of being freed.
                let fields = self.fields;
                if let Some(slot) = recycle.slot() {
                    crate::h2_backend::header_pool::recycle(slot, fields);
                }
                return EncodingHeaderBlock { hpack };
            }
        }
        let stable = if keyed {
            stable_standard_id(recycle, &self.pseudo)
        } else {
            None
        };
        let epoch = encoder.table_epoch();
        let headers = Iter {
            pseudo: Some(self.pseudo),
            fields: self.fields.into_iter(),
        };

        encoder.encode(headers, &mut hpack);
        if keyed && encoder.table_epoch() == epoch {
            let key = encoder.key_buf().clone();
            encoder.store_block(key, &hpack);
            match stable {
                Some(StableStandard::Request {
                    https,
                    path,
                    authority,
                }) => encoder.store_request(https, &path, &authority, &hpack),
                Some(StableStandard::Response { status }) => encoder.store_response(status, &hpack),
                Some(StableStandard::Trailers) => encoder.store_trailers(&hpack),
                None => {}
            }
        }

        EncodingHeaderBlock { hpack }
    }

    /// Writes the cache key into `key`, which the caller has cleared.
    ///
    /// Returns false when the headers do not fit in the 256-byte key. Those
    /// blocks are encoded normally and not cached. `recycle` is set when
    /// `fields` is one of the three maps the pool reuses.
    fn fill_encoding_key(
        key: &mut Vec<u8>,
        pseudo: &Pseudo,
        fields: &HeaderMap,
        recycle: &mut StandardMap,
    ) -> bool {
        fn push(key: &mut Vec<u8>, bytes: Option<&[u8]>) -> bool {
            match bytes {
                None => {
                    if key.len() == 256 {
                        return false;
                    }
                    key.push(0xff);
                    true
                }
                Some(bytes) => {
                    if bytes.len() > 254 || key.len() + 1 + bytes.len() > 256 {
                        return false;
                    }
                    key.push(bytes.len() as u8);
                    key.extend_from_slice(bytes);
                    true
                }
            }
        }
        let ok = push(
            key,
            pseudo.method.as_ref().map(|value| value.as_str().as_bytes()),
        ) && push(
            key,
            pseudo.scheme.as_ref().map(|value| value.as_str().as_bytes()),
        ) && push(
            key,
            pseudo
                .authority
                .as_ref()
                .map(|value| value.as_str().as_bytes()),
        ) && push(
            key,
            pseudo.path.as_ref().map(|value| value.as_str().as_bytes()),
        ) && push(
            key,
            pseudo
                .protocol
                .as_ref()
                .map(|value| value.as_str().as_bytes()),
        ) && push(
            key,
            pseudo.status.as_ref().map(|value| value.as_str().as_bytes()),
        );
        if !ok {
            return false;
        }
        let mut bits = 0u8;
        let mut count = 0u8;
        let mut ordinary = true;
        for (name, value) in fields {
            if ordinary {
                let bit = standard_header_bit(name, value);
                if bit == 0 {
                    ordinary = false;
                } else {
                    bits |= bit;
                    count += 1;
                }
            }
            if !push(key, Some(name.as_str().as_bytes())) || !push(key, Some(value.as_bytes())) {
                return false;
            }
        }
        *recycle = match (ordinary, count, bits) {
            (true, 4, REQUEST_BITS) => StandardMap::Request,
            (true, 2, RESPONSE_BITS) => StandardMap::Response,
            (true, 1, STATUS) => StandardMap::Trailers,
            _ => StandardMap::None,
        };
        true
    }

    /// Calculates the size of the currently decoded header list.
    ///
    /// According to http://httpwg.org/specs/rfc7540.html#SETTINGS_MAX_HEADER_LIST_SIZE
    ///
    /// > The value is based on the uncompressed size of header fields,
    /// > including the length of the name and value in octets plus an
    /// > overhead of 32 octets for each header field.
    fn calculate_header_list_size(&self) -> usize {
        macro_rules! pseudo_size {
            ($name:ident) => {{
                self.pseudo
                    .$name
                    .as_ref()
                    .map(|m| decoded_header_size(stringify!($name).len() + 1, m.as_str().len()))
                    .unwrap_or(0)
            }};
        }

        pseudo_size!(method)
            + pseudo_size!(scheme)
            + pseudo_size!(status)
            + pseudo_size!(authority)
            + pseudo_size!(path)
            + self.field_size
    }
}

fn decoded_header_size(name: usize, value: usize) -> usize {
    name + value + 32
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::h2_backend::frame;
    use crate::h2_backend::hpack::{huffman, Encoder};

    #[test]
    fn test_nameless_header_at_resume() {
        let mut encoder = Encoder::default();
        let mut dst = BytesMut::new();

        let headers = Headers::new(
            StreamId::ZERO,
            Default::default(),
            HeaderMap::from_iter(vec![
                (
                    HeaderName::from_static("hello"),
                    HeaderValue::from_static("world"),
                ),
                (
                    HeaderName::from_static("hello"),
                    HeaderValue::from_static("zomg"),
                ),
                (
                    HeaderName::from_static("hello"),
                    HeaderValue::from_static("sup"),
                ),
            ]),
        );

        let continuation = headers
            .encode(&mut encoder, &mut (&mut dst).limit(frame::HEADER_LEN + 8))
            .unwrap();

        assert_eq!(17, dst.len());
        assert_eq!([0, 0, 8, 1, 0, 0, 0, 0, 0], &dst[0..9]);
        assert_eq!(&[0x40, 0x80 | 4], &dst[9..11]);
        assert_eq!("hello", huff_decode(&dst[11..15]));
        assert_eq!(0x80 | 4, dst[15]);

        let mut world = dst[16..17].to_owned();

        dst.clear();

        assert!(continuation
            .encode(&mut (&mut dst).limit(frame::HEADER_LEN + 16))
            .is_none());

        world.extend_from_slice(&dst[9..12]);
        assert_eq!("world", huff_decode(&world));

        assert_eq!(24, dst.len());
        assert_eq!([0, 0, 15, 9, 4, 0, 0, 0, 0], &dst[0..9]);

        // // Next is not indexed
        assert_eq!(&[15, 47, 0x80 | 3], &dst[12..15]);
        assert_eq!("zomg", huff_decode(&dst[15..18]));
        assert_eq!(&[15, 47, 0x80 | 3], &dst[18..21]);
        assert_eq!("sup", huff_decode(&dst[21..]));
    }

    fn huff_decode(src: &[u8]) -> BytesMut {
        let mut buf = BytesMut::new();
        huffman::decode(src, &mut buf).unwrap()
    }

    #[test]
    fn test_connect_request_pseudo_headers_omits_path_and_scheme() {
        // CONNECT requests MUST NOT include :scheme & :path pseudo-header fields
        // See: https://datatracker.ietf.org/doc/html/rfc9113#section-8.5

        assert_eq!(
            Pseudo::request(
                Method::CONNECT,
                Uri::from_static("https://example.com:8443"),
                None
            ),
            Pseudo {
                method: Method::CONNECT.into(),
                authority: BytesStr::from_static("example.com:8443").into(),
                ..Default::default()
            }
        );

        assert_eq!(
            Pseudo::request(
                Method::CONNECT,
                Uri::from_static("https://example.com/test"),
                None
            ),
            Pseudo {
                method: Method::CONNECT.into(),
                authority: BytesStr::from_static("example.com").into(),
                ..Default::default()
            }
        );

        assert_eq!(
            Pseudo::request(Method::CONNECT, Uri::from_static("example.com:8443"), None),
            Pseudo {
                method: Method::CONNECT.into(),
                authority: BytesStr::from_static("example.com:8443").into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_extended_connect_request_pseudo_headers_includes_path_and_scheme() {
        // On requests that contain the :protocol pseudo-header field, the
        // :scheme and :path pseudo-header fields of the target URI (see
        // Section 5) MUST also be included.
        // See: https://datatracker.ietf.org/doc/html/rfc8441#section-4

        assert_eq!(
            Pseudo::request(
                Method::CONNECT,
                Uri::from_static("https://example.com:8443"),
                Protocol::from_static("the-bread-protocol").into()
            ),
            Pseudo {
                method: Method::CONNECT.into(),
                authority: BytesStr::from_static("example.com:8443").into(),
                scheme: BytesStr::from_static("https").into(),
                path: BytesStr::from_static("/").into(),
                protocol: Protocol::from_static("the-bread-protocol").into(),
                ..Default::default()
            }
        );

        assert_eq!(
            Pseudo::request(
                Method::CONNECT,
                Uri::from_static("https://example.com:8443/test"),
                Protocol::from_static("the-bread-protocol").into()
            ),
            Pseudo {
                method: Method::CONNECT.into(),
                authority: BytesStr::from_static("example.com:8443").into(),
                scheme: BytesStr::from_static("https").into(),
                path: BytesStr::from_static("/test").into(),
                protocol: Protocol::from_static("the-bread-protocol").into(),
                ..Default::default()
            }
        );

        assert_eq!(
            Pseudo::request(
                Method::CONNECT,
                Uri::from_static("http://example.com/a/b/c"),
                Protocol::from_static("the-bread-protocol").into()
            ),
            Pseudo {
                method: Method::CONNECT.into(),
                authority: BytesStr::from_static("example.com").into(),
                scheme: BytesStr::from_static("http").into(),
                path: BytesStr::from_static("/a/b/c").into(),
                protocol: Protocol::from_static("the-bread-protocol").into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_options_request_with_empty_path_has_asterisk_as_pseudo_path() {
        // an OPTIONS request for an "http" or "https" URI that does not include a path component;
        // these MUST include a ":path" pseudo-header field with a value of '*' (see Section 7.1 of [HTTP]).
        // See: https://datatracker.ietf.org/doc/html/rfc9113#section-8.3.1
        assert_eq!(
            Pseudo::request(Method::OPTIONS, Uri::from_static("example.com:8080"), None,),
            Pseudo {
                method: Method::OPTIONS.into(),
                authority: BytesStr::from_static("example.com:8080").into(),
                path: BytesStr::from_static("*").into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_try_append_prevents_panic_on_max_size_reached() {
        // Verify that decoding >24,577 unique headers sets `is_over_size`
        // instead of panicking via HeaderMap::append().
        //
        // HeaderMap::MAX_SIZE = 32,768. With 75% load factor, max entries = 24,576.
        // try_append returns Err(MaxSizeReached) at entry 24,577.
        // Before the fix (using append), this panicked.
        //
        // We manually construct HPACK bytes for 25,000 unique headers because
        // creating a HeaderMap with that many entries also panics on construction.

        // Build HPACK-encoded block:
        // Pseudo-headers (indexed refs to static table):
        //   :method GET         → 0x82 (static index 2)
        //   :scheme http        → 0x86 (static index 6)
        //   :path /             → 0x84 (static index 4)
        //   :authority "localhost" → literal with indexing (name index 0)
        //
        // Then 25,000 unique headers: "literal without indexing, new name"
        //   0x00 → literal without indexing, name index 0
        //   <name_len> <name_bytes>
        //   <value_len> <value_bytes>

        let num_headers = 25_000;

        // Build the HPACK block
        let mut hpack = Vec::new();

        // Pseudo-headers
        hpack.push(0x82u8); // :method GET (static index 2)
        hpack.push(0x86); // :scheme http (static index 6)
        hpack.push(0x84); // :path / (static index 4)

        // :authority "localhost" — literal with incremental indexing
        hpack.push(0x41); // literal with indexing, name index 1 (= ":authority")
        hpack.push(0x09); // value length 9
        hpack.extend_from_slice(b"localhost");

        // 25,000 unique headers: "literal without indexing, new name"
        // Format: 0x00 + name_len + name + value_len + value
        for i in 0..num_headers {
            let name = format!("x-h-{i}");
            hpack.push(0x00u8); // literal without indexing, name index 0
            hpack.push(name.len() as u8);
            hpack.extend_from_slice(name.as_bytes());
            hpack.push(1u8); // value length 1
            hpack.push(b'v');
        }

        // Build the HTTP/2 HEADERS frame: 9-byte header + HPACK payload
        let payload_len = hpack.len();
        let mut frame = BytesMut::with_capacity(9 + payload_len);

        // Frame header: 3 bytes length, 1 byte type (0x01=HEADERS), 1 byte flags, 4 bytes stream_id
        frame.put_u8(((payload_len >> 16) & 0xFF) as u8);
        frame.put_u8(((payload_len >> 8) & 0xFF) as u8);
        frame.put_u8((payload_len & 0xFF) as u8);
        frame.put_u8(0x01); // type: HEADERS
        frame.put_u8(0x04); // flags: END_HEADERS
        frame.put_u32(1); // stream_id: 1

        frame.extend_from_slice(&hpack);

        // Parse the HEADERS frame
        let head = Head::parse(&frame[..9]);
        let payload = BytesMut::from(&frame[9..]);
        let (mut headers, mut hpack_data) = Headers::load(head, payload).unwrap();
        // hpack_data contains the HPACK payload (no padding/priority in our frame)

        // Decode the HPACK block — this should NOT panic
        let mut decoder = hpack::Decoder::new(4096);
        const DEFAULT_MAX_HEADER_LIST_SIZE: usize = 16 << 20; // 16 MB
        headers
            .load_hpack(&mut hpack_data, DEFAULT_MAX_HEADER_LIST_SIZE, &mut decoder)
            .expect("load_hpack should return Ok");

        // Verify that is_over_size was set (try_append returned Err)
        assert!(
            headers.is_over_size(),
            "is_over_size should be true when HeaderMap capacity is exceeded"
        );
    }

    #[test]
    fn test_non_option_and_non_connect_requests_include_path_and_scheme() {
        let methods = [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::HEAD,
            Method::PATCH,
            Method::TRACE,
        ];

        for method in methods {
            assert_eq!(
                Pseudo::request(
                    method.clone(),
                    Uri::from_static("http://example.com:8080"),
                    None,
                ),
                Pseudo {
                    method: method.clone().into(),
                    authority: BytesStr::from_static("example.com:8080").into(),
                    scheme: BytesStr::from_static("http").into(),
                    path: BytesStr::from_static("/").into(),
                    ..Default::default()
                }
            );
            assert_eq!(
                Pseudo::request(
                    method.clone(),
                    Uri::from_static("https://example.com/a/b/c"),
                    None,
                ),
                Pseudo {
                    method: method.into(),
                    authority: BytesStr::from_static("example.com").into(),
                    scheme: BytesStr::from_static("https").into(),
                    path: BytesStr::from_static("/a/b/c").into(),
                    ..Default::default()
                }
            );
        }
    }

    fn encode_block(encoder: &mut Encoder, path: &'static str, value: &'static str) -> BytesMut {
        let mut dst = BytesMut::new();
        let frame = Headers::new(
            StreamId::from(1),
            Pseudo::request(Method::POST, Uri::from_static(path), None),
            HeaderMap::from_iter([(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static(value),
            )]),
        );
        assert!(
            frame
                .encode(encoder, &mut (&mut dst).limit(1024))
                .is_none()
        );
        dst
    }

    #[test]
    fn repeated_header_block_reuses_the_stable_encoding() {
        let mut encoder = Encoder::default();
        let path = "http://localhost/echo.Echo/Echo";
        let _first = encode_block(&mut encoder, path, "application/grpc");
        let second = encode_block(&mut encoder, path, "application/grpc");
        let third = encode_block(&mut encoder, path, "application/grpc");
        // The first encode may insert into the dynamic table. Once the table
        // is stable, the next two blocks are the cached bytes.
        assert_eq!(&second[9..], &third[9..]);
        // A different value is inserted, so the previous block must not be replayed.
        let changed = encode_block(&mut encoder, path, "application/grpc+proto");
        assert_ne!(&third[9..], &changed[9..]);
        let changed_again = encode_block(&mut encoder, path, "application/grpc+proto");
        let changed_stable = encode_block(&mut encoder, path, "application/grpc+proto");
        assert_eq!(&changed_again[9..], &changed_stable[9..]);
    }

    fn encode_frame(encoder: &mut Encoder, frame: Headers) {
        let mut dst = BytesMut::new();
        assert!(
            frame
                .encode(encoder, &mut (&mut dst).limit(1024))
                .is_none()
        );
    }

    #[test]
    fn standard_blocks_return_to_the_header_pool() {
        use crate::h2_backend::header_pool::{self, Slot};

        let _ = header_pool::take(Slot::Request, HeaderMap::new);
        let _ = header_pool::take(Slot::Response, HeaderMap::new);
        let _ = header_pool::take(Slot::Trailers, HeaderMap::new);

        let mut map = HeaderMap::with_capacity(4);
        map.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/grpc"),
        );
        map.insert(header::TE, HeaderValue::from_static("trailers"));
        map.insert(
            super::GRPC_ACCEPT_NAME.clone(),
            HeaderValue::from_bytes(super::ACCEPT_VALUE).unwrap(),
        );
        map.insert(
            header::USER_AGENT,
            HeaderValue::from_static(concat!("pbrs-grpc/", env!("CARGO_PKG_VERSION"))),
        );
        let mut encoder = Encoder::default();
        let path = Uri::from_static("http://localhost/echo.Echo/Echo");
        for _ in 0..3 {
            encode_frame(
                &mut encoder,
                Headers::new(
                    StreamId::from(1),
                    Pseudo::request(Method::POST, path.clone(), None),
                    map.clone(),
                ),
            );
        }
        let pooled = header_pool::take(Slot::Request, HeaderMap::new);
        assert_eq!(pooled.len(), 4);
        assert_eq!(
            pooled.get(header::CONTENT_TYPE).map(|v| v.as_bytes()),
            Some(b"application/grpc".as_slice())
        );

        let _ = header_pool::take(Slot::Request, HeaderMap::new);
        let mut weird = HeaderMap::new();
        weird.insert(
            HeaderName::from_static("x-test"),
            HeaderValue::from_static("1"),
        );
        let weird_path = Uri::from_static("http://localhost/echo.Echo/Echo");
        encode_frame(
            &mut encoder,
            Headers::new(
                StreamId::from(1),
                Pseudo::request(Method::POST, weird_path.clone(), None),
                weird.clone(),
            ),
        );
        encode_frame(
            &mut encoder,
            Headers::new(
                StreamId::from(1),
                Pseudo::request(Method::POST, weird_path, None),
                weird,
            ),
        );
        let pooled = header_pool::take(Slot::Request, HeaderMap::new);
        assert!(pooled.is_empty());
    }

    fn standard_request_map() -> HeaderMap {
        let mut map = HeaderMap::with_capacity(4);
        map.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/grpc"),
        );
        map.insert(header::TE, HeaderValue::from_static("trailers"));
        map.insert(
            super::GRPC_ACCEPT_NAME.clone(),
            HeaderValue::from_bytes(super::ACCEPT_VALUE).unwrap(),
        );
        map.insert(
            header::USER_AGENT,
            HeaderValue::from_static(concat!("pbrs-grpc/", env!("CARGO_PKG_VERSION"))),
        );
        map
    }

    fn encode_standard(encoder: &mut Encoder, path: &'static str) -> BytesMut {
        let mut dst = BytesMut::new();
        let frame = Headers::new(
            StreamId::from(1),
            Pseudo::request(Method::POST, Uri::from_static(path), None),
            standard_request_map(),
        );
        assert!(
            frame
                .encode(encoder, &mut (&mut dst).limit(1024))
                .is_none()
        );
        dst
    }

    #[test]
    fn standard_blocks_replay_without_mixing_paths_or_size_updates() {
        let mut encoder = Encoder::default();
        let path_a = "http://localhost/echo.Echo/Echo";
        let path_b = "http://localhost/echo.Echo/Other";
        let _ = encode_standard(&mut encoder, path_a);
        let second = encode_standard(&mut encoder, path_a);
        let third = encode_standard(&mut encoder, path_a);
        assert_eq!(&second[9..], &third[9..]);
        let other = encode_standard(&mut encoder, path_b);
        assert_ne!(&third[9..], &other[9..]);

        let mut response_encoder = Encoder::default();
        let mut response_map = HeaderMap::with_capacity(2);
        response_map.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/grpc"),
        );
        response_map.insert(
            super::GRPC_ACCEPT_NAME.clone(),
            HeaderValue::from_bytes(super::ACCEPT_VALUE).unwrap(),
        );
        let encode_response = |encoder: &mut Encoder| {
            let mut dst = BytesMut::new();
            let frame = Headers::new(
                StreamId::from(1),
                Pseudo::response(StatusCode::OK),
                response_map.clone(),
            );
            assert!(
                frame
                    .encode(encoder, &mut (&mut dst).limit(1024))
                    .is_none()
            );
            dst
        };
        let _ = encode_response(&mut response_encoder);
        let response_b = encode_response(&mut response_encoder);
        let response_c = encode_response(&mut response_encoder);
        assert_eq!(&response_b[9..], &response_c[9..]);

        let mut trailer_encoder = Encoder::default();
        let mut trailers = HeaderMap::with_capacity(1);
        trailers.insert(super::GRPC_STATUS_NAME.clone(), HeaderValue::from_static("0"));
        let encode_trailers = |encoder: &mut Encoder| {
            let mut dst = BytesMut::new();
            let frame = Headers::trailers(StreamId::from(1), trailers.clone());
            assert!(
                frame
                    .encode(encoder, &mut (&mut dst).limit(1024))
                    .is_none()
            );
            dst
        };
        let _ = encode_trailers(&mut trailer_encoder);
        let trailers_b = encode_trailers(&mut trailer_encoder);
        let trailers_c = encode_trailers(&mut trailer_encoder);
        assert_eq!(&trailers_b[9..], &trailers_c[9..]);

        let mut sized = Encoder::default();
        let _ = encode_standard(&mut sized, path_a);
        let stable = encode_standard(&mut sized, path_a);
        sized.update_max_size(0);
        let updated = encode_standard(&mut sized, path_a);
        assert_ne!(&stable[9..], &updated[9..]);
    }

    #[test]
    fn size_update_is_not_served_from_the_block_cache() {
        let mut encoder = Encoder::default();
        let path = "http://localhost/echo.Echo/Echo";
        let _ = encode_block(&mut encoder, path, "application/grpc");
        let stable = encode_block(&mut encoder, path, "application/grpc");
        encoder.update_max_size(0);
        let updated = encode_block(&mut encoder, path, "application/grpc");
        assert_ne!(&stable[9..], &updated[9..]);
    }
}
