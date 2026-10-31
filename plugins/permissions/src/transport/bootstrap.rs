//! Purpose-separated raw-key bootstrap. A completed handshake authenticates keys, not authority.
use super::*;
use ed25519_dalek::pkcs8::DecodePublicKey;

const BOOTSTRAP_ALPN: &[u8] = b"swarmie-link-bootstrap/1";
const BOOTSTRAP_EXPORTER: &[u8] = b"EXPORTER-Swarmie-Link-Bootstrap-v1";

/// Actual handshake evidence. It cannot be converted into a management ChannelBinding.
#[derive(Clone)]
pub struct BootstrapBinding {
    local: [u8; 32],
    peer: [u8; 32],
    exporter: [u8; 32],
    offer: [u8; 16],
    live: Arc<AtomicBool>,
}

impl BootstrapBinding {
    pub fn local(&self) -> &[u8; 32] {
        &self.local
    }

    pub fn peer(&self) -> &[u8; 32] {
        &self.peer
    }

    pub fn exporter(&self) -> &[u8; 32] {
        &self.exporter
    }

    pub fn offer(&self) -> &[u8; 16] {
        &self.offer
    }

    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
}

/// Restricted setup byte carrier. Its owner must validate the private offer proof before
/// sending metadata, and must use a separately admitted/pinned channel for ordinary RPC.
pub struct BootstrapStream<T> {
    inner: TlsStream<T>,
    binding: BootstrapBinding,
}

impl<T: AsyncRead + AsyncWrite + Unpin> BootstrapStream<T> {
    pub async fn connect(
        io: T,
        identity: &TransportIdentity,
        target: [u8; 32],
        offer: [u8; 16],
    ) -> Result<Self> {
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| Error::Invalid)?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedPeer::new(target)?))
                .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(
                    identity.certified()?,
                )));
        config.alpn_protocols = vec![BOOTSTRAP_ALPN.to_vec()];
        config.enable_sni = false;
        config.enable_early_data = false;
        config.resumption = rustls::client::Resumption::disabled();
        let name = ServerName::try_from("swarmie.invalid").map_err(|_| Error::Invalid)?;
        let stream = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            TlsConnector::from(Arc::new(config)).connect(name, io),
        )
        .await
        .map_err(|_| Error::Revoked)?
        .map_err(|_| Error::Forbidden)?;
        Self::completed(TlsStream::Client(stream), identity.public(), target, offer)
    }

    pub async fn accept(io: T, identity: &TransportIdentity, offer: [u8; 16]) -> Result<Self> {
        let mut config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| Error::Invalid)?
                .with_client_cert_verifier(Arc::new(BootstrapClient))
                .with_cert_resolver(Arc::new(AlwaysResolvesServerRawPublicKeys::new(
                    identity.certified()?,
                )));
        config.alpn_protocols = vec![BOOTSTRAP_ALPN.to_vec()];
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        let stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
            let start =
                tokio_rustls::LazyConfigAcceptor::new(rustls::server::Acceptor::default(), io)
                    .await
                    .map_err(|_| Error::Forbidden)?;
            // Select the purpose before CertificateVerify. A management ClientHello is never
            // accepted by the bootstrap verifier, even if its caller supplies a valid key.
            let protocols = start
                .client_hello()
                .alpn()
                .map(|values| values.collect::<Vec<_>>())
                .unwrap_or_default();
            if protocols != [BOOTSTRAP_ALPN] {
                return Err(Error::Forbidden);
            }
            start
                .into_stream(Arc::new(config))
                .await
                .map_err(|_| Error::Forbidden)
        })
        .await
        .map_err(|_| Error::Revoked)??;
        // This key belongs to this completed connection. No verifier-global mutable peer slot.
        let certificates = stream
            .get_ref()
            .1
            .peer_certificates()
            .ok_or(Error::Forbidden)?;
        let [certificate] = certificates else {
            return Err(Error::Forbidden);
        };
        let peer = canonical_key(certificate).map_err(|_| Error::Forbidden)?;
        Self::completed(TlsStream::Server(stream), identity.public(), peer, offer)
    }

    fn completed(
        inner: TlsStream<T>,
        local: [u8; 32],
        peer: [u8; 32],
        offer: [u8; 16],
    ) -> Result<Self> {
        let connection = inner.get_ref().1;
        if connection.alpn_protocol() != Some(BOOTSTRAP_ALPN) {
            return Err(Error::Forbidden);
        }
        let mut exporter = [0; 32];
        match &inner {
            TlsStream::Client(stream) => stream.get_ref().1.export_keying_material(
                &mut exporter,
                BOOTSTRAP_EXPORTER,
                Some(&offer),
            ),
            TlsStream::Server(stream) => stream.get_ref().1.export_keying_material(
                &mut exporter,
                BOOTSTRAP_EXPORTER,
                Some(&offer),
            ),
        }
        .map_err(|_| Error::Forbidden)?;
        Ok(Self {
            inner,
            binding: BootstrapBinding {
                local,
                peer,
                exporter,
                offer,
                live: Arc::new(AtomicBool::new(true)),
            },
        })
    }

    pub fn binding(&self) -> BootstrapBinding {
        self.binding.clone()
    }
}

impl<T> Drop for BootstrapStream<T> {
    fn drop(&mut self) {
        self.binding.live.store(false, Ordering::Release);
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for BootstrapStream<T> {
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

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for BootstrapStream<T> {
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

fn canonical_key(cert: &CertificateDer<'_>) -> std::result::Result<[u8; 32], rustls::Error> {
    let key = VerifyingKey::from_public_key_der(cert.as_ref()).map_err(|_| tls_denied())?;
    if key
        .to_public_key_der()
        .map_err(|_| tls_denied())?
        .as_bytes()
        != cert.as_ref()
    {
        return Err(tls_denied());
    }
    Ok(key.to_bytes())
}

#[derive(Debug)]
struct BootstrapClient;

impl ClientCertVerifier for BootstrapClient {
    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        if !chain.is_empty() {
            return Err(tls_denied());
        }
        canonical_key(end)?;
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
        PinnedPeer::new(canonical_key(cert)?)
            .map_err(|_| tls_denied())?
            .signature(message, cert, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
