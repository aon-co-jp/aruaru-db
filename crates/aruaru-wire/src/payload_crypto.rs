//! 【第3層】アプリケーション層ペイロード暗号化
//!
//! TLS/QUIC(第1層)が万一中間者攻撃や実装不備で突破されても、
//! ペイロード自体が独立したセッション鍵で保護されているため
//! 即座には平文が得られない、という多層防御(defense in depth)を実現する。
//!
//! ## 適用範囲についての注記
//! pgwire crate の `process_socket` は引数の型が `tokio::net::TcpStream` に
//! 固定されており(TLSハンドシェイクも内部で完結する設計)、標準の
//! `AsyncRead + AsyncWrite` ラッパーを間に挟むことができない。そのため
//! 本レイヤーは、標準pgwireクライアント(psql等)が使うTCP+TLS経路には
//! 適用せず、aruaru-wire側とopen-runo側の両方を自前実装するQUIC経路
//! (`quic.rs`)でのみ使用する。フレーム形式:
//! `[4バイト長(BE)][12バイトnonce][ChaCha20-Poly1305暗号文+16バイトtag]`
//!
//! 鍵は認証完了後のセッション情報から HKDF-SHA256 で導出する
//! (`derive_session_key`)。TLSと役割が重複するため必須の暗号強度向上ではなく、
//! 運用上の追加の安全マージンという位置付け。

use std::pin::Pin;
use std::task::{Context, Poll};

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const LEN_PREFIX: usize = 4;
const NONCE_LEN: usize = 12;

/// マスターシークレットとセッション識別子から、ChaCha20-Poly1305用の
/// 32バイトセッション鍵を HKDF-SHA256 で導出する。
pub fn derive_session_key(master_secret: &[u8], session_info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, master_secret);
    let mut okm = [0u8; 32];
    hk.expand(session_info, &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// 環境変数 `ARUARU_PAYLOAD_MASTER_KEY` (16進64文字=32バイト) からマスターシークレットを読む。
/// 未設定の場合は第3層を無効化する(Noneを返す)。
pub fn master_secret_from_env() -> Option<[u8; 32]> {
    let hex_str = std::env::var("ARUARU_PAYLOAD_MASTER_KEY").ok()?;
    let bytes = hex_decode(hex_str.trim())?;
    if bytes.len() != 32 {
        tracing::warn!("ARUARU_PAYLOAD_MASTER_KEY must be 32 bytes (64 hex chars); ignoring");
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// TLS/QUIC復号後の生バイトストリームを ChaCha20-Poly1305 でさらに暗号化する
/// 透過ラッパー。`S` は `AsyncRead + AsyncWrite + Unpin` であればよい
/// (TcpStream・QUICの bidi stream を tokio::io::join したもの、いずれも可)。
pub struct EncryptedStream<S> {
    inner: S,
    cipher: ChaCha20Poly1305,

    // 書き込み側: 暗号化済みフレームをためておき、inner へ書き切るまで保持する
    write_staged: Vec<u8>,
    write_pos: usize,
    write_nonce_counter: u64,

    // 読み込み側: inner から受け取った生バイトのうち未処理分
    read_raw: Vec<u8>,
    // 復号済みで呼び出し元にまだ渡していない平文
    read_plain: Vec<u8>,
    read_plain_pos: usize,
    read_nonce_counter: u64,
}

impl<S> EncryptedStream<S> {
    pub fn new(inner: S, key: [u8; 32]) -> Self {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        Self {
            inner,
            cipher,
            write_staged: Vec::new(),
            write_pos: 0,
            write_nonce_counter: 0,
            read_raw: Vec::new(),
            read_plain: Vec::new(),
            read_plain_pos: 0,
            read_nonce_counter: 0,
        }
    }

    fn make_nonce(counter: u64) -> Nonce {
        // 12バイト: 先頭4バイトはランダム化(複数接続間の再利用を避ける)、
        // 残り8バイトは単調増加カウンタ(同一接続内でのnonce再利用を防ぐ)。
        let mut bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut bytes[..4]);
        bytes[4..].copy_from_slice(&counter.to_be_bytes());
        *Nonce::from_slice(&bytes)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for EncryptedStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        // 前回のフレームが書き切れていなければ、まずそれを吐き出す
        while this.write_pos < this.write_staged.len() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.write_staged[this.write_pos..]) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()))
                }
                Poll::Ready(Ok(n)) => this.write_pos += n,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }

        // 新しいフレームを1つ暗号化してステージする
        let nonce = Self::make_nonce(this.write_nonce_counter);
        this.write_nonce_counter += 1;
        let ciphertext = this
            .cipher
            .encrypt(&nonce, buf)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "payload encryption failed"))?;

        let mut frame = Vec::with_capacity(LEN_PREFIX + NONCE_LEN + ciphertext.len());
        let frame_body_len = (NONCE_LEN + ciphertext.len()) as u32;
        frame.extend_from_slice(&frame_body_len.to_be_bytes());
        frame.extend_from_slice(nonce.as_slice());
        frame.extend_from_slice(&ciphertext);

        this.write_staged = frame;
        this.write_pos = 0;

        while this.write_pos < this.write_staged.len() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.write_staged[this.write_pos..]) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()))
                }
                Poll::Ready(Ok(n)) => this.write_pos += n,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Ready(Ok(buf.len())), // 平文全体は既に暗号化済み・受理扱い
            }
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        while this.write_pos < this.write_staged.len() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.write_staged[this.write_pos..]) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()))
                }
                Poll::Ready(Ok(n)) => this.write_pos += n,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for EncryptedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        loop {
            // 復号済み平文が残っていれば、それを先に返す
            if this.read_plain_pos < this.read_plain.len() {
                let n = std::cmp::min(
                    buf.remaining(),
                    this.read_plain.len() - this.read_plain_pos,
                );
                buf.put_slice(&this.read_plain[this.read_plain_pos..this.read_plain_pos + n]);
                this.read_plain_pos += n;
                return Poll::Ready(Ok(()));
            }

            // フレーム全体(長さprefix + body)が揃っているか確認
            if this.read_raw.len() >= LEN_PREFIX {
                let body_len =
                    u32::from_be_bytes(this.read_raw[..LEN_PREFIX].try_into().unwrap()) as usize;
                if this.read_raw.len() >= LEN_PREFIX + body_len {
                    let frame_end = LEN_PREFIX + body_len;
                    let nonce_bytes = &this.read_raw[LEN_PREFIX..LEN_PREFIX + NONCE_LEN];
                    let nonce = *Nonce::from_slice(nonce_bytes);
                    let ciphertext = &this.read_raw[LEN_PREFIX + NONCE_LEN..frame_end];

                    let plaintext = this.cipher.decrypt(&nonce, ciphertext).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "payload decryption failed (tampered or wrong key)",
                        )
                    })?;
                    this.read_nonce_counter += 1;

                    this.read_raw.drain(..frame_end);
                    this.read_plain = plaintext;
                    this.read_plain_pos = 0;
                    continue;
                }
            }

            // 追加データを inner から読み込む
            let mut tmp = [0u8; 4096];
            let mut read_buf = ReadBuf::new(&mut tmp);
            match Pin::new(&mut this.inner).poll_read(cx, &mut read_buf) {
                Poll::Ready(Ok(())) => {
                    let filled = read_buf.filled();
                    if filled.is_empty() {
                        // EOF
                        if this.read_raw.is_empty() {
                            return Poll::Ready(Ok(()));
                        }
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "connection closed mid-frame",
                        )));
                    }
                    this.read_raw.extend_from_slice(filled);
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_round_trip_encrypt_decrypt() {
        let key = [7u8; 32];
        let (client_io, server_io) = tokio::io::duplex(4096);

        let mut client = EncryptedStream::new(client_io, key);
        let mut server = EncryptedStream::new(server_io, key);

        let msg = b"SELECT aruaru_commit('grant item to user')";
        let write_task = tokio::spawn(async move {
            client.write_all(msg).await.unwrap();
            client.flush().await.unwrap();
        });

        let mut buf = vec![0u8; msg.len()];
        server.read_exact(&mut buf).await.unwrap();
        write_task.await.unwrap();

        assert_eq!(&buf, msg);
    }

    #[tokio::test]
    async fn test_wrong_key_fails_to_decrypt() {
        let (client_io, server_io) = tokio::io::duplex(4096);
        let mut client = EncryptedStream::new(client_io, [1u8; 32]);
        let mut server = EncryptedStream::new(server_io, [2u8; 32]);

        let write_task = tokio::spawn(async move {
            client.write_all(b"secret").await.unwrap();
            client.flush().await.unwrap();
        });

        let mut buf = [0u8; 6];
        let result = server.read_exact(&mut buf).await;
        write_task.await.unwrap();

        assert!(result.is_err(), "decryption with wrong key must fail");
    }

    #[test]
    fn test_derive_session_key_deterministic() {
        let k1 = derive_session_key(b"master-secret", b"session-1");
        let k2 = derive_session_key(b"master-secret", b"session-1");
        let k3 = derive_session_key(b"master-secret", b"session-2");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }
}
