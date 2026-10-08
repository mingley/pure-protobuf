//! Prost messages on the native transport for matched load comparisons.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    clippy::too_many_arguments,
    missing_docs,
    reason = "benchmark load operations"
)]
use crate::{
    LoadShape, VerifyLoadCompression, bidi_prost_request, load, native_prost_gen, process,
    prost_gen, prost_payload_len, stream_prost_req, unary_prost_request, upload_prost_req,
};
use pbrs_grpc::codec::prost::Streaming;
use pbrs_grpc::{Channel, Request, Response, Status};

pub fn router() -> pbrs_grpc::Router {
    pbrs_grpc::Server::new(native_prost_gen::TestServiceServer::new(Interop)).into_router()
}

pub async fn run(
    load_gen: &load::LoadGenerator,
    channel: Channel,
    shape: LoadShape,
    req_bytes: usize,
    resp_bytes: usize,
    stream_msgs: u32,
    gzip: bool,
) -> Result<load::LoadRecord, String> {
    match shape {
        LoadShape::Unary => {
            let client = native_prost_gen::TestServiceClient::new(channel);
            Ok(load_gen
                .run(move || {
                    let client = client.clone();
                    async move {
                        let resp = client
                            .unary_call(pbrs_grpc::Request::new(unary_prost_request(
                                req_bytes, resp_bytes,
                            )))
                            .await
                            .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                            .verify_load_compression(gzip)?;
                        let got = prost_payload_len(&resp.into_inner().payload);
                        if got != resp_bytes {
                            return Err(load::RpcCallError::Other(format!(
                                "response size mismatch: got {got}, want {resp_bytes}"
                            )));
                        }
                        Ok(())
                    }
                })
                .await)
        }
        LoadShape::ServerStream => {
            let client = native_prost_gen::TestServiceClient::new(channel);
            let mut template = stream_prost_req(stream_msgs as i32, resp_bytes as i32);
            if req_bytes > 0 {
                template.payload = Some(prost_gen::Payload {
                    body: vec![0u8; req_bytes],
                    ..Default::default()
                });
            }
            Ok(load_gen
                .run(move || {
                    let client = client.clone();
                    let template = template.clone();
                    async move {
                        let mut inbound = client
                            .streaming_output_call(pbrs_grpc::Request::new(template))
                            .await
                            .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                            .verify_load_compression(gzip)?
                            .into_inner();
                        let mut n = 0u32;
                        while let Some(msg) = inbound
                            .message()
                            .await
                            .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                        {
                            if prost_payload_len(&msg.payload) != resp_bytes {
                                return Err(load::RpcCallError::Other(format!(
                                    "stream msg {n} size mismatch: got {}, want {resp_bytes}",
                                    prost_payload_len(&msg.payload)
                                )));
                            }
                            n += 1;
                        }
                        if n != stream_msgs {
                            return Err(load::RpcCallError::Other(format!(
                                "stream count mismatch: got {n}, want {stream_msgs}"
                            )));
                        }
                        Ok(())
                    }
                })
                .await)
        }
        LoadShape::ClientStream => {
            let client = native_prost_gen::TestServiceClient::new(channel);
            let template = upload_prost_req(req_bytes as i32);
            let want = process::upload_want_bytes(stream_msgs as i32, req_bytes as i32);
            Ok(load_gen
                .run(move || {
                    let client = client.clone();
                    let template = template.clone();
                    async move {
                        let (tx, call) = client.streaming_input_call(pbrs_grpc::Request::new(()));
                        let send = async move {
                            for i in 0..stream_msgs {
                                tx.send(template.clone()).await.map_err(|e| {
                                    load::RpcCallError::Other(format!("upload send msg {i}: {e}"))
                                })?;
                            }
                            tx.close();
                            Ok::<(), load::RpcCallError>(())
                        };
                        let (send_res, resp_res) = tokio::join!(send, call);
                        send_res?;
                        let got = resp_res
                            .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                            .verify_load_compression(gzip)?
                            .into_inner()
                            .aggregated_payload_size;
                        if got != want {
                            return Err(load::RpcCallError::Other(format!(
                                "upload aggregate mismatch: got {got}, want {want}"
                            )));
                        }
                        Ok(())
                    }
                })
                .await)
        }
        LoadShape::Bidi | LoadShape::BidiPipelined => {
            let client = native_prost_gen::TestServiceClient::new(channel);
            let template = bidi_prost_request(req_bytes, resp_bytes);
            Ok(load_gen
                .run(move || {
                    let client = client.clone();
                    let template = template.clone();
                    async move {
                        let (tx, call) = client.full_duplex_call(pbrs_grpc::Request::new(()));
                        let tx = if stream_msgs == 0 {
                            tx.close();
                            None
                        } else {
                            tx.send(template.clone()).await.map_err(|e| {
                                load::RpcCallError::Other(format!("bidi first request: {e}"))
                            })?;
                            Some(tx)
                        };
                        let mut inbound = call
                            .await
                            .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                            .verify_load_compression(gzip)?
                            .into_inner();
                        let Some(tx) = tx else {
                            if inbound
                                .message()
                                .await
                                .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                                .is_some()
                            {
                                return Err(load::RpcCallError::Other(
                                    "empty bidi received a message".into(),
                                ));
                            }
                            return Ok(());
                        };
                        if shape == LoadShape::BidiPipelined {
                            let send = async move {
                                for i in 1..stream_msgs {
                                    tx.send(template.clone()).await.map_err(|e| {
                                        load::RpcCallError::Other(format!("pipeline send {i}: {e}"))
                                    })?;
                                }
                                tx.close();
                                Ok::<(), load::RpcCallError>(())
                            };
                            let receive = async {
                                let mut received = 0;
                                while let Some(reply) = inbound
                                    .message()
                                    .await
                                    .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                                {
                                    if prost_payload_len(&reply.payload) != resp_bytes {
                                        return Err(load::RpcCallError::Other(
                                            "pipeline response size mismatch".into(),
                                        ));
                                    }
                                    received += 1;
                                }
                                if received != stream_msgs {
                                    return Err(load::RpcCallError::Other(format!(
                                        "pipeline received {received}, expected {stream_msgs}"
                                    )));
                                }
                                Ok::<(), load::RpcCallError>(())
                            };
                            let (sent, received) = tokio::join!(send, receive);
                            sent?;
                            received?;
                            return Ok(());
                        }
                        for i in 0..stream_msgs {
                            if i != 0 {
                                tx.send(template.clone()).await.map_err(|e| {
                                    load::RpcCallError::Other(format!("bidi send pair {i}: {e}"))
                                })?;
                            }
                            let reply = inbound
                                .message()
                                .await
                                .map_err(|e| {
                                    load::RpcCallError::Other(format!("bidi recv pair {i}: {e}"))
                                })?
                                .ok_or_else(|| {
                                    load::RpcCallError::Other(format!(
                                        "bidi stream ended early at pair {i}"
                                    ))
                                })?;
                            if prost_payload_len(&reply.payload) != resp_bytes {
                                return Err(load::RpcCallError::Other(format!(
                                    "bidi pair {i} size mismatch: got {}, want {resp_bytes}",
                                    prost_payload_len(&reply.payload)
                                )));
                            }
                        }
                        tx.close();
                        if inbound
                            .message()
                            .await
                            .map_err(|e| load::RpcCallError::Other(e.to_string()))?
                            .is_some()
                        {
                            return Err(load::RpcCallError::Other(
                                "bidi unexpected extra message after close".to_string(),
                            ));
                        }
                        Ok(())
                    }
                })
                .await)
        }
    }
}

pub struct Interop;

impl native_prost_gen::TestService for Interop {
    async fn empty_call(
        &self,
        _: Request<prost_gen::Empty>,
    ) -> Result<Response<prost_gen::Empty>, Status> {
        Ok(Response::new(prost_gen::Empty {}))
    }

    async fn unary_call(
        &self,
        request: Request<prost_gen::SimpleRequest>,
    ) -> Result<Response<prost_gen::SimpleResponse>, Status> {
        let n = usize::try_from(request.into_inner().response_size.max(0)).unwrap_or(0);
        Ok(Response::new(prost_gen::SimpleResponse {
            payload: Some(prost_gen::Payload {
                body: vec![0; n],
                ..Default::default()
            }),
            ..Default::default()
        }))
    }

    async fn streaming_output_call(
        &self,
        request: Request<prost_gen::StreamingOutputCallRequest>,
    ) -> Result<Response<Streaming<prost_gen::StreamingOutputCallResponse>>, Status> {
        let sizes: Vec<i32> = request
            .into_inner()
            .response_parameters
            .iter()
            .map(|p| p.size)
            .collect();
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            for size in sizes {
                let response = prost_gen::StreamingOutputCallResponse {
                    payload: Some(prost_gen::Payload {
                        body: vec![0; usize::try_from(size.max(0)).unwrap_or(0)],
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                if tx.send(response).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(stream))
    }

    async fn streaming_input_call(
        &self,
        request: Request<Streaming<prost_gen::StreamingInputCallRequest>>,
    ) -> Result<Response<prost_gen::StreamingInputCallResponse>, Status> {
        let mut inbound = request.into_inner();
        let mut total: i32 = 0;
        while let Some(item) = inbound.message().await? {
            total = total.saturating_add(
                i32::try_from(prost_payload_len(&item.payload)).unwrap_or(i32::MAX),
            );
        }
        Ok(Response::new(prost_gen::StreamingInputCallResponse {
            aggregated_payload_size: total,
        }))
    }

    async fn full_duplex_call(
        &self,
        request: Request<Streaming<prost_gen::StreamingOutputCallRequest>>,
    ) -> Result<Response<Streaming<prost_gen::StreamingOutputCallResponse>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        tokio::spawn(async move {
            loop {
                match inbound.message().await {
                    Ok(Some(msg)) => {
                        for param in &msg.response_parameters {
                            let out = prost_gen::StreamingOutputCallResponse {
                                payload: Some(prost_gen::Payload {
                                    body: vec![0; usize::try_from(param.size.max(0)).unwrap_or(0)],
                                    ..Default::default()
                                }),
                                ..Default::default()
                            };
                            if tx.send(out).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(None) => return,
                    Err(status) => {
                        tx.fail(status).await;
                        return;
                    }
                }
            }
        });
        Ok(Response::new(stream))
    }
}
