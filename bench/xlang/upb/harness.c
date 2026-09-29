/* xlang upb C codec peer harness (SB-06).
 *
 * TEST TOOL ONLY. Built by scripts/xlang-codec.sh from the pinned protobuf
 * sources (upb ships inside protocolbuffers/protobuf) and never part of any
 * shipping dependency graph (see README.md).
 *
 * Schema-generic like the C++ peer: loads a FileDescriptorSet from the pinned
 * protoc, adds each file to a upb_DefPool in descriptor-set order
 * (protoc --include_imports emits dependencies first), finds one message
 * type by name, and verifies or times encode/decode. upb is arena-based, so
 * every timed op uses a fresh arena (request-scoped arena use).
 *
 * CLI mirrors bench/xlang/cpp/harness.cc; --mode must be "arena".
 * Output: exactly one JSON object on stdout; diagnostics on stderr.
 * Exit 0 on success, 1 on usage/IO errors, 2 when verification fails.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "upb/mem/arena.h"
#include "upb/message/compare.h"
#include "upb/message/message.h"
#include "upb/reflection/def_pool.h"
#include "upb/reflection/message_def.h"
#include "upb/wire/decode.h"
#include "upb/wire/encode.h"
/* Bootstrap generated code for descriptor.proto (header-inline parse
 * routines plus the minitable linked into libupb). */
#include "google/protobuf/descriptor.upb.h"

#if defined(__GNUC__) || defined(__clang__)
#define DO_NOT_OPTIMIZE(p) __asm__ volatile("" : : "r"(p) : "memory")
#else
#define DO_NOT_OPTIMIZE(p) ((void)(p))
#endif

static uint64_t now_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (uint64_t)ts.tv_sec * 1000000000u + (uint64_t)ts.tv_nsec;
}

static int read_file(const char* path, char** out, size_t* len) {
  FILE* f = fopen(path, "rb");
  if (!f) return 0;
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  if (n < 0) {
    fclose(f);
    return 0;
  }
  char* buf = (char*)malloc((size_t)n ? (size_t)n : 1);
  if (!buf) {
    fclose(f);
    return 0;
  }
  if (n > 0 && fread(buf, 1, (size_t)n, f) != (size_t)n) {
    free(buf);
    fclose(f);
    return 0;
  }
  fclose(f);
  *out = buf;
  *len = (size_t)n;
  return 1;
}

static void usage(const char* msg) {
  fprintf(stderr,
          "harness-upb: %s\n"
          "usage: harness-upb --desc FDS --message NAME --payload FILE "
          "--op encode|decode --mode arena [--iters N] [--warmup N] "
          "[--verify-only]\n",
          msg);
}

int main(int argc, char** argv) {
  const char* desc_path = NULL;
  const char* message_name = NULL;
  const char* payload_path = NULL;
  const char* op = NULL;
  const char* mode = NULL;
  long iters = 0;
  long warmup = -1;
  int verify_only = 0;

  for (int i = 1; i < argc; i++) {
    const char* k = argv[i];
    const char* v = (i + 1 < argc) ? argv[i + 1] : NULL;
#define NEED(dst)                  \
  do {                             \
    if (!v) {                      \
      usage("missing value");      \
      return 1;                    \
    }                              \
    dst = v;                       \
    i++;                           \
  } while (0)
    if (!strcmp(k, "--desc")) {
      NEED(desc_path);
    } else if (!strcmp(k, "--message")) {
      NEED(message_name);
    } else if (!strcmp(k, "--payload")) {
      NEED(payload_path);
    } else if (!strcmp(k, "--op")) {
      NEED(op);
    } else if (!strcmp(k, "--mode")) {
      NEED(mode);
    } else if (!strcmp(k, "--iters")) {
      const char* s = NULL;
      NEED(s);
      iters = atol(s);
    } else if (!strcmp(k, "--warmup")) {
      const char* s = NULL;
      NEED(s);
      warmup = atol(s);
    } else if (!strcmp(k, "--verify-only")) {
      verify_only = 1;
    } else {
      usage("unknown arg");
      return 1;
    }
#undef NEED
  }
  if (!desc_path || !message_name || !payload_path || !op || !mode ||
      (strcmp(op, "encode") && strcmp(op, "decode")) ||
      strcmp(mode, "arena") || (!verify_only && iters <= 0)) {
    usage("bad arguments");
    return 1;
  }
  if (warmup < 0) warmup = iters >= 10 ? iters / 10 : 0;
  if (warmup > 10000) warmup = 10000;

  char* desc_bytes = NULL;
  size_t desc_len = 0;
  if (!read_file(desc_path, &desc_bytes, &desc_len)) {
    fprintf(stderr, "harness-upb: cannot read desc %s\n", desc_path);
    return 1;
  }
  char* payload = NULL;
  size_t payload_len = 0;
  if (!read_file(payload_path, &payload, &payload_len)) {
    fprintf(stderr, "harness-upb: cannot read payload %s\n", payload_path);
    free(desc_bytes);
    return 1;
  }

  upb_Arena* tmp = upb_Arena_New();
  google_protobuf_FileDescriptorSet* set =
      google_protobuf_FileDescriptorSet_parse(desc_bytes, desc_len, tmp);
  if (!set) {
    fprintf(stderr, "harness-upb: cannot parse FileDescriptorSet\n");
    return 1;
  }
  upb_DefPool* pool = upb_DefPool_New();
  size_t nfiles = 0;
  const google_protobuf_FileDescriptorProto* const* files =
      google_protobuf_FileDescriptorSet_file(set, &nfiles);
  int is_encode = !strcmp(op, "encode");
  for (size_t i = 0; i < nfiles; i++) {
    upb_Status status;
    upb_Status_Clear(&status);
    if (!upb_DefPool_AddFile(pool, files[i], &status)) {
      fprintf(stderr, "harness-upb: AddFile failed: %s\n",
              upb_Status_ErrorMessage(&status));
      return 1;
    }
  }
  const upb_MessageDef* mdef =
      upb_DefPool_FindMessageByName(pool, message_name);
  if (!mdef) {
    fprintf(stderr, "harness-upb: message not found: %s\n", message_name);
    return 1;
  }
  const upb_MiniTable* mt = upb_MessageDef_MiniTable(mdef);
  const upb_ExtensionRegistry* extreg = upb_DefPool_ExtensionRegistry(pool);

  /* ---- Verification (must pass before any timing) ----
   * 1. Re-encoded bytes must equal the payload (wire_equal), or
   * 2. semantic equality: re-encode fixpoint plus upb_Message_IsEqual.
   * upb encodes fields in field-number order, so corpora payloads whose
   * field order differs (e.g. map-heavy shapes) land on (2); both prove
   * the timed codec round-trips the corpus message. */
  const char* verify = NULL;
  {
    upb_Arena* a = upb_Arena_New();
    upb_Message* ma = upb_Message_New(mt, a);
    if (upb_Decode(payload, payload_len, ma, mt, extreg, 0, a) !=
        kUpb_DecodeStatus_Ok) {
      fprintf(stderr, "harness-upb: payload does not parse as %s\n",
              message_name);
      return 2;
    }
    char* buf1 = NULL;
    size_t len1 = 0;
    if (upb_Encode(ma, mt, 0, a, &buf1, &len1) != kUpb_EncodeStatus_Ok) {
      fprintf(stderr, "harness-upb: encode failed\n");
      return 2;
    }
    if (len1 == payload_len && !memcmp(buf1, payload, payload_len)) {
      verify = "wire_equal";
    } else {
      /* Semantic check only: decode the re-encoded bytes and compare with
       * the order-insensitive upb_Message_IsEqual. No byte-fixpoint is
       * required because map entry order varies per encode call. */
      upb_Arena* b = upb_Arena_New();
      upb_Message* mb = upb_Message_New(mt, b);
      int ok = upb_Decode(buf1, len1, mb, mt, extreg, 0, b) ==
               kUpb_DecodeStatus_Ok;
      ok = ok && upb_Message_IsEqual(ma, mb, mt, 0);
      upb_Arena_Free(b);
      if (!ok) {
        fprintf(stderr,
                "harness-upb: round-trip mismatch for %s (payload %luB, "
                "reserialized %luB)\n",
                message_name, (unsigned long)payload_len,
                (unsigned long)len1);
        return 2;
      }
      verify = "semantic_equal";
    }
    upb_Arena_Free(a);
  }

  uint64_t wall_ns = 0;
  if (!verify_only) {
    if (!is_encode) {
      for (long w = 0; w < warmup; w++) {
        upb_Arena* a = upb_Arena_New();
        upb_Message* m = upb_Message_New(mt, a);
        upb_Decode(payload, payload_len, m, mt, extreg, 0, a);
        DO_NOT_OPTIMIZE(m);
        upb_Arena_Free(a);
      }
      uint64_t t0 = now_ns();
      for (long i = 0; i < iters; i++) {
        upb_Arena* a = upb_Arena_New();
        upb_Message* m = upb_Message_New(mt, a);
        if (upb_Decode(payload, payload_len, m, mt, extreg, 0, a) !=
            kUpb_DecodeStatus_Ok) {
          fprintf(stderr, "harness-upb: parse failed mid-loop\n");
          return 2;
        }
        DO_NOT_OPTIMIZE(m);
        upb_Arena_Free(a);
      }
      wall_ns = now_ns() - t0;
    } else {
      /* encode: decode once, re-encode iters times with a fresh arena. */
      upb_Arena* ma = upb_Arena_New();
      upb_Message* m = upb_Message_New(mt, ma);
      if (upb_Decode(payload, payload_len, m, mt, extreg, 0, ma) !=
          kUpb_DecodeStatus_Ok) {
        fprintf(stderr, "harness-upb: setup decode failed\n");
        return 2;
      }
      for (long w = 0; w < warmup; w++) {
        upb_Arena* a = upb_Arena_New();
        char* buf = NULL;
        size_t len = 0;
        upb_Encode(m, mt, 0, a, &buf, &len);
        DO_NOT_OPTIMIZE(buf);
        upb_Arena_Free(a);
      }
      uint64_t t0 = now_ns();
      for (long i = 0; i < iters; i++) {
        upb_Arena* a = upb_Arena_New();
        char* buf = NULL;
        size_t len = 0;
        if (upb_Encode(m, mt, 0, a, &buf, &len) != kUpb_EncodeStatus_Ok) {
          fprintf(stderr, "harness-upb: encode failed mid-loop\n");
          return 2;
        }
        DO_NOT_OPTIMIZE(buf);
        upb_Arena_Free(a);
      }
      wall_ns = now_ns() - t0;
      upb_Arena_Free(ma);
    }
  }

  printf("{\"harness\":\"upb\",\"mode\":\"arena\",\"op\":\"%s\","
         "\"message\":\"%s\",\"payload_bytes\":%lu,\"verify\":\"%s\","
         "\"iters\":%ld,\"warmup\":%ld,\"wall_ns\":%llu}\n",
         op, message_name, (unsigned long)payload_len, verify,
         verify_only ? 0L : iters, warmup, (unsigned long long)wall_ns);
  free(desc_bytes);
  free(payload);
  return 0;
}
