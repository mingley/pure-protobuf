// Command harness-go-vt-google-messages times the vtprotobuf generated
// fast-path peer on the google-messages corpus. See bench/xlang/go/README.md.
package main

import (
	gengooglemessages "github.com/mingley/pure-protobuf/bench/xlang/go/gen/google-messages"

	"github.com/mingley/pure-protobuf/bench/xlang/go/xlang"
)

func main() {
	xlang.Run("go_vt", "vt", func(_, _, message string) (xlang.Session, error) {
		return xlang.NewVTSession(message, gengooglemessages.VTExcludedFiles)
	})
}
