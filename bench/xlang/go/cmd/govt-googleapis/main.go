// Command harness-go-vt-googleapis times the vtprotobuf generated
// fast-path peer on the googleapis corpus. See bench/xlang/go/README.md.
package main

import (
	gengoogleapis "github.com/mingley/pure-protobuf/bench/xlang/go/gen/googleapis"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

func main() {
	xlang.Run("go_vt", "vt", func(_, _, message string) (xlang.Session, error) {
		return xlang.NewVTSession(message, gengoogleapis.VTExcludedFiles)
	})
}
