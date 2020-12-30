use crate::shared::VSOCK_PROXY_CID;
use nix::sys::select::{select, FdSet};
use nix::sys::socket::SockAddr;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use tracing::info;
use vsock::VsockListener;

/// Configuration parameters for port listening and remote destination
pub struct Proxy {
    local_port: u32,
    remote_addr: PathBuf,
}

impl Proxy {
    /// creates a new vsock<->uds proxy
    pub fn new(local_port: u32, remote_addr: PathBuf) -> Self {
        Self {
            local_port,
            remote_addr,
        }
    }

    /// Creates a listening socket
    /// Returns the file descriptor for it or the appropriate error
    pub fn sock_listen(&self) -> Result<VsockListener, String> {
        let sockaddr = SockAddr::new_vsock(VSOCK_PROXY_CID, self.local_port);
        let listener = VsockListener::bind(&sockaddr)
            .map_err(|_| format!("Could not bind to {:?}", sockaddr))?;
        info!("Bound to {:?}", sockaddr);
        Ok(listener)
    }

    /// Accepts an incoming connection coming on listener and handles it on a
    /// different thread
    /// Returns the handle for the new thread or the appropriate error
    pub fn sock_accept(&self, listener: &VsockListener) -> Result<(), String> {
        let (mut client, client_addr) = listener
            .accept()
            .map_err(|_| "Could not accept connection")?;
        info!("Accepted connection on {:?}", client_addr);
        let mut server = UnixStream::connect(&self.remote_addr)
            .map_err(|_| format!("Could not connect to {:?}", self.remote_addr))?;

        let client_socket = client.as_raw_fd();
        let server_socket = server.as_raw_fd();

        let mut disconnected = false;
        while !disconnected {
            let mut set = FdSet::new();
            set.insert(client_socket);
            set.insert(server_socket);

            select(None, Some(&mut set), None, None, None).expect("select");

            if set.contains(client_socket) {
                disconnected = transfer(&mut client, &mut server);
            }
            if set.contains(server_socket) {
                disconnected = transfer(&mut server, &mut client);
            }
        }
        info!("Client on {:?} disconnected", client_addr);
        Ok(())
    }
}

/// Transfers a chunck of maximum 8KB from src to dst
/// If no error occurs, returns true if the source disconnects and false otherwise
fn transfer(src: &mut dyn Read, dst: &mut dyn Write) -> bool {
    const BUFF_SIZE: usize = 8192;

    let mut buffer = [0u8; BUFF_SIZE];

    let nbytes = src.read(&mut buffer);
    let nbytes = match nbytes {
        Err(_) => 0,
        Ok(n) => n,
    };

    if nbytes == 0 {
        return true;
    }

    dst.write_all(&buffer[..nbytes]).is_err()
}