#![cfg(feature = "tls")]
use swarmie_permissions::transport::{BootstrapStream, ManagementStream, TransportIdentity};
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

#[tokio::test]
async fn bootstrap_authenticates_the_actual_unknown_client_and_pinned_target() {
    let client = TransportIdentity::generate();
    let target = TransportIdentity::generate();
    let offer = [7; 16];
    let (a, b) = duplex(8192);
    let (client_stream, target_stream) = tokio::join!(
        BootstrapStream::connect(a, &client, target.public(), offer),
        BootstrapStream::accept(b, &target, offer),
    );
    let mut client_stream = client_stream.unwrap();
    let mut target_stream = target_stream.unwrap();
    let binding = client_stream.binding();
    assert_eq!(binding.exporter(), target_stream.binding().exporter());
    assert_eq!(binding.peer(), &target.public());
    assert_eq!(target_stream.binding().peer(), &client.public());
    assert_eq!(binding.offer(), &offer);
    client_stream.write_all(b"proof").await.unwrap();
    client_stream.flush().await.unwrap();
    let mut proof = [0; 5];
    target_stream.read_exact(&mut proof).await.unwrap();
    assert_eq!(&proof, b"proof");
    drop(client_stream);
    assert!(!binding.is_live());
}

#[tokio::test]
async fn management_and_bootstrap_cannot_be_interchanged() {
    let client = TransportIdentity::generate();
    let target = TransportIdentity::generate();
    let (a, b) = duplex(8192);
    let (client_result, target_result) = tokio::join!(
        ManagementStream::connect(a, &client, target.public()),
        BootstrapStream::accept(b, &target, [1; 16]),
    );
    assert!(client_result.is_err());
    assert!(target_result.is_err());
    let (a, b) = duplex(8192);
    let (client_result, target_result) = tokio::join!(
        BootstrapStream::connect(a, &client, target.public(), [1; 16]),
        ManagementStream::accept(b, &target, client.public()),
    );
    assert!(client_result.is_err());
    assert!(target_result.is_err());
}

#[tokio::test]
async fn wrong_qr_target_pin_fails_and_parallel_clients_have_independent_evidence() {
    let first = TransportIdentity::generate();
    let second = TransportIdentity::generate();
    let target = TransportIdentity::generate();
    let (a, b) = duplex(8192);
    let (wrong, rejected) = tokio::join!(
        BootstrapStream::connect(a, &first, second.public(), [1; 16]),
        BootstrapStream::accept(b, &target, [1; 16]),
    );
    assert!(wrong.is_err());
    assert!(rejected.is_err());
    let (a, b) = duplex(8192);
    let (c, d) = duplex(8192);
    let (first_stream, first_peer, second_stream, second_peer) = tokio::join!(
        BootstrapStream::connect(a, &first, target.public(), [1; 16]),
        BootstrapStream::accept(b, &target, [1; 16]),
        BootstrapStream::connect(c, &second, target.public(), [2; 16]),
        BootstrapStream::accept(d, &target, [2; 16]),
    );
    let first_peer = first_peer.unwrap();
    let second_peer = second_peer.unwrap();
    assert_eq!(first_peer.binding().peer(), &first.public());
    assert_eq!(second_peer.binding().peer(), &second.public());
    assert_ne!(
        first_peer.binding().exporter(),
        second_peer.binding().exporter()
    );
    assert_eq!(
        first_stream.unwrap().binding().exporter(),
        first_peer.binding().exporter()
    );
    assert_eq!(
        second_stream.unwrap().binding().exporter(),
        second_peer.binding().exporter()
    );
}
