// Package xlang implements the shared driver logic for the xlang Go codec
// peer harnesses (SB-07).
//
// TEST TOOL ONLY. Like the C++/C harnesses in bench/xlang, these binaries are
// built by scripts/xlang-codec.sh into target/xlang-codec/bin (gitignored)
// and are never part of any shipping dependency graph (see README.md).
//
// The CLI mirrors bench/xlang/cpp/harness.cc exactly: the driver script
// invokes every peer harness with the same flags and parses the same
// single-JSON-object stdout. Exit codes: 0 on success, 1 on usage/IO errors,
// 2 when verification fails (the driver refuses to time that cell and fails
// the run), 3 when the codec structurally cannot serve the cell (for example
// a message without vtprotobuf fast paths); the driver records exit-3 cells
// as not_run with the stderr reason without failing the run.
package xlang

import (
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"time"

	"google.golang.org/protobuf/encoding/prototext"
	"google.golang.org/protobuf/proto"
)

// Sinks that keep timed results observable to the compiler. Every timed op
// assigns here; nothing reads them.
var (
	sinkMsg proto.Message
	sinkLen int
	sinkErr error
)

// Args mirrors the C++ harness CLI.
type Args struct {
	Desc      string
	Message   string
	Payload   string
	Op        string // encode | decode
	Mode      string
	Iters     int64
	Warmup    int64
	Verify    bool
	Harness   string
	WantMode  string
	WarmupSet bool
}

// ParseArgs parses argv (without argv[0]) for the named harness, which only
// accepts wantMode.
func ParseArgs(argv []string, harness, wantMode string) (Args, error) {
	a := Args{Harness: harness, WantMode: wantMode, Warmup: -1}
	need := func(i *int, flag string) (string, error) {
		*i++
		if *i >= len(argv) {
			return "", fmt.Errorf("missing value for %s", flag)
		}
		return argv[*i], nil
	}
	for i := 0; i < len(argv); i++ {
		k := argv[i]
		var err error
		switch k {
		case "--desc":
			a.Desc, err = need(&i, k)
		case "--message":
			a.Message, err = need(&i, k)
		case "--payload":
			a.Payload, err = need(&i, k)
		case "--op":
			a.Op, err = need(&i, k)
		case "--mode":
			a.Mode, err = need(&i, k)
		case "--iters":
			var v string
			if v, err = need(&i, k); err == nil {
				a.Iters, err = strconv.ParseInt(v, 10, 64)
			}
		case "--warmup":
			var v string
			if v, err = need(&i, k); err == nil {
				a.Warmup, err = strconv.ParseInt(v, 10, 64)
				a.WarmupSet = true
			}
		case "--verify-only":
			a.Verify = true
		default:
			return a, fmt.Errorf("unknown arg: %s", k)
		}
		if err != nil {
			return a, err
		}
	}
	if a.Desc == "" || a.Message == "" || a.Payload == "" ||
		(a.Op != "encode" && a.Op != "decode") ||
		a.Mode != wantMode ||
		(!a.Verify && a.Iters <= 0) {
		return a, fmt.Errorf("need --desc --message --payload --op encode|decode --mode %s [--iters N] [--warmup N] [--verify-only]", wantMode)
	}
	if a.Warmup < 0 {
		if a.Iters >= 10 {
			a.Warmup = a.Iters / 10
		} else {
			a.Warmup = 0
		}
	}
	if a.Warmup > 10000 {
		a.Warmup = 10000
	}
	return a, nil
}

// Unsupported marks a cell the codec structurally cannot serve. Setup
// functions return it (wrapped with %w) so Run can exit 3 instead of 2.
type Unsupported struct{ Reason string }

func (e Unsupported) Error() string { return e.Reason }

// Session binds one message type to codec operations. Implementations must
// be safe for repeated sequential use; the message type is resolved once by
// the setup function, outside the timed loops.
type Session interface {
	// NewMessage allocates a fresh message of the bound type.
	NewMessage() proto.Message
	// Parse decodes b into m with full validation.
	Parse(m proto.Message, b []byte) error
	// MarshalAppend serializes m (the timed encode form), appending to buf
	// so timed loops reuse one backing array like the C++ harness.
	MarshalAppend(m proto.Message, buf []byte) ([]byte, error)
	// MarshalDeterministic serializes m with deterministic field ordering.
	MarshalDeterministic(m proto.Message) ([]byte, error)
}

// Report is the single JSON object printed on stdout.
type Report struct {
	Harness      string `json:"harness"`
	Mode         string `json:"mode"`
	Op           string `json:"op"`
	Message      string `json:"message"`
	PayloadBytes int    `json:"payload_bytes"`
	Verify       string `json:"verify"`
	Iters        int64  `json:"iters"`
	Warmup       int64  `json:"warmup"`
	WallNs       uint64 `json:"wall_ns"`
}

// Run is the harness main loop. setup resolves (op, desc, message) to a
// Session; desc may be ignored by generated-code peers whose types are
// compiled in.
func Run(harness, wantMode string, setup func(op, desc, message string) (Session, error)) {
	if err := run(harness, wantMode, setup); err != nil {
		code := 1
		var un Unsupported
		if errIsUnsupported(err, &un) {
			code = 3
		} else if errIsVerify(err) {
			code = 2
		}
		fmt.Fprintf(os.Stderr, "harness-%s: %v\n", harness, err)
		os.Exit(code)
	}
}

func errIsUnsupported(err error, un *Unsupported) bool {
	for err != nil {
		if u, ok := err.(Unsupported); ok {
			*un = u
			return true
		}
		if u, ok := err.(*Unsupported); ok {
			*un = *u
			return true
		}
		type unwrapper interface{ Unwrap() error }
		if w, ok := err.(unwrapper); ok {
			err = w.Unwrap()
		} else {
			return false
		}
	}
	return false
}

type verifyError struct{ msg string }

func (e verifyError) Error() string { return e.msg }

func errIsVerify(err error) bool {
	_, ok := err.(verifyError)
	if ok {
		return true
	}
	p, ok := err.(*verifyError)
	return ok && p != nil
}

func vfail(format string, args ...any) error {
	return verifyError{msg: fmt.Sprintf(format, args...)}
}

func run(harness, wantMode string, setup func(op, desc, message string) (Session, error)) error {
	args, err := ParseArgs(os.Args[1:], harness, wantMode)
	if err != nil {
		return err
	}
	payload, err := os.ReadFile(args.Payload)
	if err != nil {
		return fmt.Errorf("cannot read payload %s: %w", args.Payload, err)
	}
	sess, err := setup(args.Op, args.Desc, args.Message)
	if err != nil {
		return err
	}

	// ---- Verification (must pass before any timing) ----
	// 1. Plain round-trip bytes must equal the payload, or
	// 2. deterministically-serialized bytes must equal it (map order), or
	// 3. semantic equality: re-parse plus a deterministic-serialization
	//    fixpoint and identical text format (the C++ DebugString check).
	verify, err := verifySession(sess, payload)
	if err != nil {
		return err
	}

	var wallNs uint64
	if !args.Verify {
		// Settle the heap after setup so GC noise stays out of the window.
		runtime.GC()
		if args.Op == "decode" {
			for w := int64(0); w < args.Warmup; w++ {
				m := sess.NewMessage()
				sinkErr = sess.Parse(m, payload)
				sinkMsg = m
			}
			t0 := time.Now()
			for i := int64(0); i < args.Iters; i++ {
				m := sess.NewMessage()
				if err := sess.Parse(m, payload); err != nil {
					return vfail("parse failed mid-loop: %v", err)
				}
				sinkMsg = m
			}
			wallNs = uint64(time.Since(t0).Nanoseconds())
		} else {
			m := sess.NewMessage()
			if err := sess.Parse(m, payload); err != nil {
				return vfail("setup decode failed: %v", err)
			}
			buf := make([]byte, 0, len(payload)+16)
			for w := int64(0); w < args.Warmup; w++ {
				out, err := sess.MarshalAppend(m, buf[:0])
				sinkErr = err
				sinkLen = len(out)
			}
			t0 := time.Now()
			for i := int64(0); i < args.Iters; i++ {
				out, err := sess.MarshalAppend(m, buf[:0])
				if err != nil {
					return vfail("serialize failed mid-loop: %v", err)
				}
				sinkLen = len(out)
			}
			wallNs = uint64(time.Since(t0).Nanoseconds())
		}
	}

	iters := args.Iters
	if args.Verify {
		iters = 0
	}
	rep := Report{
		Harness: harness, Mode: args.Mode, Op: args.Op,
		Message: args.Message, PayloadBytes: len(payload),
		Verify: verify, Iters: iters, Warmup: args.Warmup, WallNs: wallNs,
	}
	return json.NewEncoder(os.Stdout).Encode(rep)
}

func verifySession(sess Session, payload []byte) (string, error) {
	a := sess.NewMessage()
	if err := sess.Parse(a, payload); err != nil {
		return "", vfail("payload does not parse: %v", err)
	}
	s1, err := sess.MarshalAppend(a, nil)
	if err != nil {
		return "", vfail("serialize failed: %v", err)
	}
	if string(s1) == string(payload) {
		return "wire_equal", nil
	}
	sdet, err := sess.MarshalDeterministic(a)
	if err != nil {
		return "", vfail("deterministic serialize failed: %v", err)
	}
	if string(sdet) == string(payload) {
		return "wire_equal_deterministic", nil
	}
	b := sess.NewMessage()
	if err := sess.Parse(b, s1); err != nil {
		return "", vfail("re-parse failed: %v", err)
	}
	s3det, err := sess.MarshalDeterministic(b)
	if err != nil {
		return "", vfail("deterministic re-serialize failed: %v", err)
	}
	ta, errA := prototext.MarshalOptions{Multiline: false}.Marshal(a)
	tb, errB := prototext.MarshalOptions{Multiline: false}.Marshal(b)
	if errA != nil || errB != nil || string(s3det) != string(sdet) || string(ta) != string(tb) {
		return "", vfail("round-trip mismatch (payload %dB, reserialized %dB)", len(payload), len(s1))
	}
	return "semantic_equal", nil
}
