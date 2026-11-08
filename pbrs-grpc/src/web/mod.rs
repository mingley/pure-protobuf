//! gRPC-Web support.
//!
//! Enable the `grpc-web` feature to serve binary gRPC-Web requests over the
//! existing HTTP/2 transport. Native gRPC content types and behavior are
//! unchanged when the feature is off.

mod frame;

pub use frame::CorsPolicy;
pub(crate) use frame::{
    Mode, TextEncoder, WebContext, deadline_response, is_supported_content_type,
    read_one_text_message, send_cors_preflight, send_encoding_rejection, send_frame,
    send_http_rejection, send_ok_headers, send_trailers, send_trailers_only,
    try_send_trailers_only,
};
