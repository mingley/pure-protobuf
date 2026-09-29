// xlang C++ protobuf codec peer harness (SB-06).
//
// TEST TOOL ONLY. This harness is built by scripts/xlang-codec.sh from the
// pinned protobuf sources and is never part of any shipping dependency graph
// (no Cargo manifest references it; see README.md).
//
// The harness is schema-generic: it loads a FileDescriptorSet produced by the
// pinned protoc from the SB-05 corpora, finds one message type by name, and
// either verifies wire/semantic equality for a payload or times encode/decode
// loops. Every timed cell is verified first; the driver script refuses to
// time a cell whose verification failed.
//
// Modes:
//   heap  - plain new/delete per decode op (encode reuses one message).
//   arena - google::protobuf::Arena; the arena is reset after every decode
//           op (request-scoped arena use) and owns the encode message.
//
// Output: exactly one JSON object on stdout per invocation, e.g.
//   {"harness":"cpp","mode":"heap","op":"decode","verify":"wire_equal",
//    "iters":2000,"warmup":200,"wall_ns":1234567,"payload_bytes":507}
// Diagnostics go to stderr. Exit 0 on success, 1 on usage/IO errors,
// 2 when verification fails.

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

#include <google/protobuf/arena.h>
#include <google/protobuf/descriptor.h>
#include <google/protobuf/descriptor.pb.h>
#include <google/protobuf/dynamic_message.h>
#include <google/protobuf/io/coded_stream.h>
#include <google/protobuf/io/zero_copy_stream_impl_lite.h>
#include <google/protobuf/message.h>

namespace {

void DoNotOptimize(const void* p) {
#if defined(__GNUC__) || defined(__clang__)
  asm volatile("" : : "r"(p) : "memory");
#else
  (void)p;
#endif
}

bool ReadFile(const std::string& path, std::string* out) {
  std::ifstream in(path, std::ios::binary);
  if (!in) return false;
  std::ostringstream ss;
  ss << in.rdbuf();
  *out = ss.str();
  return true;
}

void JsonEscape(const std::string& s, std::string* out) {
  for (char c : s) {
    switch (c) {
      case '"': *out += "\\\""; break;
      case '\\': *out += "\\\\"; break;
      case '\n': *out += "\\n"; break;
      default: *out += c;
    }
  }
}

struct Args {
  std::string desc;
  std::string message;
  std::string payload;
  std::string op;    // encode | decode
  std::string mode;  // heap | arena
  long iters = 0;
  long warmup = -1;  // -1 => derive from iters
  bool verify_only = false;
};

bool ParseArgs(int argc, char** argv, Args* a, std::string* err) {
  for (int i = 1; i < argc; ++i) {
    std::string k = argv[i];
    auto need = [&](std::string* dst) {
      if (++i >= argc) return false;
      *dst = argv[i];
      return true;
    };
    if (k == "--desc") { if (!need(&a->desc)) break; }
    else if (k == "--message") { if (!need(&a->message)) break; }
    else if (k == "--payload") { if (!need(&a->payload)) break; }
    else if (k == "--op") { if (!need(&a->op)) break; }
    else if (k == "--mode") { if (!need(&a->mode)) break; }
    else if (k == "--iters") { std::string v; if (!need(&v)) break; a->iters = std::stol(v); }
    else if (k == "--warmup") { std::string v; if (!need(&v)) break; a->warmup = std::stol(v); }
    else if (k == "--verify-only") { a->verify_only = true; }
    else { *err = "unknown arg: " + k; return false; }
  }
  if (a->desc.empty() || a->message.empty() || a->payload.empty() ||
      (a->op != "encode" && a->op != "decode") ||
      (a->mode != "heap" && a->mode != "arena") ||
      (!a->verify_only && a->iters <= 0)) {
    *err = "need --desc --message --payload --op encode|decode --mode heap|arena "
           "[--iters N] [--warmup N] [--verify-only]";
    return false;
  }
  if (a->warmup < 0) a->warmup = a->iters >= 10 ? a->iters / 10 : 0;
  if (a->warmup > 10000) a->warmup = 10000;
  return true;
}

// Serialize with deterministic field ordering (map entries sorted).
bool SerializeDeterministic(const google::protobuf::Message& m, std::string* out) {
  out->clear();
  google::protobuf::io::StringOutputStream raw(out);
  google::protobuf::io::CodedOutputStream coded(&raw);
  coded.SetSerializationDeterministic(true);
  (void)m.SerializeToCodedStream(&coded);
  return !coded.HadError();
}

}  // namespace

int main(int argc, char** argv) {
  Args args;
  std::string err;
  if (!ParseArgs(argc, argv, &args, &err)) {
    std::cerr << "harness-cpp: " << err << "\n";
    return 1;
  }

  std::string desc_bytes;
  if (!ReadFile(args.desc, &desc_bytes)) {
    std::cerr << "harness-cpp: cannot read desc " << args.desc << "\n";
    return 1;
  }
  google::protobuf::FileDescriptorSet fds;
  if (!fds.ParseFromString(desc_bytes)) {
    std::cerr << "harness-cpp: cannot parse FileDescriptorSet\n";
    return 1;
  }
  google::protobuf::DescriptorPool pool;
  for (const auto& f : fds.file()) {
    if (pool.BuildFile(f) == nullptr) {
      std::cerr << "harness-cpp: cannot build file " << f.name() << "\n";
      return 1;
    }
  }
  const google::protobuf::Descriptor* md = pool.FindMessageTypeByName(args.message);
  if (md == nullptr) {
    std::cerr << "harness-cpp: message not found: " << args.message << "\n";
    return 1;
  }
  google::protobuf::DynamicMessageFactory factory(&pool);
  const google::protobuf::Message* proto = factory.GetPrototype(md);

  std::string payload;
  if (!ReadFile(args.payload, &payload)) {
    std::cerr << "harness-cpp: cannot read payload " << args.payload << "\n";
    return 1;
  }

  // ---- Verification (must pass before any timing) ----
  // 1. Plain round-trip bytes must equal the payload, or
  // 2. deterministically-serialized bytes must equal it (map order), or
  // 3. semantic equality: re-encode fixpoint plus identical DebugString.
  std::string verify;
  {
    google::protobuf::Message* a = proto->New();
    if (!a->ParseFromString(payload)) {
      std::cerr << "harness-cpp: payload does not parse as " << args.message << "\n";
      delete a;
      return 2;
    }
    std::string s1;
    if (!a->SerializeToString(&s1)) {
      std::cerr << "harness-cpp: serialize failed\n";
      delete a;
      return 2;
    }
    std::string sdet;
    if (!SerializeDeterministic(*a, &sdet)) {
      std::cerr << "harness-cpp: deterministic serialize failed\n";
      delete a;
      return 2;
    }
    if (s1 == payload) {
      verify = "wire_equal";
    } else {
      if (sdet == payload) {
        verify = "wire_equal_deterministic";
      } else {
        // Semantic check: parse the re-encoded bytes, then require a
        // deterministic-serialization fixpoint (map order is sorted, so
        // this is stable across processes) plus identical DebugString.
        google::protobuf::Message* b = proto->New();
        bool ok = b->ParseFromString(s1);
        std::string s3det;
        ok = ok && SerializeDeterministic(*b, &s3det) && s3det == sdet &&
             a->DebugString() == b->DebugString();
        delete b;
        if (!ok) {
          std::cerr << "harness-cpp: round-trip mismatch for " << args.message
                    << " (payload " << payload.size() << "B, reserialized "
                    << s1.size() << "B)\n";
          delete a;
          return 2;
        }
        verify = "semantic_equal";
      }
    }
    delete a;
  }

  uint64_t wall_ns = 0;
  if (!args.verify_only) {
    const bool arena_mode = args.mode == "arena";
    if (args.op == "decode") {
      for (long w = 0; w < args.warmup; ++w) {
        if (arena_mode) {
          google::protobuf::Arena arena;
          google::protobuf::Message* m = proto->New(&arena);
          (void)m->ParseFromString(payload);
          DoNotOptimize(m);
        } else {
          google::protobuf::Message* m = proto->New();
          (void)m->ParseFromString(payload);
          DoNotOptimize(m);
          delete m;
        }
      }
      auto t0 = std::chrono::steady_clock::now();
      if (arena_mode) {
        google::protobuf::Arena arena;
        for (long i = 0; i < args.iters; ++i) {
          google::protobuf::Message* m = proto->New(&arena);
          if (!m->ParseFromString(payload)) {
            std::cerr << "harness-cpp: parse failed mid-loop\n";
            return 2;
          }
          DoNotOptimize(m);
          arena.Reset();
        }
      } else {
        for (long i = 0; i < args.iters; ++i) {
          google::protobuf::Message* m = proto->New();
          if (!m->ParseFromString(payload)) {
            std::cerr << "harness-cpp: parse failed mid-loop\n";
            delete m;
            return 2;
          }
          DoNotOptimize(m);
          delete m;
        }
      }
      auto t1 = std::chrono::steady_clock::now();
      wall_ns = (uint64_t)std::chrono::duration_cast<std::chrono::nanoseconds>(t1 - t0).count();
    } else {
      // encode: parse once, serialize iters times.
      std::string sink;
      sink.reserve(payload.size() + 16);
      if (arena_mode) {
        google::protobuf::Arena arena;
        google::protobuf::Message* m = proto->New(&arena);
        if (!m->ParseFromString(payload)) {
          std::cerr << "harness-cpp: setup decode failed\n";
          return 2;
        }
        for (long w = 0; w < args.warmup; ++w) (void)m->SerializeToString(&sink);
        auto t0 = std::chrono::steady_clock::now();
        for (long i = 0; i < args.iters; ++i) {
          if (!m->SerializeToString(&sink)) {
            std::cerr << "harness-cpp: serialize failed mid-loop\n";
            return 2;
          }
          DoNotOptimize(sink.data());
        }
        auto t1 = std::chrono::steady_clock::now();
        wall_ns = (uint64_t)std::chrono::duration_cast<std::chrono::nanoseconds>(t1 - t0).count();
      } else {
        google::protobuf::Message* m = proto->New();
        if (!m->ParseFromString(payload)) {
          std::cerr << "harness-cpp: setup decode failed\n";
          delete m;
          return 2;
        }
        for (long w = 0; w < args.warmup; ++w) (void)m->SerializeToString(&sink);
        auto t0 = std::chrono::steady_clock::now();
        for (long i = 0; i < args.iters; ++i) {
          if (!m->SerializeToString(&sink)) {
            std::cerr << "harness-cpp: serialize failed mid-loop\n";
            delete m;
            return 2;
          }
          DoNotOptimize(sink.data());
        }
        auto t1 = std::chrono::steady_clock::now();
        wall_ns = (uint64_t)std::chrono::duration_cast<std::chrono::nanoseconds>(t1 - t0).count();
        delete m;
      }
    }
  }

  std::string emsg;
  JsonEscape(args.message, &emsg);
  std::printf("{\"harness\":\"cpp\",\"mode\":\"%s\",\"op\":\"%s\","
              "\"message\":\"%s\",\"payload_bytes\":%u,\"verify\":\"%s\","
              "\"iters\":%ld,\"warmup\":%ld,\"wall_ns\":%llu}\n",
              args.mode.c_str(), args.op.c_str(), emsg.c_str(),
              (unsigned)payload.size(), verify.c_str(),
              args.verify_only ? 0L : args.iters, args.warmup,
              (unsigned long long)wall_ns);
  return 0;
}
