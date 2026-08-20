//! helloworld request/response using the kernel (not prost).

use protobuf::{
    Cardinality, DescriptorPool, FieldDescriptor, FieldType, IntoProxied, MessageDescriptor,
    Presence, ProtoString, Value,
};
use std::sync::{Arc, OnceLock};

fn pool() -> Arc<DescriptorPool> {
    static P: OnceLock<Arc<DescriptorPool>> = OnceLock::new();
    P.get_or_init(|| {
        let mut p = DescriptorPool::new();
        p.register_message(string_msg("helloworld.HelloRequest"));
        p.register_message(string_msg("helloworld.HelloReply"));
        Arc::new(p)
    })
    .clone()
}

fn string_msg(full: &str) -> MessageDescriptor {
    MessageDescriptor::builder(full)
        .field(FieldDescriptor::new(
            "name",
            1,
            FieldType::String,
            Cardinality::Optional,
            Presence::Implicit,
        ))
        .build()
}

protobuf::impl_generated_message!(
    HelloRequest,
    HelloRequestView,
    HelloRequestMut,
    "helloworld.HelloRequest",
    pool()
);

protobuf::impl_generated_message!(
    HelloReply,
    HelloReplyView,
    HelloReplyMut,
    "helloworld.HelloReply",
    pool()
);

impl HelloRequest {
    pub fn name(&self) -> &protobuf::ProtoStr {
        protobuf::gen_support::str_from(self.inner.get_singular(1))
    }
    pub fn set_name(&mut self, v: impl IntoProxied<ProtoString>) {
        self.inner.set(1, Value::String(v.into_proxied()));
    }
}

impl HelloReply {
    pub fn message(&self) -> &protobuf::ProtoStr {
        protobuf::gen_support::str_from(self.inner.get_singular(1))
    }
    pub fn set_message(&mut self, v: impl IntoProxied<ProtoString>) {
        self.inner.set(1, Value::String(v.into_proxied()));
    }
}
