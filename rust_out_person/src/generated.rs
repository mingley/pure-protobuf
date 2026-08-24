#[path="person.u.pb.rs"]
#[allow(nonstandard_style, unused, unreachable_pub)]
#[doc(hidden)]
mod internal_do_not_use_person;

#[allow(nonstandard_style, unused)]
#[doc(inline)]
pub use internal_do_not_use_person::*;
#[allow(nonstandard_style, unused)]
pub mod __unstable {
pub static PERSON_DESCRIPTOR_INFO: ::protobuf::__internal::runtime::__unstable::DescriptorInfo = ::protobuf::__internal::runtime::__unstable::DescriptorInfo {
  descriptor: b"\n\x0cperson.proto\x12\x07\x65xample\"\x17\n\x07\x41\x64\x64ress\x12\x0c\n\x04\x63ity\x18\x01 \x01(\t\"\xa9\x02\n\x06Person\x12\n\n\x02id\x18\x01 \x01(\x05\x12\x0c\n\x04name\x18\x02 \x01(\t\x12\x12\n\x05\x65mail\x18\x03 \x01(\tH\x00\x88\x01\x01\x12\x0c\n\x04tags\x18\x04 \x03(\t\x12+\n\x06scores\x18\x05 \x03(\x0b\x32\x1b.example.Person.ScoresEntry\x12!\n\x07\x61\x64\x64ress\x18\x06 \x01(\x0b\x32\x10.example.Address\x12+\n\x06\x65xtras\x18\x10 \x03(\x0b\x32\x1b.example.Person.ExtrasEntry\x1a-\n\x0bScoresEntry\x12\x0b\n\x03key\x18\x01 \x01(\t\x12\r\n\x05value\x18\x02 \x01(\x05:\x02\x38\x01\x1a-\n\x0b\x45xtrasEntry\x12\x0b\n\x03key\x18\x01 \x01(\t\x12\r\n\x05value\x18\x02 \x01(\x05:\x02\x38\x01\x42\x08\n\x06_emailb\x06proto3",
  deps: &[
  ],
};
}
