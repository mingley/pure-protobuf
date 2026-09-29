// Command harness-go-grpc-testing times the google.golang.org/protobuf
// generated-code peer on the grpc-testing corpus. See bench/xlang/go/README.md.
package main

import (
	_ "github.com/mingley/pure-protobuf/bench/xlang/go/gen/grpc-testing"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

func main() {
	xlang.Run("go", "generated", func(_, _, message string) (xlang.Session, error) {
		return xlang.NewStdSession(message)
	})
}
