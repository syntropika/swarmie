//! Synthetic loopback HTTPS fixture. Its public test key is never an application trust anchor.
use std::{collections::BTreeMap, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}
pub struct Reply {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub fn certificate() -> reqwest::Certificate {
    reqwest::Certificate::from_pem(include_bytes!("vault-contract/https-ca-fixture.pem")).unwrap()
}

pub async fn serve(
    count: usize,
    handler: impl Fn(Request) -> Reply + Send + 'static,
) -> (String, JoinHandle<()>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cert = CertificateDer::from_pem_slice(include_bytes!(
        "vault-contract/https-certificate-fixture.pem"
    ))
    .unwrap();
    let key = PrivateKeyDer::from_pem_slice(include_bytes!(
        "vault-contract/https-private-key-fixture.pem"
    ))
    .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let endpoint = format!(
        "https://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    let task = tokio::spawn(async move {
        let acceptor = TlsAcceptor::from(Arc::new(config));
        for _ in 0..count {
            let (io, _) = listener.accept().await.unwrap();
            let mut socket = acceptor.accept(io).await.unwrap();
            let mut bytes = vec![];
            let mut buffer = [0; 4096];
            let end = loop {
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                assert!(bytes.len() < 1_048_576);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let text = std::str::from_utf8(&bytes[..end]).unwrap();
            let mut lines = text.lines();
            let first = lines.next().unwrap();
            let mut parts = first.split(' ');
            let method = parts.next().unwrap().to_owned();
            let path = parts.next().unwrap().to_owned();
            let headers: BTreeMap<String, String> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
                .collect();
            let length = headers
                .get("content-length")
                .map_or(0, |n| n.parse::<usize>().unwrap());
            while bytes.len() - end < length {
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                assert!(bytes.len() < 1_048_576);
            }
            let reply = handler(Request {
                method,
                path,
                headers,
                body: bytes[end..end + length].to_vec(),
            });
            let mut head = format!(
                "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n",
                reply.status,
                reply.body.len()
            );
            for (k, v) in reply.headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            socket.write_all(head.as_bytes()).await.unwrap();
            // Distinct writes exercise consumer buffering without promising a fixed network chunking.
            for chunk in reply.body.chunks(7) {
                socket.write_all(chunk).await.unwrap();
                socket.flush().await.unwrap();
            }
            socket.shutdown().await.unwrap();
        }
    });
    (endpoint, task)
}
