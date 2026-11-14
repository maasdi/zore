use super::*;
use std::sync::mpsc;
use std::task::{Wake, Waker};

struct Signal(mpsc::Sender<()>);

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send(());
    }
}

fn signal() -> (Waiter, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    (Waker::from(Arc::new(Signal(sender))).into(), receiver)
}

fn pair() -> (i64, SocketAddr, std::net::UdpSocket) {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let local = socket.local_addr().unwrap();
    let peer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let id = insert(Handle::Datagram(Datagram {
        socket,
        read_deadline: AtomicI64::new(0),
        write_deadline: AtomicI64::new(0),
    }));
    (id, local, peer)
}

fn finish(operation: &mut Operation, waiter: Waiter, receiver: &mpsc::Receiver<()>) -> Output {
    for _ in 0..10 {
        if let Some(output) = operation.poll(waiter.clone()) {
            return output;
        }
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    panic!("UDP operation did not complete");
}

fn bytes(output: Output) -> Vec<u8> {
    let Output::Bytes(output) = output else {
        panic!("expected datagram")
    };
    assert_eq!(output.failed, 0);
    let bytes = unsafe { crate::bytes(output.array.data, output.array.len) }.to_vec();
    unsafe { crate::alloc::zore_free(output.array.data, output.array.cap) };
    bytes
}

fn error(output: Output) -> String {
    let (data, len) = match output {
        Output::Bytes(value) => (value.message, value.message_len),
        Output::Value(value) => (value.message, value.message_len),
    };
    let text = String::from_utf8_lossy(unsafe { crate::bytes(data, len) }).into_owned();
    crate::string::zore_string_release(data, len);
    text
}

#[test]
fn udp_pending_write_retries_one_whole_datagram_including_empty_payloads() {
    let _serial = crate::string::serial();
    for data in [b"".as_slice(), b"whole-datagram".as_slice()] {
        let (id, _, peer) = pair();
        let mut operation = udp_write_operation(id, data, &peer.local_addr().unwrap().to_string());
        operation.udp_write_block_once = true;
        let (waiter, received) = signal();
        assert!(operation.poll(waiter.clone()).is_none());
        peer.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.recv_from(&mut [0; 32]).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
        peer.set_nonblocking(false).unwrap();
        let Output::Value(output) = finish(&mut operation, waiter, &received) else {
            panic!()
        };
        assert_eq!((output.failed, output.value), (0, data.len() as i64));
        let mut buffer = [0; 32];
        let (count, _) = peer.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..count], data);
        peer.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.recv_from(&mut buffer).unwrap_err().kind(),
            ErrorKind::WouldBlock
        );
        zore_native_net_close_handle(id);
    }
}

#[test]
fn udp_pending_read_registers_once_and_reports_zero_capacity_truncation() {
    let _serial = crate::string::serial();
    let (id, local, peer) = pair();
    let mut operation = udp_read_operation(id, 0);
    let (waiter, received) = signal();
    assert!(operation.poll(waiter.clone()).is_none());
    let registration = Arc::as_ptr(operation.waiting.as_ref().unwrap());
    for _ in 0..20 {
        assert!(operation.poll(waiter.clone()).is_none());
        assert_eq!(
            registration,
            Arc::as_ptr(operation.waiting.as_ref().unwrap())
        );
    }
    peer.send_to(b"discarded", local).unwrap();
    let packet = bytes(finish(&mut operation, waiter, &received));
    assert_eq!(packet[0], 1);
    assert_eq!(packet.len(), 2 + packet[1] as usize);
    assert_eq!(
        &packet[2..],
        peer.local_addr().unwrap().to_string().as_bytes()
    );
    peer.send_to(b"", local).unwrap();
    let packet = bytes(Output::Bytes(udp_read(id, 0)));
    assert_eq!(packet[0], 0);
    assert_eq!(packet.len(), 2 + packet[1] as usize);
    zore_native_net_close_handle(id);
}

#[test]
fn udp_expired_reads_preserve_packets_and_expired_empty_writes_send_nothing() {
    let _serial = crate::string::serial();
    let (id, local, peer) = pair();
    let handle = lookup(id).unwrap();
    let Handle::Datagram(socket) = handle.as_ref() else {
        panic!()
    };
    peer.send_to(b"queued", local).unwrap();
    socket.read_deadline.store(1, Ordering::SeqCst);
    let (waiter, _) = signal();
    let mut read = udp_read_operation(id, 64);
    assert_eq!(
        error(read.poll(waiter).unwrap()),
        "net.UDP.ReadFrom: timed out"
    );
    socket.read_deadline.store(0, Ordering::SeqCst);
    let packet = bytes(Output::Bytes(udp_read(id, 64)));
    assert_eq!(&packet[2 + packet[1] as usize..], b"queued");
    socket.write_deadline.store(1, Ordering::SeqCst);
    let mut write = udp_write_operation(id, b"", &peer.local_addr().unwrap().to_string());
    let (waiter, _) = signal();
    assert_eq!(
        error(write.poll(waiter).unwrap()),
        "net.UDP.WriteTo: timed out"
    );
    peer.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.recv_from(&mut [0; 1]).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    zore_native_net_close_handle(id);
}
