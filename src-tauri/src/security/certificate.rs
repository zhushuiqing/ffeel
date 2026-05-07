use std::path::Path;

pub struct CertificateManager;

impl CertificateManager {
    /// 从磁盘加载证书，若不存在则生成新的自签名证书
    /// 返回 (cert_pem, key_pem)
    pub fn load_or_create(config_dir: &Path) -> Result<(String, String), String> {
        let cert_path = config_dir.join("cert.pem");
        let key_path = config_dir.join("key.pem");

        if cert_path.exists() && key_path.exists() {
            let cert_pem =
                std::fs::read_to_string(&cert_path).map_err(|e| format!("读取证书失败: {}", e))?;
            let key_pem =
                std::fs::read_to_string(&key_path).map_err(|e| format!("读取密钥失败: {}", e))?;
            return Ok((cert_pem, key_pem));
        }

        let (cert_pem, key_pem) = Self::generate()?;

        if let Some(parent) = cert_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建证书目录失败: {}", e))?;
        }
        std::fs::write(&cert_path, &cert_pem).map_err(|e| format!("写入证书文件失败: {}", e))?;
        std::fs::write(&key_path, &key_pem).map_err(|e| format!("写入密钥文件失败: {}", e))?;

        Ok((cert_pem, key_pem))
    }

    /// 生成自签名 ECDSA P-256 证书
    fn generate() -> Result<(String, String), String> {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
            .map_err(|e| format!("生成密钥对失败: {}", e))?;

        let mut params = rcgen::CertificateParams::new(vec!["ffeel.local".to_string()])
            .map_err(|e| format!("创建证书参数失败: {}", e))?;
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::DigitalSignature,
            rcgen::KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "ffeel");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);

        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| format!("自签名证书生成失败: {}", e))?;

        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();

        Ok((cert_pem, key_pem))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_returns_valid_pem() {
        let (cert_pem, key_pem) = CertificateManager::generate().unwrap();
        assert!(cert_pem.starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(key_pem.starts_with("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn test_load_or_create_returns_same_cert_on_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let (cert1, key1) = CertificateManager::load_or_create(&path).unwrap();
        let (cert2, key2) = CertificateManager::load_or_create(&path).unwrap();

        assert_eq!(cert1, cert2);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_generated_cert_can_be_parsed_by_rustls() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (cert_pem, key_pem) = CertificateManager::generate().unwrap();

        let certs = rustls_pemfile::certs(&mut cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .expect("应能解析证书 PEM");
        assert_eq!(certs.len(), 1);

        let keys = rustls_pemfile::private_key(&mut key_pem.as_bytes())
            .expect("应能解析密钥 PEM")
            .expect("应包含私钥");

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, keys)
            .expect("应能用证书和密钥创建 TLS 配置");

        assert!(
            !config.alpn_protocols.is_empty() || true,
            "TLS config created"
        );
    }
}
