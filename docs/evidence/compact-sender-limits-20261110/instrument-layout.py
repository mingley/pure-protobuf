from pathlib import Path

p = Path("pbrs-grpc/src/request.rs")
s = p.read_text()
before = "    pub(crate) fn new(\n        cancel: crate::rpc_cancel::RpcCancel,\n        fut: Pin<Box<dyn Future<Output = Result<T, Status>> + Send>>,\n    ) -> Self {\n"
after = before + '        if std::env::var_os("PBRS_FUTURE_LAYOUT").is_some() {\n            eprintln!("PBRS_SENDER_CALL_BYTES={}", std::mem::size_of_val(fut.as_ref().get_ref()));\n        }\n'
assert s.count(before) == 1
p.write_text(s.replace(before, after))

p = Path("pbrs-grpc/src/bin/streaming-poll-probe.rs")
s = p.read_text()
before = "    let client = TestServiceClient::new(channel);\n"
after = before + """    if std::env::var_os("PBRS_FUTURE_LAYOUT").is_some() {
        report_sender("input", pbrs_grpc::testing::StreamingInputCallRequest::new());
        report_sender("request", pbrs_grpc::testing::StreamingOutputCallRequest::new());
        report_sender("response", pbrs_grpc::testing::StreamingOutputCallResponse::new());
    }
"""
assert s.count(before) == 1
s = s.replace(before, after)
s += """
fn report_sender<T: pbrs_grpc::CodecMessage>(label: &str, message: T) {
    let (sender, _receiver) = pbrs_grpc::Streaming::<T>::channel(1);
    let future = sender.send(message);
    eprintln!("PBRS_SENDER_LAYOUT label={} sender={} send_future={}", label, std::mem::size_of_val(&sender), std::mem::size_of_val(&future));
}
"""
p.write_text(s)
