    #[test]
    #[ignore]
    fn extension_read_probe() {
        use std::hint::black_box;
        use super::TlsConnectInfo;
        let mode = std::env::var("EXT_MODE").expect("mode");
        let iterations: u64 = std::env::var("EXT_ITERATIONS").expect("iterations").parse().expect("number");
        let mut request = Request::new(());
        request.remote_addr = Some("127.0.0.1:12345".parse().expect("peer"));
        request.local_addr = Some("127.0.0.1:443".parse().expect("local"));
        if mode == "mtls" {
            request.peer_identity = PeerIdentity::from_der_certs([b"peer-leaf"]);
        }
        request.set_http(None, Some(super::HttpStr::from_static(if mode == "tcp" { "http" } else { "https" })), None);
        black_box(request.extensions());
        let begin = std::time::Instant::now();
        match mode.as_str() {
            "tcp" => {
                for _ in 0..iterations {
                    black_box(black_box(&request).extensions().get::<TcpConnectInfo>().expect("TCP"));
                }
            }
            "tls" | "mtls" => {
                for _ in 0..iterations {
                    black_box(black_box(&request).extensions().get::<TlsConnectInfo>().expect("TLS"));
                }
            }
            "control" => {
                for _ in 0..iterations {
                    black_box(black_box(&request).remote_addr());
                }
            }
            _ => panic!("unknown mode"),
        }
        println!("EXT_PROBE {} {} {}", mode, iterations, begin.elapsed().as_nanos());
    }
