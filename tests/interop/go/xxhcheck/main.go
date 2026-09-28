package main

import (
	"fmt"

	xxhash "github.com/cespare/xxhash/v2"
)

func main() {
	inputs := []string{"", "a", "abc", "10.0.0.1:80_0", "10.0.0.1:80_1", "session-42", "x-request-id"}
	for _, s := range inputs {
		fmt.Printf("seed0 %q %d\n", s, xxhash.Sum64String(s))
	}
	var d xxhash.Digest
	d.ResetWithSeed(0x1234_5678_9abc_def0)
	d.WriteString("10.0.0.1:80")
	fmt.Printf("seeded %d\n", d.Sum64())
	d.ResetWithSeed(0x1234_5678_9abc_def0)
	d.WriteString("10.0.0.2:80")
	fmt.Printf("seeded %d\n", d.Sum64())
}
