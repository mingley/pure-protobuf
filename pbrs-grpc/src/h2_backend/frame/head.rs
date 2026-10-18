use super::StreamId;

use bytes::BufMut;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Head {
    kind: Kind,
    flag: u8,
    stream_id: StreamId,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    Headers = 1,
    Priority = 2,
    Reset = 3,
    Settings = 4,
    PushPromise = 5,
    Ping = 6,
    GoAway = 7,
    WindowUpdate = 8,
    Continuation = 9,
    Unknown,
}

// ===== impl Head =====

impl Head {
    pub fn new(kind: Kind, flag: u8, stream_id: StreamId) -> Head {
        Head {
            kind,
            flag,
            stream_id,
        }
    }

    /// Parse an HTTP/2 frame header
    pub fn parse(header: &[u8]) -> Head {
        // One length check. Indexing the 9-byte array does not check again.
        let prefix: &[u8; super::HEADER_LEN] = match header.get(..super::HEADER_LEN) {
            Some(prefix) => prefix.try_into().unwrap_or_else(|_| short_header()),
            None => short_header(),
        };
        let (stream_id, _) = StreamId::parse(&prefix[5..9]);

        Head {
            kind: Kind::new(prefix[3]),
            flag: prefix[4],
            stream_id,
        }
    }

    pub fn stream_id(&self) -> StreamId {
        self.stream_id
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn flag(&self) -> u8 {
        self.flag
    }

    pub fn encode_len(&self) -> usize {
        super::HEADER_LEN
    }

    pub fn encode<T: BufMut>(&self, payload_len: usize, dst: &mut T) {
        debug_assert!(self.encode_len() <= dst.remaining_mut());
        debug_assert!(payload_len < (1 << 24));

        // One 9-byte store. Four separate puts each copied a fragment.
        let len = payload_len as u32;
        let id: u32 = self.stream_id.into();
        let head = [
            (len >> 16) as u8,
            (len >> 8) as u8,
            len as u8,
            self.kind as u8,
            self.flag,
            (id >> 24) as u8,
            (id >> 16) as u8,
            (id >> 8) as u8,
            id as u8,
        ];
        dst.put_slice(&head);
    }
}

// ===== impl Kind =====

impl Kind {
    pub fn new(byte: u8) -> Kind {
        match byte {
            0 => Kind::Data,
            1 => Kind::Headers,
            n => Kind::from_byte_other(n),
        }
    }

    /// Control-frame kinds. A gRPC connection's steady frames are DATA and HEADERS.
    #[cold]
    #[inline(never)]
    fn from_byte_other(byte: u8) -> Kind {
        match byte {
            2 => Kind::Priority,
            3 => Kind::Reset,
            4 => Kind::Settings,
            5 => Kind::PushPromise,
            6 => Kind::Ping,
            7 => Kind::GoAway,
            8 => Kind::WindowUpdate,
            9 => Kind::Continuation,
            _ => Kind::Unknown,
        }
    }
}

#[cold]
#[inline(never)]
fn short_header() -> &'static [u8; super::HEADER_LEN] {
    panic!("HTTP/2 frame shorter than the 9-byte header");
}
