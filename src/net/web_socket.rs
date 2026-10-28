//! Browser physical transport only. Each binary WS message is an unchanged
//! relay datagram; the existing UDP owner retains all protocol state.
//! With `wasm-lan`, a LAN-port bind instead selects the WebRTC peer socket.

use std::io;
use std::net::{SocketAddr, SocketAddrV4};

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = globalThis, js_name = veraSocketOpen)]
    fn open(origin: &str, channel: &str, max_datagram: usize) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraSocketSend)]
    fn send(id: u32, bytes: &[u8]) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraSocketReceive)]
    fn receive(id: u32) -> JsValue;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraSocketTakeRouteChange)]
    fn take_route_change(id: u32) -> bool;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraSocketClose)]
    fn close(id: u32);
}

#[cfg(feature = "wasm-lan")]
#[path = "web_rtc.rs"]
mod web_rtc;

#[derive(Debug)]
pub(super) struct UdpSocket {
    id: Option<u32>,
    channel: &'static str,
    local: SocketAddrV4,
    relay: Option<SocketAddrV4>,
    #[cfg(feature = "wasm-lan")]
    lan: Option<web_rtc::LanSocket>,
}

impl UdpSocket {
    pub fn bind(local: SocketAddrV4, channel: &'static str) -> io::Result<Self> {
        // Online binds an ephemeral port; the LAN menu binds its fixed ports.
        #[cfg(feature = "wasm-lan")]
        if local.ip().is_unspecified() && local.port() != 0 {
            return Ok(Self {
                id: None,
                channel,
                local,
                relay: None,
                lan: Some(web_rtc::LanSocket::open(local.port())?),
            });
        }
        if !local.ip().is_unspecified() || local.port() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "browser networking supports online relay sessions only",
            ));
        }
        // Opening is deferred until the existing owner accepts a relay grant.
        // A LAN menu must not accidentally open a remote transport.
        Ok(Self {
            id: None,
            channel,
            local,
            relay: None,
            #[cfg(feature = "wasm-lan")]
            lan: None,
        })
    }

    pub fn set_relay(&mut self, endpoint: SocketAddrV4) -> io::Result<()> {
        #[cfg(feature = "wasm-lan")]
        if self.lan.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a browser LAN socket cannot carry a relay session",
            ));
        }
        let next = open(
            super::browser_proxy_url(),
            self.channel,
            vera20k_relay_protocol::MAX_DATAGRAM,
        )
        .map_err(|_| io::Error::other("browser proxy configuration is unavailable"))?;
        if let Some(previous) = self.id.replace(next) {
            close(previous);
        }
        // This is the grant's logical source, not the last send destination.
        // The gateway has one fixed UDP upstream and does not accept targets.
        self.relay = Some(endpoint);
        Ok(())
    }

    pub fn take_route_change(&self) -> bool {
        self.id.is_some_and(take_route_change)
    }

    pub fn set_nonblocking(&self, enabled: bool) -> io::Result<()> {
        if enabled {
            Ok(())
        } else {
            Err(io::ErrorKind::Unsupported.into())
        }
    }

    pub fn set_broadcast(&self, enabled: bool) -> io::Result<()> {
        // Group peers receive one copy each, like limited IPv4 broadcast.
        #[cfg(feature = "wasm-lan")]
        if self.lan.is_some() {
            return Ok(());
        }
        if enabled {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "LAN discovery is unavailable in the browser",
            ))
        } else {
            Ok(())
        }
    }

    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        Ok(None)
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        #[cfg(feature = "wasm-lan")]
        if let Some(lan) = &self.lan {
            return Ok(lan.local_addr());
        }
        Ok(self.local.into())
    }

    pub fn send_to(&self, bytes: &[u8], endpoint: SocketAddrV4) -> io::Result<usize> {
        #[cfg(feature = "wasm-lan")]
        if let Some(lan) = &self.lan {
            return lan.send_to(bytes, endpoint);
        }
        if self.relay != Some(endpoint) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "browser socket destination is not its admitted relay",
            ));
        }
        if bytes.is_empty() || bytes.len() > vera20k_relay_protocol::MAX_DATAGRAM {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let id = self.id.ok_or(io::ErrorKind::NotConnected)?;
        match send(id, bytes) {
            0 => Ok(bytes.len()),
            1 => Err(io::ErrorKind::WouldBlock.into()),
            _ => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "browser proxy socket is closed",
            )),
        }
    }

    pub fn recv_from(&self, buffer: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        #[cfg(feature = "wasm-lan")]
        if let Some(lan) = &self.lan {
            return lan.recv_from(buffer);
        }
        let Some(id) = self.id else {
            return Err(io::ErrorKind::WouldBlock.into());
        };
        let value = receive(id);
        if value.is_null() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        if value.is_undefined() {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        let packet = js_sys::Uint8Array::new(&value);
        let length = packet.length() as usize;
        if length == 0 || length > buffer.len() || length > vera20k_relay_protocol::MAX_DATAGRAM {
            return Err(io::ErrorKind::InvalidData.into());
        }
        packet.copy_to(&mut buffer[..length]);
        let endpoint = self.relay.ok_or(io::ErrorKind::NotConnected)?;
        Ok((length, endpoint.into()))
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            close(id);
        }
    }
}
