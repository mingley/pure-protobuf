// Command harness-go-vt-xds times the vtprotobuf generated
// fast-path peer on the xds corpus. See bench/xlang/go/README.md.
package main

import (
	genxds "github.com/mingley/pure-protobuf/bench/xlang/go/gen/xds"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

func main() {
	xlang.Run("go_vt", "vt", func(_, _, message string) (xlang.Session, error) {
		return xlang.NewVTSession(message, genxds.VTExcludedFiles)
	})
}
