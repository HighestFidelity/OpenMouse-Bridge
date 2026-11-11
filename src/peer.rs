//! Which local account opened a connection to the API (audit N3).
//!
//! The API listens on loopback with no token, and a non-browser client can
//! forge any Origin. On Linux the kernel lists every TCP socket with its
//! owner uid in /proc/net/tcp{,6}, so the bridge looks up the client end of
//! each connection there and serves only its own account. Other systems
//! keep the Origin checks alone.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::{net::SocketAddr, sync::Arc};

use axum::{extract::connect_info::Connected, serve::IncomingStream};
use tokio::{net::TcpListener, sync::OnceCell};

/// Both ends of an accepted connection, as the server saw them, plus the
/// connection's peer verdict. axum builds this once per accepted connection
/// and clones it into every request on it, so the clones share one verdict.
#[derive(Clone, Debug)]
pub struct Connection {
    peer: SocketAddr,
    /// The listening address the client connected to; `None` if the socket
    /// could not report it, which the Linux check refuses.
    local: Option<SocketAddr>,
    /// The first completed check, reused by every later request on this
    /// connection. Stays unset if that check was cancelled.
    verdict: Arc<OnceCell<bool>>,
}

impl Connection {
    pub fn new(peer: SocketAddr, local: Option<SocketAddr>) -> Self {
        Self {
            peer,
            local,
            verdict: Arc::default(),
        }
    }

    /// Whether the client end of this connection belongs to this process's
    /// uid. The socket table is read for the first request only: a TCP
    /// connection keeps its owner, so later requests reuse the answer.
    #[cfg(target_os = "linux")]
    pub async fn same_user(&self) -> bool {
        *self
            .verdict
            .get_or_init(|| same_user(self.peer, self.local))
            .await
    }
}

impl Connected<IncomingStream<'_, TcpListener>> for Connection {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self::new(*stream.remote_addr(), stream.io().local_addr().ok())
    }
}

/// Whether the socket whose local end is `peer` and remote end is `local`
/// belongs to this process's uid. Fails closed: an unknown listening
/// address, an unreadable table or a missing entry is a refusal.
#[cfg(target_os = "linux")]
async fn same_user(peer: SocketAddr, local: Option<SocketAddr>) -> bool {
    let Some(local) = local else {
        tracing::warn!("Refused API connection: listening address unknown");
        return false;
    };
    let (tcp, tcp6) = tokio::join!(
        tokio::fs::read_to_string("/proc/net/tcp"),
        tokio::fs::read_to_string("/proc/net/tcp6"),
    );
    // tcp6 is absent when IPv6 is disabled; tcp is required.
    let Ok(tcp) = tcp else {
        tracing::warn!("Refused API connection: /proc/net/tcp is unreadable");
        return false;
    };
    let tcp6 = tcp6.unwrap_or_default();
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    match owner_uid([tcp.as_str(), tcp6.as_str()], peer, local) {
        Some(owner) if owner == uid => true,
        Some(owner) => {
            tracing::warn!(%peer, owner, "Refused API connection from another account");
            false
        }
        None => {
            tracing::warn!(%peer, "Refused API connection: client socket not found");
            false
        }
    }
}

/// The uid owning the socket whose local end is `peer` and remote end is
/// `server`, from /proc/net/tcp-format tables. `None` if no entry matches or
/// matching entries disagree.
fn owner_uid<'a>(
    tables: impl IntoIterator<Item = &'a str>,
    peer: SocketAddr,
    server: SocketAddr,
) -> Option<u32> {
    let peer = canonical(peer);
    let server = canonical(server);
    let mut found = None;
    for line in tables.into_iter().flat_map(|table| table.lines().skip(1)) {
        let mut fields = line.split_whitespace();
        // sl, local_address, rem_address, st, tx:rx, tr:when, retrnsmt, uid
        let (Some(_), Some(local), Some(remote)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if parse_endpoint(local) != Some(peer) || parse_endpoint(remote) != Some(server) {
            continue;
        }
        let uid = fields.nth(4)?.parse::<u32>().ok()?;
        if found.is_some_and(|seen| seen != uid) {
            return None;
        }
        found = Some(uid);
    }
    found
}

/// IPv4-mapped IPv6 as plain IPv4: a client on an IPv6 socket that dials
/// 127.0.0.1 shows up in tcp6 as ::ffff:127.0.0.1.
fn canonical(address: SocketAddr) -> SocketAddr {
    SocketAddr::new(address.ip().to_canonical(), address.port())
}

/// `0100007F:45C6` (IPv4) or 32 hex digits plus port (IPv6). The kernel
/// prints each 32-bit word of the address as a native-endian integer and the
/// port as a plain number.
fn parse_endpoint(text: &str) -> Option<SocketAddr> {
    let (address, port) = text.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let word = |hex: &str| u32::from_str_radix(hex, 16).ok().map(u32::to_ne_bytes);
    let ip: std::net::IpAddr = match address.len() {
        8 => std::net::Ipv4Addr::from(word(address)?).into(),
        32 => {
            let mut bytes = [0u8; 16];
            for (chunk, out) in address.as_bytes().chunks(8).zip(bytes.chunks_mut(4)) {
                out.copy_from_slice(&word(std::str::from_utf8(chunk).ok()?)?);
            }
            std::net::Ipv6Addr::from(bytes).into()
        }
        _ => return None,
    };
    Some(canonical(SocketAddr::new(ip, port)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

    /// 127.0.0.1 as the kernel prints it on this machine.
    fn v4(port: u16) -> String {
        let word = u32::from_ne_bytes([127, 0, 0, 1]);
        format!("{word:08X}:{port:04X}")
    }

    /// ::ffff:127.0.0.1 as the kernel prints it on this machine.
    fn v4_mapped(port: u16) -> String {
        let bytes = std::net::Ipv4Addr::LOCALHOST.to_ipv6_mapped().octets();
        let words: String = bytes
            .chunks(4)
            .map(|chunk| format!("{:08X}", u32::from_ne_bytes(chunk.try_into().unwrap())))
            .collect();
        format!("{words}:{port:04X}")
    }

    fn row(local: &str, remote: &str, uid: u32) -> String {
        format!(
            "   0: {local} {remote} 01 00000000:00000000 00:00000000 00000000 {uid:>5}        0 4242 1 0000000000000000 20 4 30 10 -1"
        )
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn finds_the_client_end_not_the_server_end() {
        let table = [
            HEADER.to_owned(),
            // The listener and the server's accepted socket belong to us.
            row(&v4(17846), &v4(0), 1000),
            row(&v4(17846), &v4(51000), 1000),
            // The client socket belongs to someone else.
            row(&v4(51000), &v4(17846), 1001),
        ]
        .join("\n");
        assert_eq!(
            owner_uid([table.as_str(), ""], addr(51000), addr(17846)),
            Some(1001)
        );
    }

    #[test]
    fn reads_ipv4_mapped_clients_from_tcp6() {
        let tcp = [HEADER.to_owned(), row(&v4(17846), &v4(52000), 1000)].join("\n");
        let tcp6 = [
            HEADER.to_owned(),
            row(&v4_mapped(52000), &v4_mapped(17846), 1000),
        ]
        .join("\n");
        assert_eq!(
            owner_uid([tcp.as_str(), tcp6.as_str()], addr(52000), addr(17846)),
            Some(1000)
        );
    }

    #[test]
    fn missing_or_conflicting_entries_give_none() {
        let tcp = [HEADER.to_owned(), row(&v4(53000), &v4(17846), 1000)].join("\n");
        // Wrong server port and unknown client.
        assert_eq!(owner_uid([tcp.as_str()], addr(53000), addr(9999)), None);
        assert_eq!(owner_uid([tcp.as_str()], addr(53001), addr(17846)), None);
        assert_eq!(owner_uid([""], addr(53000), addr(17846)), None);
        // Two entries for one connection that disagree.
        let both = format!("{tcp}\n{}", row(&v4(53000), &v4(17846), 0));
        assert_eq!(owner_uid([both.as_str()], addr(53000), addr(17846)), None);
        // Truncated row.
        let short = format!("{HEADER}\n   0: {} {} 01", v4(53000), v4(17846));
        assert_eq!(owner_uid([short.as_str()], addr(53000), addr(17846)), None);
    }

    #[test]
    fn parses_kernel_endpoints() {
        assert_eq!(parse_endpoint(&v4(17846)), Some(addr(17846)));
        assert_eq!(parse_endpoint(&v4_mapped(80)), Some(addr(80)));
        assert_eq!(
            parse_endpoint(&format!("{}:0050", "0".repeat(31) + "1")).map(|a| a.port()),
            Some(80)
        );
        assert_eq!(parse_endpoint("0100007F"), None);
        assert_eq!(parse_endpoint("XYZ:0050"), None);
        assert_eq!(parse_endpoint("0100007F:FFFFF"), None);
    }

    /// A real loopback connection from this process is found under our uid.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn real_connection_from_this_process_is_allowed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(server).await.unwrap();
        let (_accepted, peer) = listener.accept().await.unwrap();
        assert_eq!(peer, client.local_addr().unwrap());
        assert!(Connection::new(peer, Some(server)).same_user().await);
        // A connection that does not exist is refused, and so is one whose
        // listening address is unknown.
        assert!(!Connection::new(addr(1), Some(server)).same_user().await);
        assert!(!Connection::new(peer, None).same_user().await);
    }

    /// Later requests on a connection reuse the first verdict instead of
    /// reading the socket table again.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn verdict_is_checked_once_per_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = listener.local_addr().unwrap();
        let _client = tokio::net::TcpStream::connect(server).await.unwrap();
        let (_accepted, peer) = listener.accept().await.unwrap();

        let allowed = Connection::new(peer, Some(server));
        assert_eq!(allowed.verdict.get(), None);
        assert!(allowed.same_user().await);
        assert_eq!(allowed.verdict.get(), Some(&true));
        // The next request gets a clone. Point it at a socket a fresh table
        // read would refuse: the cached answer proves there was no re-read.
        let mut next_request = allowed.clone();
        next_request.peer = addr(1);
        assert!(next_request.same_user().await);

        // A refusal sticks the same way.
        let refused = Connection::new(addr(1), Some(server));
        assert!(!refused.same_user().await);
        let mut next_request = refused.clone();
        next_request.peer = peer;
        assert!(!next_request.same_user().await);
        assert_eq!(refused.verdict.get(), Some(&false));
    }
}
