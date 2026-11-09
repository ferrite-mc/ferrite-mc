use crate::cfb8::Cfb8;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{
    TcpStream,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
};

pub struct Wire {
    stream: TcpStream,
    read: Option<Cfb8>,
    write: Option<Cfb8>,
    /// Ciphertext for the packet currently being written. `poll_write` only
    /// hands us an immutable slice, so the buffer is encrypted in place here
    /// and handed to the socket in one call. Reused between writes.
    scratch: Vec<u8>,
}
impl Wire {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            read: None,
            write: None,
            scratch: Vec::new(),
        }
    }
    pub fn enable_encryption(&mut self, key: &[u8; 16]) {
        self.read = Some(Cfb8::new(key));
        self.write = Some(Cfb8::new(key));
    }
    /// Split into independent read and write halves. Each direction keeps its own CFB8 state,
    /// exactly as the single-stream implementation did.
    pub fn into_split(self) -> (WireReader, WireWriter) {
        let (read, write) = self.stream.into_split();
        (
            WireReader {
                stream: read,
                cipher: self.read,
            },
            WireWriter {
                stream: write,
                cipher: self.write,
            },
        )
    }
}
/// Read half of a [`Wire`]. Decryption stays inside `poll_read`, so the reader never has to
/// buffer or copy a whole frame just to decrypt it.
pub struct WireReader {
    stream: OwnedReadHalf,
    cipher: Option<Cfb8>,
}
/// Write half of a [`Wire`]. Encryption is applied once, in place, to a whole batch of frames
/// immediately before it is handed to the socket.
pub struct WireWriter {
    stream: OwnedWriteHalf,
    cipher: Option<Cfb8>,
}
/// The write half used by the connection writer. `write_loop` is generic over this trait so
/// tests can stall a batch write and exercise queue overflow without real socket buffers.
pub(crate) trait BatchWriter {
    fn write_batch<'a>(
        &'a mut self,
        batch: &'a mut [u8],
    ) -> impl std::future::Future<Output = std::io::Result<()>> + 'a;
}
impl BatchWriter for WireWriter {
    fn write_batch<'a>(
        &'a mut self,
        batch: &'a mut [u8],
    ) -> impl std::future::Future<Output = std::io::Result<()>> + 'a {
        WireWriter::write_batch(self, batch)
    }
}
impl WireWriter {
    /// Encrypt `batch` in place (single CFB8 pass) and write it as one unit.
    pub async fn write_batch(&mut self, batch: &mut [u8]) -> std::io::Result<()> {
        if let Some(cipher) = &mut self.cipher {
            cipher.encrypt(batch);
        }
        self.stream.write_all(batch).await?;
        self.stream.flush().await
    }
}
impl AsyncRead for WireReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.stream).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if let Some(cipher) = &mut self.cipher {
                    cipher.decrypt(&mut buf.filled_mut()[before..]);
                }
                Poll::Ready(Ok(()))
            }
            v => v,
        }
    }
}
impl AsyncRead for Wire {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.stream).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                if let Some(cipher) = &mut self.read {
                    cipher.decrypt(&mut buf.filled_mut()[before..]);
                }
                Poll::Ready(Ok(()))
            }
            v => v,
        }
    }
}
impl AsyncWrite for Wire {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.as_mut().get_mut();
        if this.write.is_none() {
            return Pin::new(&mut this.stream).poll_write(cx, data);
        }
        let cipher = this.write.as_mut().unwrap();
        // CFB8 has no rewind, so keep the state around until we know how many
        // bytes the socket took: a short write has to be replayed from here.
        let saved = cipher.state();
        let mut scratch = std::mem::take(&mut this.scratch);
        scratch.clear();
        scratch.extend_from_slice(data);
        cipher.encrypt(&mut scratch);
        let result = Pin::new(&mut this.stream).poll_write(cx, &scratch);
        match result {
            // Common case: the whole packet went out after a single pass over
            // the data (the old path encrypted it twice).
            Poll::Ready(Ok(written)) if written == data.len() => {
                this.scratch = scratch;
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Ok(written)) => {
                // Short write: rewind and advance by exactly the bytes the
                // socket accepted, so the caller retries with `data[written..]`.
                cipher.restore(saved);
                let mut accepted = data[..written].to_vec();
                cipher.encrypt(&mut accepted);
                this.scratch = scratch;
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Err(error)) => {
                cipher.restore(saved);
                this.scratch = scratch;
                Poll::Ready(Err(error))
            }
            Poll::Pending => {
                // Nothing was written, so nothing may be consumed.
                cipher.restore(saved);
                this.scratch = scratch;
                Poll::Pending
            }
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfb8::Cfb8;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_key() -> [u8; 16] {
        std::array::from_fn(|_| rand::random())
    }

    /// Frames queued behind the socket are encrypted as one batch by the writer half.
    #[tokio::test]
    async fn split_halves_encrypt_and_decrypt_a_batch() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let key = test_key();
        let mut client = Wire::new(client);
        client.enable_encryption(&key);
        let (mut client_read, mut client_write) = client.into_split();
        let mut server = Wire::new(server);
        server.enable_encryption(&key);
        let (mut server_read, mut server_write) = server.into_split();
        let plain = b"two frames in one write\x00\x01\x02".to_vec();
        let mut batch = plain.clone();
        client_write.write_batch(&mut batch).await.unwrap();
        assert_ne!(batch, plain, "bytes must be encrypted on the wire");
        let mut buffer = vec![0u8; plain.len()];
        client_read_bypass(&mut server_read, &mut buffer).await;
        assert_eq!(buffer, plain);
        let mut reply = b"pong".to_vec();
        server_write.write_batch(&mut reply).await.unwrap();
        let mut buffer = [0u8; 4];
        client_read_bypass(&mut client_read, &mut buffer).await;
        assert_eq!(&buffer, b"pong");
    }

    async fn client_read_bypass<R: AsyncRead + Unpin>(reader: &mut R, buffer: &mut [u8]) {
        reader.read_exact(buffer).await.unwrap();
    }

    /// A 3 MiB payload forces the socket buffers full, so the writer goes
    /// through `Pending` and short writes instead of the happy path only.
    #[tokio::test]
    async fn encrypted_stream_roundtrips_under_backpressure() {
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            let key = test_key();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let plaintext: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i * 7 + 3) as u8).collect();
            let expected = plaintext.clone();

            let writer = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut wire = Wire::new(stream);
                wire.enable_encryption(&key);
                for chunk in plaintext.chunks(65_521) {
                    wire.write_all(chunk).await.unwrap();
                }
                wire.flush().await.unwrap();
            });

            let mut reader = tokio::net::TcpStream::connect(address).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let mut cipher = Cfb8::new(&key);
            let mut received = Vec::with_capacity(expected.len());
            let mut buffer = vec![0u8; 8192];
            while received.len() < expected.len() {
                let read = reader.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "connection closed early");
                cipher.decrypt(&mut buffer[..read]);
                received.extend_from_slice(&buffer[..read]);
            }
            writer.await.unwrap();
            assert_eq!(received, expected);
        })
        .await
        .expect("encrypted transfer timed out");
    }
}
