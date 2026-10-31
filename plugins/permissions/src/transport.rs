//! Mutually pinned TLS 1.3 byte carrier. Tembra still owns RPC, objects and invocation lifecycle.
//! Raw public-key pins come from an independently authenticated enrollment/operator ceremony.
use crate::{Error, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey, pkcs8::EncodePublicKey};
use rand::rngs::OsRng;
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
    client::{
        AlwaysResolvesClientRawPublicKeys,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::{
        AlwaysResolvesServerRawPublicKeys,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
    sign::CertifiedKey,
};
use sha2::{Digest, Sha256};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};
use zeroize::Zeroizing;

mod bootstrap;

pub use bootstrap::{BootstrapBinding, BootstrapStream};

const ALPN: &[u8] = b"swarmie-management/1";
const EXPORTER: &[u8] = b"EXPORTER-Swarmie-Management-v1";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Private endpoint key. A management-only client has no HPKE or vault data key.
pub struct TransportIdentity(Zeroizing<[u8; 32]>);

impl TransportIdentity {
    pub fn generate() -> Self {
        Self(Zeroizing::new(SigningKey::generate(&mut OsRng).to_bytes()))
    }
    /// Native key custodian entry; never expose a serialized seed management endpoint.
    pub fn from_seed(seed: Zeroizing<[u8; 32]>) -> Self {
        Self(seed)
    }
    pub fn public(&self) -> [u8; 32] {
        SigningKey::from_bytes(&self.0).verifying_key().to_bytes()
    }
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        SigningKey::from_bytes(&self.0)
            .sign(message)
            .to_bytes()
            .to_vec()
    }
    fn certified(&self) -> Result<Arc<CertifiedKey>> {
        use ed25519_dalek::pkcs8::EncodePrivateKey;
        let key = SigningKey::from_bytes(&self.0);
        let encoded = key.to_pkcs8_der().map_err(|_| Error::Invalid)?;
        let private = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(encoded.as_bytes().to_vec()));
        let signer = rustls::crypto::ring::default_provider()
            .key_provider
            .load_private_key(private)
            .map_err(|_| Error::Invalid)?;
        let cert = key
            .verifying_key()
            .to_public_key_der()
            .map_err(|_| Error::Invalid)?;
        Ok(Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(cert.as_bytes().to_vec())],
            signer,
        )))
    }
}

/// Read-only proof created only after this module's actual mutual TLS handshake completes.
/// Its exporter is public binding material, never a vault key or an authorization by itself.
#[derive(Clone)]
pub struct ChannelBinding {
    local: [u8; 32],
    peer: [u8; 32],
    exporter: [u8; 32],
    live: Arc<AtomicBool>,
}

impl ChannelBinding {
    pub fn local(&self) -> &[u8; 32] {
        &self.local
    }
    pub fn peer(&self) -> &[u8; 32] {
        &self.peer
    }
    pub fn local_fingerprint(&self) -> Vec<u8> {
        Sha256::digest(self.local).to_vec()
    }
    pub fn peer_fingerprint(&self) -> Vec<u8> {
        Sha256::digest(self.peer).to_vec()
    }
    pub fn exporter(&self) -> &[u8; 32] {
        &self.exporter
    }
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
}

/// Byte carrier for the embedding's existing Tembra transport. No alternate RPC protocol is added.
/// Dropping a carrier fences its attachment; it does not expire a retained offline vault owner.
pub struct ManagementStream<T> {
    inner: TlsStream<T>,
    binding: ChannelBinding,
}

impl<T: AsyncRead + AsyncWrite + Unpin> ManagementStream<T> {
    pub async fn connect(io: T, identity: &TransportIdentity, peer: [u8; 32]) -> Result<Self> {
        let verifier = Arc::new(PinnedPeer::new(peer)?);
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| Error::Invalid)?
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(
                    identity.certified()?,
                )));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.enable_sni = false;
        config.enable_early_data = false;
        config.resumption = rustls::client::Resumption::disabled();
        // Raw-key identity is the independent pin; this fixed name is not a DNS trust substitute.
        let name = ServerName::try_from("swarmie.invalid").map_err(|_| Error::Invalid)?;
        let stream = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            TlsConnector::from(Arc::new(config)).connect(name, io),
        )
        .await
        .map_err(|_| Error::Revoked)?
        .map_err(|_| Error::Forbidden)?;
        let mut exporter = [0; 32];
        if stream.get_ref().1.alpn_protocol() != Some(ALPN) {
            return Err(Error::Forbidden);
        }
        stream
            .get_ref()
            .1
            .export_keying_material(&mut exporter, EXPORTER, Some(ALPN))
            .map_err(|_| Error::Forbidden)?;
        Ok(Self {
            inner: TlsStream::Client(stream),
            binding: ChannelBinding {
                local: identity.public(),
                peer,
                exporter,
                live: Arc::new(AtomicBool::new(true)),
            },
        })
    }

    pub async fn accept(io: T, identity: &TransportIdentity, peer: [u8; 32]) -> Result<Self> {
        let verifier = Arc::new(PinnedPeer::new(peer)?);
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| Error::Invalid)?
                .with_client_cert_verifier(verifier)
                .with_cert_resolver(Arc::new(AlwaysResolvesServerRawPublicKeys::new(
                    identity.certified()?,
                )));
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        let stream = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            TlsAcceptor::from(Arc::new(config)).accept(io),
        )
        .await
        .map_err(|_| Error::Revoked)?
        .map_err(|_| Error::Forbidden)?;
        let mut exporter = [0; 32];
        if stream.get_ref().1.alpn_protocol() != Some(ALPN) {
            return Err(Error::Forbidden);
        }
        stream
            .get_ref()
            .1
            .export_keying_material(&mut exporter, EXPORTER, Some(ALPN))
            .map_err(|_| Error::Forbidden)?;
        Ok(Self {
            inner: TlsStream::Server(stream),
            binding: ChannelBinding {
                local: identity.public(),
                peer,
                exporter,
                live: Arc::new(AtomicBool::new(true)),
            },
        })
    }

    pub fn binding(&self) -> ChannelBinding {
        self.binding.clone()
    }
}

impl<T> Drop for ManagementStream<T> {
    fn drop(&mut self) {
        self.binding.live.store(false, Ordering::Release);
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for ManagementStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(&result, Poll::Ready(Err(_)))
            || (matches!(&result, Poll::Ready(Ok(())))
                && before == buf.filled().len()
                && buf.remaining() > 0)
        {
            self.binding.live.store(false, Ordering::Release);
        }
        result
    }
}
impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for ManagementStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, bytes);
        if matches!(&result, Poll::Ready(Err(_))) {
            self.binding.live.store(false, Ordering::Release);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.binding.live.store(false, Ordering::Release);
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Debug)]
struct PinnedPeer {
    public: VerifyingKey,
    spki: Vec<u8>,
}
impl PinnedPeer {
    fn new(public: [u8; 32]) -> Result<Self> {
        let public = VerifyingKey::from_bytes(&public).map_err(|_| Error::Invalid)?;
        let spki = public
            .to_public_key_der()
            .map_err(|_| Error::Invalid)?
            .as_bytes()
            .to_vec();
        Ok(Self { public, spki })
    }
    fn certificate(
        &self,
        cert: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
    ) -> std::result::Result<(), rustls::Error> {
        if !chain.is_empty() || cert.as_ref() != self.spki {
            return Err(tls_denied());
        }
        Ok(())
    }
    fn signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.certificate(cert, &[])?;
        if signature.scheme != SignatureScheme::ED25519 {
            return Err(tls_denied());
        }
        self.public
            .verify_strict(
                message,
                &Signature::from_slice(signature.signature()).map_err(|_| tls_denied())?,
            )
            .map_err(|_| tls_denied())?;
        Ok(HandshakeSignatureValid::assertion())
    }
}
fn tls_denied() -> rustls::Error {
    rustls::Error::General("management peer proof rejected".into())
}
impl ServerCertVerifier for PinnedPeer {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        self.certificate(end, chain)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(tls_denied())
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signature(message, cert, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
impl ClientCertVerifier for PinnedPeer {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        self.certificate(end, chain)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(tls_denied())
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signature(message, cert, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
