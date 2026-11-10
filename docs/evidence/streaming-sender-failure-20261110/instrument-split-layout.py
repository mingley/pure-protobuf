from pathlib import Path
p = Path("pbrs-grpc/src/request.rs")
s = p.read_text()
before = "    pub(crate) fn new(\n        cancel: crate::rpc_cancel::RpcCancel,\n        fut: Pin<Box<dyn Future<Output = Result<T, Status>> + Send>>,\n    ) -> Self {\n"
after = before + '        if std::env::var_os("PBRS_FUTURE_LAYOUT").is_some() {\n            eprintln!("PBRS_SPLIT_CALL_BYTES={}", std::mem::size_of_val(fut.as_ref().get_ref()));\n        }\n'
assert s.count(before) == 1
p.write_text(s.replace(before, after))
