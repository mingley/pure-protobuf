// Command harness-go-vt-otlp times the vtprotobuf generated
// fast-path peer on the otlp corpus. See bench/xlang/go/README.md.
package main

import (
	genotlp "github.com/mingley/pure-protobuf/bench/xlang/go/gen/otlp"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

func main() {
	xlang.Run("go_vt", "vt", func(_, _, message string) (xlang.Session, error) {
		return xlang.NewVTSession(message, genotlp.VTExcludedFiles)
	})
}
