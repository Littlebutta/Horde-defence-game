//! Direct connections between co-op players over the plain internet, beside Steam.
//!
//! The host opens a TCP port, asks the router to forward it (UPnP), and posts its addresses and a
//! secret token in the Steam lobby. A friend who joins the lobby tries every address at once and
//! keeps the first that answers; from then on the game's messages go straight between the two PCs,
//! with no Steam bandwidth limits in the way. Nobody has to forward a port by hand: when no address
//! answers (a router without UPnP, a strict firewall) the friend simply stays on Steam networking,
//! which punches through NATs itself and relays when it can't.

#![cfg_attr(not(feature = "steam"), allow(dead_code))]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// "PHD1": the first bytes of a handshake
const MAGIC: u32 = 0x5048_4431;
/// how long the router keeps the forwarded port (renewed by each new co-op lobby)
const LEASE: u32 = 4 * 3600;
/// no single message is bigger than this (the map is sent in 60 KB chunks)
const MAX_MSG: usize = 64 << 20;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(2500);

pub enum In {
    Data(u64, Vec<u8>),
    /// the connection to this peer broke (with the generation of the connection that broke)
    Lost(u64, u32),
}

/// One direct connection. Messages go to a writer thread, so sending a big map never stalls a frame.
pub struct Peer {
    tx: Sender<Vec<u8>>,
    sock: TcpStream,
    generation: u32,
}

impl Peer {
    fn start(id: u64, generation: u32, s: TcpStream, ins: Sender<In>) -> Option<Peer> {
        let _ = s.set_nodelay(true);
        let _ = s.set_read_timeout(None);
        let _ = s.set_write_timeout(None);
        let mut w = s.try_clone().ok()?;
        let mut r = s.try_clone().ok()?;
        let (tx, rx) = channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for m in rx {
                if w.write_all(&(m.len() as u32).to_le_bytes()).and_then(|_| w.write_all(&m)).is_err() {
                    break;
                }
            }
            let _ = w.shutdown(Shutdown::Both);
        });
        std::thread::spawn(move || {
            let mut len = [0u8; 4];
            loop {
                if r.read_exact(&mut len).is_err() {
                    break;
                }
                let n = u32::from_le_bytes(len) as usize;
                if n > MAX_MSG {
                    break;
                }
                let mut b = vec![0u8; n];
                if r.read_exact(&mut b).is_err() || ins.send(In::Data(id, b)).is_err() {
                    break;
                }
            }
            let _ = ins.send(In::Lost(id, generation));
        });
        Some(Peer { tx, sock: s, generation })
    }
    pub fn send(&self, data: &[u8]) {
        let _ = self.tx.send(data.to_vec());
    }
    pub fn close(&self) {
        let _ = self.sock.shutdown(Shutdown::Both);
    }
}

fn read_u32(s: &mut TcpStream) -> Option<u32> {
    let mut b = [0u8; 4];
    s.read_exact(&mut b).ok()?;
    Some(u32::from_le_bytes(b))
}
fn read_u64(s: &mut TcpStream) -> Option<u64> {
    Some(read_u32(s)? as u64 | (read_u32(s)? as u64) << 32)
}

/// splitmix64's finaliser: spreads the bits of the seed over the whole token
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// This PC's address on its local network (the one its default route uses). Nothing is sent.
fn local_ip(probe: &str, bind: &str) -> Option<IpAddr> {
    let u = UdpSocket::bind(bind).ok()?;
    u.connect(probe).ok()?;
    Some(u.local_addr().ok()?.ip())
}

// ------------------------------------------------------------------------------------------ host

pub struct Host {
    pub token: u64,
    addrs: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    /// handshakes that passed the token check, waiting until their Steam id shows up in the lobby
    waiting: Arc<Mutex<Vec<(u64, TcpStream, Instant)>>>,
    peers: HashMap<u64, Peer>,
    generation: u32,
    tx: Sender<In>,
    rx: Receiver<In>,
    mapping: Arc<Mutex<Option<(igd_next::Gateway, u16)>>>,
}

impl Host {
    /// Opens the port and starts asking the router to forward it. `seed` makes the token unique.
    pub fn start(seed: u64) -> Option<Host> {
        let v4 = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        let port = v4.local_addr().ok()?.port();
        let v6 = TcpListener::bind((Ipv6Addr::UNSPECIFIED, port)).ok();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        let token = mix64(seed ^ nanos.rotate_left(17) ^ port as u64) | 1;
        let (tx, rx) = channel();
        let h = Host {
            token,
            addrs: Arc::default(),
            stop: Arc::new(AtomicBool::new(false)),
            waiting: Arc::default(),
            peers: HashMap::new(),
            generation: 0,
            tx,
            rx,
            mapping: Arc::default(),
        };
        let has_v6 = v6.is_some();
        for l in std::iter::once(v4).chain(v6) {
            let (stop, waiting) = (h.stop.clone(), h.waiting.clone());
            let _ = l.set_nonblocking(true);
            std::thread::spawn(move || accept_loop(l, token, stop, waiting));
        }
        // find the addresses friends can reach us on; UPnP can take a few seconds
        let (addrs, mapping) = (h.addrs.clone(), h.mapping.clone());
        std::thread::spawn(move || {
            let mut list = Vec::new();
            if let Some(IpAddr::V4(lan)) = local_ip("8.8.8.8:53", "0.0.0.0:0") {
                let local = SocketAddr::new(IpAddr::V4(lan), port);
                let mut opts = igd_next::SearchOptions::default();
                opts.timeout = Some(Duration::from_secs(3));
                if let Ok(gw) = igd_next::search_gateway(opts) {
                    let proto = igd_next::PortMappingProtocol::TCP;
                    let ext = gw
                        .add_port(proto, port, local, LEASE, "Pixel Horde co-op")
                        .map(|_| port)
                        .or_else(|_| gw.add_any_port(proto, local, LEASE, "Pixel Horde co-op"))
                        .ok();
                    if let (Some(ext), Ok(ip)) = (ext, gw.get_external_ip()) {
                        list.push(SocketAddr::new(ip, ext).to_string());
                        *mapping.lock().unwrap() = Some((gw, ext));
                    }
                }
                list.push(local.to_string());
            }
            if has_v6 {
                if let Some(IpAddr::V6(ip)) = local_ip("[2001:4860:4860::8888]:53", "[::]:0") {
                    // a global IPv6 address needs no NAT traversal, only the firewall's OK
                    if ip.segments()[0] & 0xe000 == 0x2000 {
                        list.push(SocketAddr::new(IpAddr::V6(ip), port).to_string());
                    }
                }
            }
            *addrs.lock().unwrap() = Some(list.join(","));
        });
        Some(h)
    }

    /// The addresses to post in the lobby, once they are known (returned once).
    pub fn take_addrs(&self) -> Option<String> {
        self.addrs.lock().unwrap().take()
    }
    pub fn has(&self, id: u64) -> bool {
        self.peers.contains_key(&id)
    }

    /// Takes on connections from lobby members (`member` says who is in the lobby) and collects
    /// what arrived. Returns the peers whose connection broke.
    pub fn poll(&mut self, member: impl Fn(u64) -> bool, out: &mut Vec<(u64, Vec<u8>)>) -> Vec<u64> {
        let ready: Vec<(u64, TcpStream)> = {
            let mut w = self.waiting.lock().unwrap();
            let mut ready = Vec::new();
            let mut keep = Vec::new();
            for (id, s, t) in w.drain(..) {
                if member(id) {
                    ready.push((id, s));
                } else if t.elapsed() < Duration::from_secs(10) {
                    keep.push((id, s, t));
                }
            }
            *w = keep;
            ready
        };
        for (id, mut s) in ready {
            if s.write_all(&[1]).is_err() {
                continue;
            }
            self.generation += 1;
            if let Some(p) = Peer::start(id, self.generation, s, self.tx.clone()) {
                if let Some(old) = self.peers.insert(id, p) {
                    old.close();
                }
            }
        }
        let mut lost = Vec::new();
        for m in self.rx.try_iter() {
            match m {
                In::Data(id, d) => out.push((id, d)),
                In::Lost(id, g) => {
                    if self.peers.get(&id).is_some_and(|p| p.generation == g) {
                        self.peers.remove(&id);
                        lost.push(id);
                    }
                }
            }
        }
        lost
    }
    /// false when this peer has no direct connection (send it through Steam instead)
    pub fn send(&self, to: u64, data: &[u8]) -> bool {
        match self.peers.get(&to) {
            Some(p) => {
                p.send(data);
                true
            }
            None => false,
        }
    }
    pub fn drop_peer(&mut self, id: u64) {
        if let Some(p) = self.peers.remove(&id) {
            p.close();
        }
    }
    pub fn close(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for p in self.peers.values() {
            p.close();
        }
        self.peers.clear();
        if let Some((gw, port)) = self.mapping.lock().unwrap().take() {
            std::thread::spawn(move || {
                let _ = gw.remove_port(igd_next::PortMappingProtocol::TCP, port);
            });
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.close();
    }
}

fn accept_loop(l: TcpListener, token: u64, stop: Arc<AtomicBool>, waiting: Arc<Mutex<Vec<(u64, TcpStream, Instant)>>>) {
    while !stop.load(Ordering::Relaxed) {
        match l.accept() {
            Ok((mut s, _)) => {
                let waiting = waiting.clone();
                std::thread::spawn(move || {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                    let ok = read_u32(&mut s) == Some(MAGIC) && read_u64(&mut s) == Some(token);
                    match (ok, read_u64(&mut s)) {
                        (true, Some(id)) => waiting.lock().unwrap().push((id, s, Instant::now())),
                        _ => {
                            let _ = s.shutdown(Shutdown::Both);
                        }
                    }
                });
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

// ------------------------------------------------------------------------------------------ client

/// Tries the host's addresses (comma separated) all at once and hands back the first connection the
/// host accepts, or None when none does.
pub fn connect(addrs: &str, token: u64, me: u64) -> Receiver<Option<TcpStream>> {
    let (done, res) = channel();
    let list: Vec<SocketAddr> = addrs.split(',').filter_map(|a| a.trim().parse().ok()).collect();
    std::thread::spawn(move || {
        let (tx, rx) = channel();
        for a in &list {
            let (a, tx) = (*a, tx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(TcpStream::connect_timeout(&a, CONNECT_TIMEOUT).ok());
            });
        }
        drop(tx);
        // the first address to connect is usually the best route (the LAN beats the internet)
        for s in rx.iter().flatten() {
            if let Some(s) = handshake(s, token, me) {
                let _ = done.send(Some(s));
                return;
            }
        }
        let _ = done.send(None);
    });
    res
}

fn handshake(mut s: TcpStream, token: u64, me: u64) -> Option<TcpStream> {
    let mut b = Vec::with_capacity(20);
    b.extend_from_slice(&MAGIC.to_le_bytes());
    b.extend_from_slice(&(token as u32).to_le_bytes());
    b.extend_from_slice(&((token >> 32) as u32).to_le_bytes());
    b.extend_from_slice(&(me as u32).to_le_bytes());
    b.extend_from_slice(&((me >> 32) as u32).to_le_bytes());
    s.write_all(&b).ok()?;
    // the host answers once it sees us in the Steam lobby
    s.set_read_timeout(Some(Duration::from_secs(12))).ok()?;
    let mut ok = [0u8; 1];
    s.read_exact(&mut ok).ok()?;
    (ok[0] == 1).then_some(s)
}

/// A client's direct connection to the host, once made.
pub struct ToHost {
    pub peer: Peer,
    rx: Receiver<In>,
}

impl ToHost {
    pub fn new(host: u64, s: TcpStream) -> Option<ToHost> {
        let (tx, rx) = channel();
        Some(ToHost { peer: Peer::start(host, 1, s, tx)?, rx })
    }
    /// What arrived from the host; false once the connection broke.
    pub fn poll(&mut self, out: &mut Vec<Vec<u8>>) -> bool {
        let mut alive = true;
        for m in self.rx.try_iter() {
            match m {
                In::Data(_, d) => out.push(d),
                In::Lost(..) => alive = false,
            }
        }
        alive
    }
}

impl Drop for ToHost {
    fn drop(&mut self) {
        self.peer.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host and a friend on this machine: the token check, the lobby check, and messages both ways.
    #[test]
    fn host_and_friend() {
        let mut h = Host::start(7).expect("listen");
        let t0 = Instant::now();
        let addrs = loop {
            if let Some(a) = h.take_addrs() {
                break a;
            }
            assert!(t0.elapsed() < Duration::from_secs(15));
            std::thread::sleep(Duration::from_millis(20));
        };
        let port = addrs.split(',').filter_map(|a| a.parse::<SocketAddr>().ok()).map(|a| a.port()).last().expect("an address");
        let local = format!("127.0.0.1:{}", port);
        // a wrong token is turned away
        let bad = connect(&local, h.token ^ 2, 42);
        // the right token waits until the friend is in the lobby
        let good = connect(&format!("10.255.255.1:9,{}", local), h.token, 42);
        let mut data = Vec::new();
        let t0 = Instant::now();
        let s = loop {
            h.poll(|id| id == 42, &mut data);
            if let Ok(s) = good.try_recv() {
                break s.expect("connected");
            }
            assert!(t0.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(bad.recv_timeout(Duration::from_secs(8)).unwrap().is_none());
        assert!(h.has(42));
        let mut c = ToHost::new(1, s).unwrap();
        let big = vec![7u8; 3 << 20];
        c.peer.send(b"hello");
        assert!(h.send(42, &big));
        let (mut got, mut back) = (Vec::new(), Vec::new());
        let t0 = Instant::now();
        while (got.is_empty() || back.is_empty()) && t0.elapsed() < Duration::from_secs(10) {
            h.poll(|id| id == 42, &mut got);
            c.poll(&mut back);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, vec![(42, b"hello".to_vec())]);
        assert_eq!(back, vec![big]);
        drop(c);
        let t0 = Instant::now();
        loop {
            if h.poll(|_| true, &mut got) == vec![42] {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(5), "the broken connection is noticed");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!h.has(42));
        h.close();
    }
}
