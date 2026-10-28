//! Browser LAN physical transport only. Peers in the same signaling group
//! exchange unchanged LAN datagrams over WebRTC data channels; the gateway
//! only coordinates the handshake and assigns each peer a virtual IPv4.
//! Discovery, admission, ACK, retry and match state stay with the UDP owner.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = globalThis, js_name = veraLanOpen)]
    fn open(origin: &str, port: u16, max_datagram: usize) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLanSend)]
    fn send(id: u32, ip: u32, port: u16, bytes: &[u8]) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLanReceive)]
    fn receive(id: u32) -> JsValue;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLanLocalAddress)]
    fn local_address(id: u32) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLanClose)]
    fn close(id: u32);
}

/// Received packets carry the peer's virtual IPv4 and source port ahead of
/// the unchanged datagram; this prefix never reaches the protocol owner.
const RECEIVE_PREFIX_LEN: usize = 6;

#[derive(Debug)]
pub(super) struct LanSocket {
    id: u32,
    port: u16,
}

impl LanSocket {
    /// Opening joins the group derived from the public address. Until the
    /// gateway assigns a virtual address, sends drop like an unrouted UDP
    /// broadcast and the protocol owner keeps its own retries.
    pub fn open(port: u16) -> io::Result<Self> {
        let id = open(
            super::super::browser_proxy_url(),
            port,
            vera20k_relay_protocol::MAX_DATAGRAM,
        )
        .map_err(|_| io::Error::other("browser LAN signaling configuration is unavailable"))?;
        Ok(Self { id, port })
    }

    pub fn local_addr(&self) -> SocketAddr {
        SocketAddrV4::new(Ipv4Addr::from(local_address(self.id)), self.port).into()
    }

    pub fn send_to(&self, bytes: &[u8], endpoint: SocketAddrV4) -> io::Result<usize> {
        if bytes.is_empty() || bytes.len() > vera20k_relay_protocol::MAX_DATAGRAM {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        match send(self.id, endpoint.ip().to_bits(), endpoint.port(), bytes) {
            0 => Ok(bytes.len()),
            1 => Err(io::ErrorKind::WouldBlock.into()),
            _ => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "browser LAN socket is closed",
            )),
        }
    }

    pub fn recv_from(&self, buffer: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let value = receive(self.id);
        if value.is_null() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        if value.is_undefined() {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        let packet = js_sys::Uint8Array::new(&value);
        let total = packet.length() as usize;
        let length = total.saturating_sub(RECEIVE_PREFIX_LEN);
        if length == 0 || length > buffer.len() || length > vera20k_relay_protocol::MAX_DATAGRAM {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut prefix = [0; RECEIVE_PREFIX_LEN];
        packet
            .subarray(0, RECEIVE_PREFIX_LEN as u32)
            .copy_to(&mut prefix);
        packet
            .subarray(RECEIVE_PREFIX_LEN as u32, total as u32)
            .copy_to(&mut buffer[..length]);
        let ip = Ipv4Addr::from(u32::from_be_bytes(prefix[..4].try_into().unwrap()));
        let port = u16::from_be_bytes([prefix[4], prefix[5]]);
        Ok((length, SocketAddrV4::new(ip, port).into()))
    }
}

impl Drop for LanSocket {
    fn drop(&mut self) {
        close(self.id);
    }
}
