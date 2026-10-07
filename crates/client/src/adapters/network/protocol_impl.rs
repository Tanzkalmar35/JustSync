use just_sync_protocol::client_relay_handshake::CreateSessionMsg;

pub(super) struct ProtocolImpl {}

impl ProtocolImpl {
    /// Registers the peer on the relay server.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> relay server stream.
    /// * `recv` - The relay server -> localhost stream.
    ///
    /// # Errors
    ///
    /// * If serializing the given message fails.
    /// * If writing the given message to the outgoing stream fails.
    /// * If closing the init stream fails.
    /// * If reading a message from the incoming stream fails.
    /// * If the incoming message is not a valid `SessionCreatedMsg`.
    ///
    /// # Returns
    ///
    /// The relay server response of type `SessionCreatedMsg`.
    pub(super) async fn send_register_cmd(
        &self,
        send: &mut SendStream,
        recv: &mut RecvStream,
    ) -> anyhow::Result<CreateSessionMsg> {
        let register_msg = CreateSessionMsg::Register {
            p_version: PROTOCOL_VERSION,
        };

        send.write_all(&postcard::to_vec(&register_msg)?).await?;
        send.finish()?;

        let mut buf = vec![0u8; 1024];
        let n = recv.read(&mut buf).await?.unwrap_or(0);

        Ok(postcard::from_bytes(&buf[..n])?)
    }

    pub(super) async fn send_create_session_cmd(
        &self,
        send: &mut SendStream,
        recv: &mut RecvStream,
        pk: [u8; 32],
        sig: [u8; 32],
    ) -> Result<CreateSessionMsg> {
        let msg = CreateSessionMsg::CreateSession { 
            identity_pk: pk, 
            sig_id: sig, 
        };

        send.write_all(&postcard::to_vec(&msg)?).await?;
        send.finish()?;

        let mut buf = vec![0u8; 1024];
        let n = recv.read(&mut buf).await?.unwrap_or(0);

        Ok(postcard::from_bytes(&buf[..n])?)
    }

    /// Sends a given message in valid format, end to end encrypted.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> remote peer stream.
    /// * `msg` - The message to send to the remote peer.
    /// * `cipher` - The cipher used for E2EE.
    ///
    /// # Errors
    ///
    /// * If serializing the given message fails.
    /// * If encryption of the message fails.
    /// * If writing the message and the header to the output stream fails.
    pub(super) async fn send_framed<T>(
        &self,
        send: &mut quinn::SendStream,
        msg: T,
        cipher: Option<&ChaCha20Poly1305>,
    ) -> Result<()>
    where
        T: Sized + Serialize,
    {
        let mut bytes = serde_json::to_vec(&msg)?;

        // Encrypt message if cipher is provided, don't if not
        if let Some(c) = cipher {
            let nonce = ChaCha20Poly1305::generate_nonce(OsRng);
            match c.encrypt(&nonce, bytes.as_ref()) {
                Ok(blob) => {
                    // Prepend the 12-byte nonce to the ciphertext
                    let mut payload = nonce.to_vec();
                    payload.extend_from_slice(&blob);
                    bytes = payload;
                }
                Err(e) => {
                    error!(
                        "[Net] An error occured while encrypting outgoing message: {:?}",
                        e
                    );
                    return Err(anyhow::anyhow!("Encryption failed"));
                }
            }
        }

        let len = u32::try_from(bytes.len())?;

        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&bytes).await?;
        Ok(())
    }

    /// Recieves, decrypts and forwards a formatted message from a peer to the core.
    ///
    /// # Arguments
    ///
    /// * `recv` - The remote peer -> localhost stream.
    /// * `cipher` - The cipher used for decryption of the incoming message.
    ///
    /// # Errors
    ///
    /// * If reading the message from the `RecvStream` fails
    /// * If the incoming message is >100MB.
    /// * If the cipher is provided, if the incoming message does not contain the nonce.
    /// * If the cipher is provided, if decrypting the incoming message fails.
    ///
    /// # Returns
    ///
    /// The incoming message, validated, decrypted, ready to use.
    pub(super) async fn recv_framed<T>(
        &self,
        recv: &mut quinn::RecvStream,
        cipher: Option<&ChaCha20Poly1305>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Validate msg size
        if len > 100 * 1024 * 1024 {
            return Err(anyhow::anyhow!("Message too large (100MB limit)"));
        } else if len == 0 {
            return Box::pin(self.recv_framed(recv, cipher)).await;
        }

        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;

        // If a cipher is provided, we expect encrypted traffic. Otherwise not.
        if let Some(c) = cipher {
            // ChaCha20Poly1305 nonce is exactly 12 bytes
            if buf.len() < 12 {
                return Err(anyhow::anyhow!(
                    "Encrypted payload too small to contain nonce"
                ));
            }

            // Split the buffer into nonce and ciphertext
            let nonce = chacha20poly1305::Nonce::clone_from_slice(&buf[..12]);
            match c.decrypt(&nonce, &buf[12..]) {
                Ok(text) => Ok(serde_json::from_slice::<T>(&text)?),
                Err(e) => {
                    error!(
                        "[Net] An error occured decrypting incoming message: {:?}",
                        e
                    );
                    Err(anyhow::anyhow!("Decryption failed"))
                }
            }
        } else {
            // Unencrypted traffic (e.g., initial SPAKE2 handshake)
            Ok(serde_json::from_slice::<T>(&buf)?)
        }
    }
}
