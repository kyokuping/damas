use crate::config::{TLSCertificate, TLSPrivateKey};
use rustls::{ServerConfig, crypto::CryptoProvider};

use std::sync::Arc;

pub fn build_rustls_server_config(
    provider: CryptoProvider,
    cert: TLSCertificate,
    key: TLSPrivateKey,
) -> Result<Arc<rustls::ServerConfig>, anyhow::Error> {
    Ok(Arc::new(
        ServerConfig::builder_with_provider(provider.into())
            .with_safe_default_protocol_versions()
            .map_err(|e| anyhow::anyhow!("Protocol versions error: {}", e))?
            .with_no_client_auth()
            .with_single_cert(vec![cert.into()], key.into())
            .map_err(|e| anyhow::anyhow!("Certificate error: {}", e))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};

    const MOCK_CERT: &[u8] = b"-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIUV5AL+XoFlAxN9oHMIFQ3F/2syb0wDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDMyMjEwMDg1M1oXDTM2MDMx
OTEwMDg1M1owFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEA8A9EgfgRoxfvCp7gafm5im8cUUqjfpY2tiSY5FBKXoMN
CC8FdbDQ04PEsEC4pPo9kCUA1uW4zoMVYtBXJKtXWTWpLYALmC0aK3obHFwLUEY6
F7cpIo62T+9TWdbNB+wTWuD58tIDDy9UW3CrCHiBUm+3cwoUa91IiA5W/mM3VO3h
211hP9DVRmn3r5nDRIUNNzeibnlAKWD28vWYtXsBH0rAjEDBBrKrytdCqllomTqL
oYjGxPaTxohvN1CkyHr6C3HkpoUE7NT5WkB2rMW1dQFuJZndTEYpFjyccICLobaO
RXYswmik87Ot/Yue9VTfznpsWvz3eTNMXrTOSHyeowIDAQABo1MwUTAdBgNVHQ4E
FgQUhWv+MszcHBAVIyzn3s7Ext4dXEMwHwYDVR0jBBgwFoAUhWv+MszcHBAVIyzn
3s7Ext4dXEMwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAf5Dn
/6YfrjNUW7KhwFkHrzPmt96Rwn+wAyHZxgpyXjE58r+SVn50vTtAsdHl1pEIXoDI
A+O5BblCBcczSK5j0QS6GhhVQmq/qlgbD1seQVCdYjdhWrDwabiT7qNlMrG/Ou78
uEIPs2YEO/9J4gLwDYfuEBbzm6YsafplRBk89ONnculUCcerK3TH7uwTj0tEMFze
MU5BnuTlkLIh/NfWWYMk6aQbyRXyGkZNrJxua6XZBOz3zphtPPmFcEh2SJYMKg2G
cLsgeG07wafYLYeQxXzTq5I+EYVLnqC9ekoYB1Ty5qoBM4RjLkYiSLFtahEZoP1D
ovn/Dd/ah7w0GjOE5Q==
-----END CERTIFICATE-----
";
    const MOCK_KEY: &[u8] = b"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDwD0SB+BGjF+8K
nuBp+bmKbxxRSqN+lja2JJjkUEpegw0ILwV1sNDTg8SwQLik+j2QJQDW5bjOgxVi
0Fckq1dZNaktgAuYLRorehscXAtQRjoXtykijrZP71NZ1s0H7BNa4Pny0gMPL1Rb
cKsIeIFSb7dzChRr3UiIDlb+YzdU7eHbXWE/0NVGafevmcNEhQ03N6JueUApYPby
9Zi1ewEfSsCMQMEGsqvK10KqWWiZOouhiMbE9pPGiG83UKTIevoLceSmhQTs1Pla
QHasxbV1AW4lmd1MRikWPJxwgIuhto5FdizCaKTzs639i571VN/Oemxa/Pd5M0xe
tM5IfJ6jAgMBAAECggEAPm9qHPd89tMhu7xol4d4pzWQwt/Lt/+viR3pme/796rT
993q6JotJeXugPzESTxASL4nAr1KnINhS4ruLz5VAIHBV3EnEtQgK1Cdvnl+A8nQ
EBz2GOPPLOkM35/LQZU3z3oV5/6RByEDKqkaAqD82YjuyH/FoewykhhQreb2HCMl
i2sS/aAszdhuFssH19z7YyYMNgRwzD0YIb3KOzwqI5t2pm3wTTJ7QzkZWFziadTN
UJ0Typ7uWLdcTuBF3TuGTm15AeLbzpY53d9oz29jrKVD0YBtnccf3OKrJwMAfxAA
U/Td7j0N4rQVDvn1uWmlHZi1FUAyOJSNBgD1CWxSEQKBgQD+6g2tEsXLRqHPFLun
er0PIU6M4Hk1+Vxcb1Qatzm8Wz4AJJ6Mr4MFUV2HNw+7k6BQ/XgKROYYYZPnepdH
jcOimukZ/Q4xFS+q4vj8+GfXHIkCIgZ86ncfQODpcrIBYlrFWNEt6i7MWOf7ttd0
85Roj+2wmNAKpN1FA6sPaSsBCwKBgQDxFQRzv6ghzdcHShAwBT6MnVS0lzJyUdAA
UN4YMY3cOLV4z38jk8a8NieVnEGhBqKyl4iY8IvdOAXFkTCqHpCKCh9ldUZhb3kV
ZFndDtqCl/T31Hjhjtb9KD2OK2MpPQXqPMDXsDckDJDLkUIVcK1Wres1Sx16ekUn
O5ajjASHyQKBgQCD56jb/fLLlOj1tszDhQd/ZMS4sQ8HltjsG89xY45EoRIcENba
BZfOkKPM6/kAHwu93OrYpX5K73MRPKY7KGgrI+2qvP8y9ruLuZcNj5xr+yAKMoEY
8lphmbjIE8l4XeSKacMT9zHwG7Eu1xX2NnR9Brz/vJMqbtTweU1y1ACksQKBgF/6
ORKHy7zhgOjDAJzNibBbdnyK8Sd4ELH/f9vr5ok0/nJBUWFtlKIbgTjbw3kC9kTZ
dSVGJriEdC/KdLBViL+b9hHjVYi242Kz197c6fsx2fHMYe+SeV7B5XezKEAjrjYp
x7BW1C0C36Zbhw6YFDo89TX7WJoJEXzkCT3FIYyZAoGBANCyOt5f5lYetrZOT2XP
e+w1mvgzD38vSC1bKVRt2YeWB/SWoEIyXJ28rE3IVW2UYPt53b+oPIP1lkHujLfV
HmM/bfOCTDfgtxnm77Mu43c8WLuaFiGvpH1o988Iuu2u1SOtJRW7cMiClxF7ywF5
xJmPLcfZPY5yCuRAv0hCxafa
-----END PRIVATE KEY-----
";

    fn mock_cert() -> (TLSCertificate, TLSPrivateKey) {
        let cert_der =
            CertificateDer::from_pem_slice(MOCK_CERT).expect("Failed to parse MOCK_CERT as PEM");

        let key_der =
            PrivateKeyDer::from_pem_slice(MOCK_KEY).expect("Failed to parse MOCK_KEY as PEM");

        (TLSCertificate(cert_der), TLSPrivateKey(key_der))
    }

    #[test]
    fn test_server_config_from_temp_files() {
        let (cert, key) = mock_cert();
        let result =
            build_rustls_server_config(rustls::crypto::ring::default_provider(), cert, key);
        assert!(
            result.is_ok(),
            "Failed to build server config: {:?}",
            result.err()
        );
    }
}
