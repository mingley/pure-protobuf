# xlang C++ peer build (SB-06). Invoked by scripts/xlang-codec.sh, which sets
# up PROTOBUF_SRC (pinned, SHA-verified protobuf tree) and PEER_BUILD (the
# Release libprotobuf build). Output goes to target/xlang-codec/bin (gitignored).
ROOT := ../../..
PROTOBUF_SRC ?= $(ROOT)/third_party/protobuf
PEER_BUILD ?= $(ROOT)/target/xlang-peer-build
OUT_DIR ?= $(ROOT)/target/xlang-codec/bin

CXX ?= c++
CXXFLAGS ?= -std=c++17 -O2 -DNDEBUG
override CXXFLAGS += -I$(PROTOBUF_SRC)/src -I$(PEER_BUILD)/_deps/absl-src \
	-I$(PROTOBUF_SRC)/third_party/utf8_range

ABSL_LIBS := $(shell find $(PEER_BUILD)/_deps/absl-build -name 'libabsl_*.a' 2>/dev/null)
LIBS := $(PEER_BUILD)/libprotobuf.a $(ABSL_LIBS) \
	$(PEER_BUILD)/third_party/utf8_range/libutf8_range.a \
	$(PEER_BUILD)/third_party/utf8_range/libutf8_validity.a

# absl time needs CoreFoundation on macOS.
ifeq ($(shell uname),Darwin)
LIBS += -framework CoreFoundation
endif

BIN := $(OUT_DIR)/harness-cpp

all: $(BIN)

$(BIN): harness.cc | $(OUT_DIR)
	$(CXX) $(CXXFLAGS) -o $@ harness.cc $(LIBS) -lpthread

$(OUT_DIR):
	mkdir -p $(OUT_DIR)

clean:
	rm -f $(BIN)

.PHONY: all clean
