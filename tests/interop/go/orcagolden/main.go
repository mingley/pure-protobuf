// ORCA wire golden: marshal a known OrcaLoadReport with the pinned
// grpc-go/xds types (encode mode), or unmarshal hex bytes and print
// the decoded fields (decode mode). Usage:
//
//	go run ./orcagolden -mode=encode
//	go run ./orcagolden -mode=decode <hex>
package main

import (
	"encoding/hex"
	"flag"
	"fmt"
	"os"

	v3orcapb "github.com/cncf/xds/go/xds/data/orca/v3"
	"google.golang.org/protobuf/proto"
)

func main() {
	mode := flag.String("mode", "encode", "encode or decode")
	flag.Parse()
	switch *mode {
	case "encode":
		report := &v3orcapb.OrcaLoadReport{
			CpuUtilization:         0.5,
			MemUtilization:         0.25,
			RequestCost:            map[string]float64{"bytes": 3487},
			Utilization:            map[string]float64{"gpu": 0.3},
			RpsFractional:          100.5,
			Eps:                    2,
			NamedMetrics:           map[string]float64{"queue": 0.9},
			ApplicationUtilization: 0.8,
		}
		bytes, err := proto.Marshal(report)
		if err != nil {
			panic(err)
		}
		fmt.Println(hex.EncodeToString(bytes))
	case "decode":
		if flag.NArg() != 1 {
			panic("decode needs one hex arg")
		}
		bytes, err := hex.DecodeString(flag.Arg(0))
		if err != nil {
			panic(err)
		}
		var report v3orcapb.OrcaLoadReport
		if err := proto.Unmarshal(bytes, &report); err != nil {
			panic(err)
		}
		fmt.Printf("cpu=%v mem=%v rps=%v eps=%v app=%v costs=%v utils=%v named=%v\n",
			report.CpuUtilization, report.MemUtilization, report.RpsFractional,
			report.Eps, report.ApplicationUtilization, report.RequestCost,
			report.Utilization, report.NamedMetrics)
	default:
		panic("unknown mode")
	}
	os.Exit(0)
}
