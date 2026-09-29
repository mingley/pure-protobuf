package xlang

import (
	"fmt"

	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/reflect/protoreflect"
	"google.golang.org/protobuf/reflect/protoregistry"
)

// stdSession is a Session over generated types via the standard
// google.golang.org/protobuf runtime (proto.Unmarshal / proto.Marshal, which
// dispatch to the generated fast paths). The descriptor set argument is
// accepted for CLI compatibility and ignored: types are compiled in.
type stdSession struct {
	mt         protoreflect.MessageType
	marshal    proto.MarshalOptions
	marshalDet proto.MarshalOptions
}

// NewStdSession resolves message in the global registry populated by the
// generated code linked into the binary.
func NewStdSession(message string) (Session, error) {
	mt, err := protoregistry.GlobalTypes.FindMessageByName(protoreflect.FullName(message))
	if err != nil {
		return nil, fmt.Errorf("message %s not registered in this binary: %w", message, err)
	}
	return &stdSession{
		mt:         mt,
		marshalDet: proto.MarshalOptions{Deterministic: true},
	}, nil
}

func (s *stdSession) NewMessage() proto.Message { return s.mt.New().Interface() }

func (s *stdSession) Parse(m proto.Message, b []byte) error { return proto.Unmarshal(b, m) }

func (s *stdSession) MarshalAppend(m proto.Message, buf []byte) ([]byte, error) {
	return s.marshal.MarshalAppend(buf, m)
}

func (s *stdSession) MarshalDeterministic(m proto.Message) ([]byte, error) {
	return s.marshalDet.Marshal(m)
}
