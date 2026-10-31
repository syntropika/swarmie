#![cfg(feature = "tls")]
use swarmie_permissions::transport::{ManagementStream, TransportIdentity};
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

#[tokio::test]
async fn real_mutual_tls_agrees_on_exporter_and_fences_closed_attachments() {
    let client = TransportIdentity::generate();
    let server = TransportIdentity::generate();
    let (a, b) = duplex(8192);
    let (a, b) = tokio::join!(
        ManagementStream::connect(a, &client, server.public()),
        ManagementStream::accept(b, &server, client.public())
    );
    let mut a = a.unwrap();
    let mut b = b.unwrap();
    let proof = a.binding();
    assert_eq!(proof.exporter(), b.binding().exporter());
    assert_eq!(proof.peer(), &server.public());
    assert_eq!(b.binding().peer(), &client.public());
    a.write_all(b"synthetic-sensitive-channel-fixture")
        .await
        .unwrap();
    a.flush().await.unwrap();
    let mut data = [0; 35];
    b.read_exact(&mut data).await.unwrap();
    assert_eq!(&data, b"synthetic-sensitive-channel-fixture");
    drop(a);
    assert!(!proof.is_live());
}
#[tokio::test]
async fn either_wrong_endpoint_pin_refuses_the_handshake() {
    let client = TransportIdentity::generate();
    let server = TransportIdentity::generate();
    let foreign = TransportIdentity::generate();
    for wrong_client in [false, true] {
        let (a, b) = duplex(8192);
        let (a, b) = tokio::join!(
            ManagementStream::connect(
                a,
                &client,
                if wrong_client {
                    server.public()
                } else {
                    foreign.public()
                }
            ),
            ManagementStream::accept(
                b,
                &server,
                if wrong_client {
                    foreign.public()
                } else {
                    client.public()
                }
            )
        );
        assert!(a.is_err() || b.is_err());
    }
}
