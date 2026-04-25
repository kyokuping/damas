use bytes::Bytes;
use compio::BufResult;
use compio::buf::{IoBuf, IoBufMut};
use compio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use compio::time::timeout;
use damas::cert::build_rustls_server_config;
use damas::config::{TLSCertificate, TLSPrivateKey};
use futures::SinkExt;
use futures::channel::mpsc::{self, Receiver, Sender};
use futures::future::join;
use rustls::SignatureScheme;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{ClientConfig, DigitallySignedStruct};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct MemoryStream {
    tx: Sender<Bytes>,
    rx: Receiver<Bytes>,
    read_buffer: Option<Bytes>,
}

impl AsyncRead for MemoryStream {
    async fn read<B: IoBufMut>(&mut self, mut buf: B) -> BufResult<usize, B> {
        tracing::debug!(target: "MemoryStream", capacity = buf.buf_capacity(), "read requested");
        if self.read_buffer.as_ref().is_none_or(|b| b.is_empty()) {
            tracing::trace!(target: "MemoryStream", "buffer empty,waiting for data");
            match self.rx.recv().await {
                Ok(data) => {
                    tracing::debug!(target: "MemoryStream", len = data.len(), "received bytes from rx");
                    self.read_buffer = Some(data)
                }
                _ => {
                    tracing::warn!(target: "MemoryStream", "rx channel closed during read");
                    return BufResult(Ok(0), buf);
                }
            }
        }

        let rb = self.read_buffer.as_mut().unwrap();
        let consumed = std::cmp::min(rb.len(), buf.buf_capacity());
        let chunk = rb.split_to(consumed);

        unsafe {
            std::ptr::copy_nonoverlapping(chunk.as_ptr(), buf.as_buf_mut_ptr(), consumed);
            buf.set_buf_init(consumed);
        }

        if rb.is_empty() {
            self.read_buffer = None;
        }
        tracing::info!(target: "MemoryStream", bytes = consumed, "read successful");
        BufResult(Ok(consumed), buf)
    }
}

impl AsyncWrite for MemoryStream {
    async fn write<T: IoBuf>(&mut self, buf: T) -> BufResult<usize, T> {
        let len = buf.as_slice().len();
        tracing::debug!(target: "MemoryStream", len, "write requested");

        let data = bytes::Bytes::copy_from_slice(buf.as_slice());

        if let Err(e) = self.tx.try_send(data) {
            tracing::warn!(target: "MemoryStream", "broken pipe");
            if e.is_full() {
                tracing::debug!(target: "MemoryStream", "channel full, awaiting capacity");
                if self.tx.send(e.into_inner()).await.is_err() {
                    tracing::error!(target: "MemoryStream", "failed to send data: channel closed");
                    return BufResult(
                        Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "closed",
                        )),
                        buf,
                    );
                }
            } else {
                tracing::error!(target: "MemoryStream", "write failed: broken pipe");
                return BufResult(
                    Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "closed",
                    )),
                    buf,
                );
            }
        };
        tracing::info!(target: "MemoryStream", bytes = len, "Channel transfer complete");
        BufResult(Ok(len), buf)
    }
    async fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
    async fn shutdown(&mut self) -> Result<(), std::io::Error> {
        self.tx.close_channel();
        Ok(())
    }
}

fn create_duplex_pair(buffer_size: usize) -> (MemoryStream, MemoryStream) {
    let (server_tx, client_rx) = mpsc::channel::<Bytes>(buffer_size);
    let (client_tx, server_rx) = mpsc::channel::<Bytes>(buffer_size);
    (
        MemoryStream {
            tx: server_tx,
            rx: server_rx,
            read_buffer: None,
        },
        MemoryStream {
            tx: client_tx,
            rx: client_rx,
            read_buffer: None,
        },
    )
}
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn make_dangerous_client_config() -> Result<Arc<ClientConfig>, anyhow::Error> {
    let verifier: Arc<dyn ServerCertVerifier> = Arc::new(NoVerifier);

    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    Ok(Arc::new(config))
}

fn load_cert(path: &str) -> TLSCertificate {
    let mut base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    base.push(path);

    TLSCertificate::try_from(base.clone())
        .unwrap_or_else(|_| panic!("fail to load cert.pem: {:?}", base))
}

fn load_key(path: &str) -> TLSPrivateKey {
    let mut base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    base.push(path);

    let key = rustls_pki_types::PrivateKeyDer::from_pem_file(&base)
        .unwrap_or_else(|_| panic!("fail to load key.pem: {:?}", base));
    TLSPrivateKey(key)
}

#[compio::test]
async fn test_tls_handshake_runtime() -> Result<(), anyhow::Error> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let client_config = make_dangerous_client_config()?;
    let provider = rustls::crypto::ring::default_provider();
    let cert: TLSCertificate = load_cert("test/fixtures/tls/cert.pem");
    let key: TLSPrivateKey = load_key("test/fixtures/tls/key.pem");
    let server_config = build_rustls_server_config(provider, cert, key)?;

    //inmemory stream
    let (server_io, client_io) = create_duplex_pair(65536);

    // server side handshake
    let server_task = compio::runtime::spawn(async move {
        let _span = tracing::info_span!("server").entered();
        tracing::info!("handshake start");
        let acceptor = compio::tls::TlsAcceptor::from(server_config);
        let mut stream = acceptor
            .accept(server_io)
            .await
            .expect("Server handshake failed");
        tracing::info!("handshake done");

        let buf = [0u8; 4];
        let BufResult(_, buf) = stream.read(buf).await;
        assert_eq!(&buf, b"ping");

        let BufResult(res, _) = stream.write_all(b"pong").await;
        assert!(res.is_ok(), "Writing to TLS stream should succeed");
        let _ = stream.shutdown().await;
        Ok::<(), anyhow::Error>(())
    });

    // client side handshake
    let client_task = compio::runtime::spawn(async move {
        let _span = tracing::info_span!("client").entered();
        tracing::info!("handshake start");
        let connector = compio::tls::TlsConnector::from(client_config);
        let domain = "localhost";
        let mut client_stream = connector
            .connect(domain, client_io)
            .await
            .expect("Client handshake failed");
        tracing::info!("handshake done");

        let BufResult(res, _) = client_stream.write_all(b"ping").await;
        assert!(res.is_ok(), "Writing to TLS stream should succeed");
        client_stream.flush().await.expect("flush failed");

        let client_buf = [0u8; 4];
        let BufResult(_, buf) = client_stream.read(client_buf).await;
        assert_eq!(&buf, b"pong");

        let _ = client_stream.shutdown().await;
        Ok::<(), anyhow::Error>(())
    });

    let result = timeout(Duration::from_secs(10), join(server_task, client_task)).await;

    match result {
        Ok((server_res, client_res)) => {
            server_res.map_err(|e| anyhow::anyhow!("Server task panicked: {:?}", e))??;
            client_res.map_err(|e| anyhow::anyhow!("Client task panicked: {:?}", e))??;
        }
        Err(_) => {
            panic!("test timeout");
        }
    }
    Ok(())
}
