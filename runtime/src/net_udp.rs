use super::*;

pub(super) fn packet(data: &[u8], peer: SocketAddr, capacity: usize) -> Result<Vec<u8>, String> {
    if matches!(peer, SocketAddr::V6(address) if address.scope_id() != 0) {
        return Err("scoped peer addresses are unsupported".into());
    }
    let peer = peer.to_string();
    let count = data.len().min(capacity);
    let mut output = Vec::with_capacity(2 + peer.len() + count);
    output.push(u8::from(count < data.len()));
    output.push(peer.len() as u8);
    output.extend_from_slice(peer.as_bytes());
    output.extend_from_slice(&data[..count]);
    Ok(output)
}

#[cfg(windows)]
fn ignore_connection_reset(socket: &std::net::UdpSocket) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;

    #[link(name = "ws2_32")]
    unsafe extern "system" {
        fn WSAIoctl(
            socket: usize,
            code: u32,
            input: *const core::ffi::c_void,
            input_len: u32,
            output: *mut core::ffi::c_void,
            output_len: u32,
            returned: *mut u32,
            overlapped: *mut core::ffi::c_void,
            completion: *const core::ffi::c_void,
        ) -> i32;
    }
    // Windows otherwise fails the next read when an earlier send drew an ICMP port-unreachable reply.
    const SIO_UDP_CONNRESET: u32 = 0x9800_000C;
    let report: u32 = 0;
    let mut returned: u32 = 0;
    let status = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as usize,
            SIO_UDP_CONNRESET,
            (&raw const report).cast(),
            size_of::<u32>() as u32,
            std::ptr::null_mut(),
            0,
            &raw mut returned,
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn ignore_connection_reset(_socket: &std::net::UdpSocket) -> std::io::Result<()> {
    Ok(())
}

/// # Safety
/// Output is writable and address addresses len readable UTF-8 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_udp_listen(
    out: *mut ValueError,
    family: i64,
    address: *const u8,
    len: i64,
) {
    if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        unsafe { out.write(ValueError::failed_text("net.ListenUDP: unsupported host")) };
        return;
    }
    let address = unsafe { text_of(address, len) };
    let result = blocking::run(move || -> std::io::Result<std::net::UdpSocket> {
        let endpoint: SocketAddr = address.parse().map_err(|_| {
            std::io::Error::new(ErrorKind::InvalidInput, "invalid literal endpoint")
        })?;
        if (family == 4 && !endpoint.is_ipv4()) || (family == 6 && !endpoint.is_ipv6()) {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                "address family mismatch",
            ));
        }
        let socket = std::net::UdpSocket::bind(endpoint)?;
        ignore_connection_reset(&socket)?;
        socket.set_nonblocking(true)?;
        Ok(socket)
    });
    let result = match result {
        Ok(socket) => ValueError::ok(insert(Handle::Datagram(Datagram {
            socket,
            read_deadline: AtomicI64::new(0),
            write_deadline: AtomicI64::new(0),
        }))),
        Err(error) => ValueError::failed_text(&format!("net.ListenUDP: {error}")),
    };
    unsafe { out.write(result) };
}

/// # Safety
/// Output is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_udp_read(
    out: *mut ByteArrayError,
    id: i64,
    capacity: i64,
) {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let result = poll::udp_read(id, capacity);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let result = ByteArrayError::failed("net.UDP.ReadFrom: unsupported host");
    unsafe { out.write(result) };
}

/// # Safety
/// Output is writable, data holds len readable bytes and peer is valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_udp_write(
    out: *mut ValueError,
    id: i64,
    data: *const u8,
    len: i64,
    peer: *const u8,
    peer_len: i64,
) {
    let data = unsafe { crate::bytes(data, len) };
    let peer = unsafe { text_of(peer, peer_len) };
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    let result = poll::udp_write(id, data, &peer);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let result = ValueError::failed_text("net.UDP.WriteTo: unsupported host");
    unsafe { out.write(result) };
}

/// # Safety
/// Output is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zore_native_net_udp_close(out: *mut ErrorOut, id: i64) {
    let result = if matches!(lookup(id).as_deref(), Some(Handle::Datagram(_))) {
        drop(remove(id));
        ErrorOut::ok()
    } else {
        ErrorOut::failed("net.UDP.Close: not an open datagram socket")
    };
    unsafe { out.write(result) };
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn reads_survive_a_send_to_a_closed_port() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        ignore_connection_reset(&socket).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let closed = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let closed_address = closed.local_addr().unwrap();
        drop(closed);
        socket.send_to(b"gone", closed_address).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        socket
            .send_to(b"hello", socket.local_addr().unwrap())
            .unwrap();
        let mut buffer = [0u8; 16];
        let (count, _) = socket.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..count], b"hello");
    }
}
