use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use futures::prelude::*;
use net2;
use net2::unix::UnixTcpBuilderExt;
use tokio_core::net::TcpListener;
use tokio_core::reactor::*;

use self::addr_stream::AddrStream;

pub fn new_listener_reuseport(addr: &SocketAddr, handle: &Handle) -> io::Result<TcpListener> {
    // let listener = ::mio::net::TcpListener::bind(addr).unwrap();
    let sock = match *addr {
        SocketAddr::V4(..) => net2::TcpBuilder::new_v4(),
        SocketAddr::V6(..) => net2::TcpBuilder::new_v6(),
    }?;

    sock.reuse_address(true)?;
    sock.reuse_port(true)?;
    sock.bind(addr)?;

    let listener = sock.listen(1024)?;
    TcpListener::from_listener(listener, addr, handle)
}

/// A stream of connections from binding to an address.
#[must_use = "streams do nothing unless polled"]
#[derive(Debug)]
pub struct AddrIncoming {
    addr: SocketAddr,
    keep_alive_timeout: Option<Duration>,
    listener: TcpListener,
    handle: Handle,
    timeout: Option<Timeout>,
}

impl AddrIncoming {
    pub fn new(addr: SocketAddr, handle: Handle) -> io::Result<Self> {
        let listener = new_listener_reuseport(&addr, &handle)?;
        Ok(Self {
            addr,
            keep_alive_timeout: Some(Duration::from_secs(90)),
            listener,
            handle,
            timeout: None,
        })
    }

    /*
    /// Get the local address bound to this listener.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
    */
}

impl Stream for AddrIncoming {
    // currently unnameable...
    type Item = AddrStream;
    type Error = ::std::io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Check if a previous timeout is active that was set by IO errors.
        if let Some(ref mut to) = self.timeout {
            match to.poll().expect("timeout never fails") {
                Async::Ready(_) => {}
                Async::NotReady => return Ok(Async::NotReady),
            }
        }
        self.timeout = None;
        loop {
            match self.listener.accept() {
                Ok((socket, addr)) => {
                    if let Some(dur) = self.keep_alive_timeout {
                        if let Err(e) = socket.set_keepalive(Some(dur)) {
                            trace!("error trying to set TCP keepalive: {}", e);
                        }
                    }
                    return Ok(Async::Ready(Some(AddrStream::new(socket, addr))));
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(Async::NotReady),
                Err(ref e) => {
                    eprintln!("e: {:?}", e);

                    // Connection errors can be ignored directly, continue by
                    // accepting the next request.
                    if connection_error(e) {
                        continue;
                    }
                    // Sleep 10ms.
                    let delay = ::std::time::Duration::from_millis(10);
                    debug!("accept error: {}; sleeping {:?}", e, delay);
                    let mut timeout =
                        Timeout::new(delay, &self.handle).expect("can always set a timeout");
                    let result = timeout.poll().expect("timeout never fails");
                    match result {
                        Async::Ready(()) => continue,
                        Async::NotReady => {
                            self.timeout = Some(timeout);
                            return Ok(Async::NotReady);
                        }
                    }
                }
            }
        }
    }
}

/// This function defines errors that are per-connection. Which basically
/// means that if we get this error from `accept()` system call it means
/// next connection might be ready to be accepted.
///
/// All other errors will incur a timeout before next `accept()` is performed.
/// The timeout is useful to handle resource exhaustion errors like ENFILE
/// and EMFILE. Otherwise, could enter into tight loop.
fn connection_error(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::ConnectionRefused || e.kind() == io::ErrorKind::ConnectionAborted
        || e.kind() == io::ErrorKind::ConnectionReset
}

mod addr_stream {
    use bytes::{Buf, BufMut};
    use futures::Poll;
    use std::io::{self, Read, Write};
    use std::net::SocketAddr;
    use tokio_core::net::TcpStream;
    use tokio_io::{AsyncRead, AsyncWrite};

    #[derive(Debug)]
    pub struct AddrStream {
        inner: TcpStream,
        pub(super) remote_addr: SocketAddr,
    }

    impl AddrStream {
        pub(super) fn new(tcp: TcpStream, addr: SocketAddr) -> AddrStream {
            AddrStream {
                inner: tcp,
                remote_addr: addr,
            }
        }
    }

    impl Read for AddrStream {
        #[inline]
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl Write for AddrStream {
        #[inline]
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.write(buf)
        }

        #[inline]
        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl AsyncRead for AddrStream {
        #[inline]
        unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
            self.inner.prepare_uninitialized_buffer(buf)
        }

        #[inline]
        fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
            self.inner.read_buf(buf)
        }
    }

    impl AsyncWrite for AddrStream {
        #[inline]
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            AsyncWrite::shutdown(&mut self.inner)
        }

        #[inline]
        fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
            self.inner.write_buf(buf)
        }
    }
}
