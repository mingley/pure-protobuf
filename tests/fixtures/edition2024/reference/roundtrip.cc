#include <iomanip>
#include <iostream>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "closed_enum.pb.h"
#include "google/protobuf/descriptor.h"
#include "google/protobuf/dynamic_message.h"
#include "google/protobuf/stubs/common.h"
#include "google/protobuf/unknown_field_set.h"

namespace {

static_assert(GOOGLE_PROTOBUF_VERSION == 7036001);

int Digit(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  throw std::invalid_argument("invalid input hex");
}

std::string FromHex(const std::string& hex) {
  if (hex.size() % 2 != 0) throw std::invalid_argument("odd input hex");
  std::string result;
  for (size_t i = 0; i < hex.size(); i += 2) {
    result.push_back(static_cast<char>(Digit(hex[i]) * 16 + Digit(hex[i + 1])));
  }
  return result;
}

std::string ToHex(const std::string& bytes) {
  std::ostringstream result;
  for (unsigned char byte : bytes) {
    result << std::hex << std::setw(2) << std::setfill('0')
           << static_cast<unsigned int>(byte);
  }
  return result.str();
}

void Run(const std::string& name, const std::string& input_hex) {
  const std::string input = FromHex(input_hex);
  cg14::reference::Cases generated;
  google::protobuf::DynamicMessageFactory factory;
  std::unique_ptr<google::protobuf::Message> dynamic(
      factory.GetPrototype(cg14::reference::Cases::descriptor())->New());
  if (dynamic_cast<cg14::reference::Cases*>(dynamic.get()) != nullptr) {
    throw std::runtime_error("dynamic parser delegated to generated class");
  }
  if (!generated.ParseFromString(input) || !dynamic->ParseFromString(input)) {
    throw std::runtime_error("reference parse failed: " + name);
  }

  std::string generated_wire;
  std::string dynamic_wire;
  if (!generated.SerializeToString(&generated_wire) ||
      !dynamic->SerializeToString(&dynamic_wire)) {
    throw std::runtime_error("reference serialize failed: " + name);
  }

  const auto& unknown = generated.unknown_fields();
  std::cout << name << " input=" << input_hex
            << " generated=" << ToHex(generated_wire)
            << " dynamic=" << ToHex(dynamic_wire)
            << " packed=[";
  for (int i = 0; i < generated.packed_values_size(); ++i) {
    if (i != 0) std::cout << ",";
    std::cout << generated.packed_values(i);
  }
  std::cout << "] expanded=[";
  for (int i = 0; i < generated.expanded_values_size(); ++i) {
    if (i != 0) std::cout << ",";
    std::cout << generated.expanded_values(i);
  }
  std::cout << "] map_size=" << generated.enum_map().size() << " unknown=[";
  for (int i = 0; i < unknown.field_count(); ++i) {
    if (i != 0) std::cout << ",";
    const auto& field = unknown.field(i);
    std::cout << field.number() << ":";
    if (field.type() == google::protobuf::UnknownField::TYPE_VARINT) {
      std::cout << field.varint();
    } else if (field.type() ==
               google::protobuf::UnknownField::TYPE_LENGTH_DELIMITED) {
      std::cout << ToHex(std::string(field.length_delimited()));
    } else {
      throw std::runtime_error("unexpected unknown wire type: " + name);
    }
  }
  std::cout << "]" << std::endl;
  if (generated_wire != dynamic_wire ||
      unknown.field_count() !=
          dynamic->GetReflection()->GetUnknownFields(*dynamic).field_count()) {
    throw std::runtime_error("generated/dynamic mismatch: " + name);
  }
}

}  // namespace

int main() {
  const auto* descriptor = cg14::reference::Cases::descriptor();
  if (!descriptor->FindFieldByName("packed_values")->is_packed() ||
      descriptor->FindFieldByName("expanded_values")->is_packed() ||
      !descriptor->FindFieldByName("packed_values")
           ->enum_type()
           ->is_closed() ||
      !descriptor->FindFieldByName("enum_map")
           ->message_type()
           ->map_value()
           ->enum_type()
           ->is_closed()) {
    throw std::runtime_error("reference schema features were not resolved");
  }
  const std::string negative = "ffffffffffffffffff01";
  const std::vector<std::pair<std::string, std::string>> cases = {
      {"packed_negative", "0a0c00" + negative + "01"},
      {"unpacked_negative", "08" + negative},
      {"packed_highbit32", "0a0700808080800801"},
      {"unpacked_highbit32", "088080808008"},
      {"packed_highbit64", "0a0700828080801001"},
      {"mixed", "200708010a0c0200" + negative + "08030a02010220080800"},
      {"expanded_mixed", "1203000201100310010a020001"},
      {"map_mixed", "1a04080110021a04080310011a0d080110" + negative},
      {"map_highbit64", "1a080801108280808010"},
  };
  for (const auto& [name, input] : cases) Run(name, input);
}
