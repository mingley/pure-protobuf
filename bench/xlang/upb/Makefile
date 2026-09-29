# xlang upb peer build (SB-06). Invoked by scripts/xlang-codec.sh, which sets
# up PROTOBUF_SRC (pinned, SHA-verified protobuf tree; upb ships inside it)
# and PEER_BUILD (its Release libupb build). Output goes to
# target/xlang-codec/bin (gitignored).
ROOT := ../../..
PROTOBUF_SRC ?= $(ROOT)/third_party/protobuf
PEER_BUILD ?= $(ROOT)/target/xlang-peer-build
OUT_DIR ?= $(ROOT)/target/xlang-codec/bin

CC ?= cc
CFLAGS ?= -O2 -DNDEBUG
override CFLAGS += -std=c11 -I$(PROTOBUF_SRC) -I$(PROTOBUF_SRC)/upb/reflection/cmake

LIBS := $(PEER_BUILD)/libupb.a \
        $(PEER_BUILD)/third_party/utf8_range/libutf8_range.a \
        $(PEER_BUILD)/third_party/utf8_range/libutf8_validity.a

BIN := $(OUT_DIR)/harness-upb

all: $(BIN)

$(BIN): harness.c | $(OUT_DIR)
	$(CC) $(CFLAGS) -o $@ harness.c $(LIBS)

$(OUT_DIR):
	mkdir -p $(OUT_DIR)

clean:
	rm -f $(BIN)

.PHONY: all clean
