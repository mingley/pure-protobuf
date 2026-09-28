"""Decode the first HTTP/2 response :status for the local server probes."""

_STATIC_STATUSES = {
    8: 200,
    9: 204,
    10: 206,
    11: 304,
    12: 400,
    13: 404,
    14: 500,
}

# RFC 7541 Huffman encodings used by the pinned h2 peer for these probe codes.
_HUFFMAN_STATUSES = {b"\x68\x0d\xff": 405, b"\x68\x2d\xff": 415}


def _integer(data: bytes, pos: int, prefix_bits: int) -> tuple[int, int]:
    if pos >= len(data):
        raise ValueError("truncated HPACK integer")
    limit = (1 << prefix_bits) - 1
    value = data[pos] & limit
    pos += 1
    if value < limit:
        return value, pos
    shift = 0
    while True:
        if pos >= len(data) or shift > 28:
            raise ValueError("truncated or oversized HPACK integer")
        byte = data[pos]
        pos += 1
        value += (byte & 0x7F) << shift
        if (byte & 0x80) == 0:
            return value, pos
        shift += 7


def decode_first_status(frame: tuple[int, int, int, bytes]) -> int:
    frame_type, flags, stream_id, payload = frame
    if frame_type != 1 or stream_id != 1 or (flags & 0x04) == 0:
        raise ValueError("expected stream 1 HEADERS with END_HEADERS")
    if flags & (0x08 | 0x20) or not payload:
        raise ValueError("unsupported padded/priority or empty HEADERS")

    pos = 0
    while pos < len(payload) and (payload[pos] & 0xE0) == 0x20:
        _, pos = _integer(payload, pos, 5)
    if pos >= len(payload):
        raise ValueError("HEADERS contained no :status")

    first = payload[pos]
    if first & 0x80:
        index, _ = _integer(payload, pos, 7)
        try:
            return _STATIC_STATUSES[index]
        except KeyError as exc:
            raise ValueError(f"unsupported indexed :status {index}") from exc
    if first & 0x40:
        prefix_bits = 6
    elif (first & 0xF0) in (0x00, 0x10):
        prefix_bits = 4
    else:
        raise ValueError("HEADERS did not start with :status")
    name_index, pos = _integer(payload, pos, prefix_bits)
    if name_index != 8:
        raise ValueError(f"expected :status name index 8, got {name_index}")
    if pos >= len(payload):
        raise ValueError("missing :status value")
    huffman = (payload[pos] & 0x80) != 0
    length, pos = _integer(payload, pos, 7)
    if pos + length > len(payload):
        raise ValueError("truncated :status value")
    raw = payload[pos : pos + length]
    if huffman:
        try:
            return _HUFFMAN_STATUSES[raw]
        except KeyError as exc:
            raise ValueError(f"unsupported Huffman :status {raw.hex()}") from exc
    if len(raw) != 3 or not raw.isdigit():
        raise ValueError(f"invalid plain :status {raw!r}")
    return int(raw)
