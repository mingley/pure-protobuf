// Command harness-hyperpb times Buf's hyperpb dynamic parser (SB-07).
//
// Schema-generic like the C++ peer: it loads a FileDescriptorSet produced by
// the pinned protoc, compiles a hyperpb parser for one message type, and
// either verifies the round-trip or times decode loops. hyperpb is a
// parse-only peer: it is compared only in the dynamic-parse category (A12),
// so --op encode is rejected as Unsupported and the driver records those
// cells as not_run. See bench/xlang/go/README.md.
package main

import (
	"fmt"
	"os"

	"buf.build/go/hyperpb"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/reflect/protoreflect"
	"google.golang.org/protobuf/types/descriptorpb"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

// hyperSession is an xlang.Session over hyperpb dynamic messages. Marshal
// works through protobuf-go's generic reflection fallback (hyperpb populates
// only unmarshal methods); it is used for verification only, never timed.
type hyperSession struct {
	mt         *hyperpb.MessageType
	marshal    proto.MarshalOptions
	marshalDet proto.MarshalOptions
}

func newHyperSession(desc, message string) (xlang.Session, error) {
	raw, err := os.ReadFile(desc)
	if err != nil {
		return nil, fmt.Errorf("cannot read desc %s: %w", desc, err)
	}
	var fds descriptorpb.FileDescriptorSet
	if err := proto.Unmarshal(raw, &fds); err != nil {
		return nil, fmt.Errorf("cannot parse FileDescriptorSet: %w", err)
	}
	mt, err := hyperpb.CompileFileDescriptorSet(&fds, protoreflect.FullName(message))
	if err != nil {
		return nil, xlang.Unsupported{Reason: fmt.Sprintf("hyperpb cannot compile %s: %v", message, err)}
	}
	return &hyperSession{
		mt:         mt,
		marshalDet: proto.MarshalOptions{Deterministic: true},
	}, nil
}

func (s *hyperSession) NewMessage() proto.Message { return hyperpb.NewMessage(s.mt) }

func (s *hyperSession) Parse(m proto.Message, b []byte) error { return proto.Unmarshal(b, m) }

func (s *hyperSession) MarshalAppend(m proto.Message, buf []byte) ([]byte, error) {
	return s.marshal.MarshalAppend(buf, m)
}

func (s *hyperSession) MarshalDeterministic(m proto.Message) ([]byte, error) {
	return s.marshalDet.Marshal(m)
}

func main() {
	xlang.Run("go_hyperpb", "dynamic", func(op, desc, message string) (xlang.Session, error) {
		// The driver never routes encode cells here (it records them as
		// not_run directly); reject defensively so direct invocations
		// cannot produce a timed hyperpb encode number.
		if op == "encode" {
			return nil, xlang.Unsupported{Reason: "hyperpb is decode-only; compared only in A12 (dynamic/reflection decode)"}
		}
		return newHyperSession(desc, message)
	})
}
