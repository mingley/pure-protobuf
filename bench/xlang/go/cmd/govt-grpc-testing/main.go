// Command harness-go-vt-grpc-testing times the vtprotobuf generated
// fast-path peer on the grpc-testing corpus. See bench/xlang/go/README.md.
package main

import (
	gengrpctesting "github.com/mingley/pure-protobuf/bench/xlang/go/gen/grpc-testing"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

func main() {
	xlang.Run("go_vt", "vt", func(_, _, message string) (xlang.Session, error) {
		return xlang.NewVTSession(message, gengrpctesting.VTExcludedFiles)
	})
}
