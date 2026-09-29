# xlang Go peer build (SB-07). Invoked by scripts/xlang-codec.sh, which sets up
# PROTOC (the pinned, SHA-verified protoc). Fetches the pinned Go modules,
# builds the pinned protoc plugins from the module cache, generates
# protobuf-go + vtprotobuf code for every SB-05 corpus, and compiles the
# per-corpus harness binaries into OUT_DIR (target/xlang-codec/bin,
# gitignored). Per-(peer,corpus) status lands in STATUS_TSV so the driver can
# report failed cells as not_run with a reason instead of failing the run.
ROOT := ../../..
PROTOC ?= $(ROOT)/target/pinned-protoc-build/protoc
OUT_DIR ?= $(ROOT)/target/xlang-codec/bin
PLUGIN_DIR ?= $(ROOT)/target/xlang-codec/go-plugins
STATUS_TSV ?= $(ROOT)/target/xlang-codec/work/go-build.tsv
CODEGEN_TSV ?= $(ROOT)/target/xlang-codec/work/go-codegen.tsv
LOG_DIR ?= $(ROOT)/target/xlang-codec/logs
CORPORA ?= google-messages googleapis grpc-testing otlp xds
CODEGEN_STAMP ?= $(ROOT)/target/xlang-codec/go-codegen.stamp

GO ?= go
PROTOS := $(shell find $(ROOT)/bench/corpora -name '*.proto' 2>/dev/null)

PLUGINS := $(PLUGIN_DIR)/protoc-gen-go $(PLUGIN_DIR)/protoc-gen-go-vtproto

all: binaries

# Offline-reproducible builds: fetch everything first; every compile below
# then runs with GOPROXY=off so a missing module fails loudly instead of
# fetching silently.
fetch:
	$(GO) mod download all

$(PLUGIN_DIR):
	mkdir -p $(PLUGIN_DIR)

$(PLUGIN_DIR)/protoc-gen-go: go.mod go.sum | $(PLUGIN_DIR)
	GOPROXY=off $(GO) build -o $@ google.golang.org/protobuf/cmd/protoc-gen-go

$(PLUGIN_DIR)/protoc-gen-go-vtproto: go.mod go.sum | $(PLUGIN_DIR)
	GOPROXY=off $(GO) build -o $@ github.com/planetscale/vtprotobuf/cmd/protoc-gen-go-vtproto

plugins: fetch $(PLUGINS)

$(CODEGEN_STAMP): $(PROTOC) codegen.sh go.mod go.sum $(PROTOS) | plugins
	mkdir -p $(LOG_DIR) $(dir $(CODEGEN_TSV)) $(dir $(CODEGEN_STAMP))
	PATH="$(abspath $(PLUGIN_DIR)):$(PATH)" ./codegen.sh "$(abspath $(PROTOC))" "$(abspath $(LOG_DIR))" "$(abspath $(CODEGEN_TSV))" $(CORPORA)
	touch $@

codegen: $(CODEGEN_STAMP)

binaries: plugins codegen
	mkdir -p $(OUT_DIR) $(LOG_DIR) $(dir $(STATUS_TSV))
	: > "$(STATUS_TSV)"
	for c in $(CORPORA); do \
	  cg="$$(awk -F'\t' -v c="$$c" '$$1==c{print $$2}' "$(CODEGEN_TSV)")"; \
	  for spec in "go:./cmd/go-$$c:harness-go-$$c" "go_vt:./cmd/govt-$$c:harness-go-vt-$$c"; do \
	    peer="$${spec%%:*}"; rest="$${spec#*:}"; pkg="$${rest%%:*}"; bin="$${rest#*:}"; \
	    case "$$cg" in \
	      ok|"") \
	        if GOPROXY=off $(GO) build -o "$(OUT_DIR)/$$bin" "$$pkg" >"$(LOG_DIR)/build-go-$$bin.log" 2>&1; then \
	          printf '%s\t%s\tok\n' "$$peer" "$$c" >> "$(STATUS_TSV)"; \
	        else \
	          printf '%s\t%s\tbuild failed; see %s\n' "$$peer" "$$c" "$(LOG_DIR)/build-go-$$bin.log" >> "$(STATUS_TSV)"; \
	        fi;; \
	      *) \
	        printf '%s\t%s\t%s\n' "$$peer" "$$c" "$$cg" >> "$(STATUS_TSV)";; \
	    esac; \
	  done; \
	done
	if GOPROXY=off $(GO) build -o "$(OUT_DIR)/harness-hyperpb" ./cmd/hyperpb >"$(LOG_DIR)/build-go-harness-hyperpb.log" 2>&1; then \
	  printf 'go_hyperpb\t-\tok\n' >> "$(STATUS_TSV)"; \
	else \
	  printf 'go_hyperpb\t-\tbuild failed; see %s\n' "$(LOG_DIR)/build-go-harness-hyperpb.log" >> "$(STATUS_TSV)"; \
	fi
	@ok=$$(grep -c '	ok$$' "$(STATUS_TSV)" || true); total=$$(wc -l < "$(STATUS_TSV)"); echo "xlang-go: $$ok/$$total binaries built (see $(STATUS_TSV))"

clean:
	rm -f $(OUT_DIR)/harness-go-* $(OUT_DIR)/harness-hyperpb

.PHONY: all fetch plugins codegen binaries clean
