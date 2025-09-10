use std::os::fd::{OwnedFd, RawFd};
use nix::fcntl;
use nix::errno::Errno;
use nix::sys::sendfile::sendfile64;
use nix::sys::epoll::*;
//use nix::sys::socket::*;
use nix::sys::socket::{accept4, bind, listen, socket, setsockopt, send, recvfrom, AddressFamily, SockType, SockFlag, Backlog, MsgFlags};
use nix::sys::socket::sockopt;
use nix::sys::socket::SockaddrIn;
use nix::sys::stat::fstat;
use nix::unistd::close;
use std::os::unix::io::AsRawFd;
use nix::poll::PollTimeout;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;

const ERR404: &str = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 17\r\n\r\nFile not found.\r\n";
const ERR414: &str = "HTTP/1.1 414 URI Too Long\r\nContent-Type: text/plain\r\nContent-Length: 57\r\n\r\nThe URI provided is too long for the server to process.\r\n";
const ERR405: &str = "HTTP/1.1 405 Method Not Allowed\r\nAllow: GET\r\nContent-Type: text/plain\r\nContent-Length: 37\r\n\r\nOnly GET/HEAD requests are allowed.\r\n";
const ERR500: &str = "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: 40\r\n\r\nAn internal server error has occurred.\r\n";

// Size of send and receive buffers, in bytes.
const BUF_SIZE: usize = 1024 * 1024 * 1;

const EXTS: [(&str, &str); 9] = [
    // ext[n] = extension
    // ext[n]
    (".css",     "text/css"),
    (".html",    "text/html"),
    (".jpeg",    "image/jpeg"),
    (".jpg",     "image/jpeg"),
    (".json",    "application/json"),
    (".png",     "image/png"),
    (".txt",     "text/plain"),
    (".webm",    "video/webm"),
    (".woff",    "font/woff"),
];

const BACKLOG: i32 = 50;
const PORT: u16 = 8080;

fn log(msg: &str) {
    println!("{}", msg);
    //printf("%s:%d:%s(): %s\n", __FILE__, __LINE__, __func__, msg);
}

fn watch_socket(epoll_fd: &Epoll, sock_fd: std::os::fd::BorrowedFd) -> nix::Result<()> {
    let event: EpollEvent = EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLET, sock_fd.as_raw_fd() as u64);

    epoll_fd.add(&sock_fd, event)
}

/*
fn pabort(msg: &str) {
    //std::libc::perror(msg);
    std::process::exit(1);
}
*/
fn reroot(_root: &str) {
/*
    if (syscall(SYS_unshare, CLONE_NEWUSER | CLONE_NEWNS))
        pabort("unshare");

    if (mount(root, root, NULL, MS_BIND | MS_PRIVATE, NULL))
        pabort("mount");

    if (syscall(SYS_pivot_root, root, root))
        pabort("pivot_root");

    if (chdir("/"))
        pabort("chdir");
*/
}

fn server_socket() -> nix::Result<OwnedFd> {
    let server_fd = socket(
        AddressFamily::Inet,
        SockType::Stream,
        SockFlag::SOCK_NONBLOCK & SockFlag::SOCK_CLOEXEC,
        None
    )?;
    setsockopt(&server_fd, sockopt::ReuseAddr, &true)?;
    //fcntl::fcntl(server_fd, fcntl::FcntlArg::F_SETFL(fcntl::OFlag::O_NONBLOCK)).unwrap();

    let addr = SockaddrIn::new(0, 0, 0, 0, PORT);
    bind(server_fd.as_raw_fd(), &addr)?;
    listen(&server_fd, Backlog::new(BACKLOG)?)?; // FIXME: Backlog::MAXCONN?

    Ok(server_fd)
}

/*
fn setcork(fd: RawFd, optval: isize) {
//    if (setsockopt(fd, SOL_TCP, TCP_CORK, &optval, sizeof(int)))
//        perror("setcork/setsockopt");
}
*/

fn send_chunk(fd: RawFd, response: &str) {
    let buf: &[u8] = response.as_bytes();
    send(fd, buf, MsgFlags::MSG_NOSIGNAL).unwrap();
}

fn main() -> Result<(), std::io::Error> {
    let epoll_fd = Epoll::new(EpollCreateFlags::empty())?;

    let server_fd = server_socket()?;
    log("Got server socket.");

    watch_socket(&epoll_fd, server_fd.as_fd())?;
    log("Watching server_fd.");

    reroot("site");
    log("Isolated ./site as process mount root.");

    let mut events = [EpollEvent::empty()];

    let mut recvbuf: [u8; BUF_SIZE] = [0; BUF_SIZE];
    //static char recvbuf[BUF_SIZE] = {0};
    //static char sendbuf[BUF_SIZE] = {0};
    loop {
        let Ok(_num_events) = epoll_fd.wait(&mut events, PollTimeout::NONE) else {
            //perror("epoll_wait");
            println!("??? num_events/epoll_wait()");
            continue;
        };

        for event in events {
            let fd = event.data() as RawFd;
            let bfd = unsafe { BorrowedFd::borrow_raw(fd) };

            if !event.events().contains(EpollFlags::EPOLLIN) {
                //perror("epoll_wait");
                println!("??? epoll_wait()/not EPOLLIN?");
                let _ = close(fd);
                continue;
            }

            if server_fd.as_raw_fd() == fd {
                loop {
                    let Ok(client_fd) = accept4(server_fd.as_raw_fd(), SockFlag::SOCK_NONBLOCK) else {
                        let errno = Errno::last();
                        // EAGAIN/EWOULDBLOCK aren't actual errors, so
                        // be quiet about it.
                        if (errno != Errno::EAGAIN) && (errno != Errno::EWOULDBLOCK) {
                            //perror("accept");
                            println!("error in accept()");
                        }
                        break;
                    };


                    let client_fd_fucked = unsafe { std::os::fd::BorrowedFd::borrow_raw(client_fd) };
                    watch_socket(&epoll_fd, client_fd_fucked)?;
                }
                continue;
            }

            let Ok((_count, _)) = recvfrom::<()>(fd, &mut recvbuf) else {
                let errno = Errno::last();
                if errno != Errno::UnknownErrno && errno != Errno::EAGAIN && errno != Errno::EBADF {
                    // if count == -1, read() failed.
                    // we don't care about the following errno values:
                    // - 0      (there was no error)
                    // - EAGAIN ("resource temporarily unavailable"
                    // - EBADF  ("bad file descriptor")
                    println!("error in recvfrom()");
                }
                continue;
            };

            let mut parts = str::from_utf8(&recvbuf).unwrap_or("").split(" ");
            let Some(method) = parts.next() else {
                println!("??? no method???");
                continue;
            };
            let (Some(path), Some(_version)) = (parts.next(), parts.next()) else {
                // The path didn't fit in recvbuf, meaning it was too long.
                send_chunk(fd, ERR414);
                let _ = close(fd);
                continue;
            };

            let is_get = method == "GET";
            let is_head = method == "HEAD";
            if !is_get && !is_head {
                // Non-GET/HEAD requests get a 405 error.
                send_chunk(fd, ERR405);
                let _ = close(fd);
                continue;
            }

            // GET or HEAD request.

            let ends_with_slash = &path[path.len() - 1..1] == "/";
            let path =
                if ends_with_slash {
                    // Return /:dir/index.html for /:dir/.
                    format!("{}/index.html", path)
                } else {
                    String::from(path)
                };

            let Ok(file_fd) = fcntl::open(path.as_str(), fcntl::OFlag::O_RDONLY, nix::sys::stat::Mode::empty()) else {
                println!("error in open()");
                let errno = Errno::last();
                if errno == Errno::ENOENT { // No such path - return 404.
                    send_chunk(fd, ERR404);
                } else { // All other errors - return 500.
                    send_chunk(fd, ERR500);
                }
                let _ = close(fd);
                continue;

            };

            let Ok(st) = fstat(&file_fd) else {
                //perror("fstat");
                println!("error in fstat()");
                let _ = close(file_fd);
                send_chunk(fd, ERR500);
                let _ = close(fd);
                continue;
            };

            // Redirect /:dir to /:dir/.
            // TODO: Try reusing the already-open `file_fd`, like in the C version.
            //if !ends_with_slash && ((st.st_mode & SFlag::S_IFMT) == SFlag::S_IFDIR) {
            if !ends_with_slash && std::fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false) {
                let _ = close(file_fd);
                send_chunk(fd, &format!(
                    "HTTP/1.1 307 Temporary Redirect\r\n\
                    Location: {}/\r\n\
                    Content-Length: 0\r\n\
                    \r\n",
                    path
                ));
                let _ = close(fd);
                continue;
            }

            let mut content_type: &str = "application/octet-stream";

            let mut parts = path.split(".");
            if let (Some(_name), Some(ext)) = (&parts.next(), &parts.last()) {
                for i in 0..EXTS.len() {
                    if &EXTS[i].0 == ext {
                        content_type = EXTS[i].1;
                        break;
                    }
                }
            }

            let content_length = st.st_size;
            let sendbuf = format!(
                    "HTTP/1.1 200 OK\r\n\
                    Content-Type: {}\r\n\
                    Content-Length: {}\r\n\
                    Connection: close\r\n\
                    Server: chttpd <https://github.com/duckinator/chttpd>\r\n\
                    \r\n",
                    content_type, content_length);

            //setcork(fd, 1); // put a cork in it

            send_chunk(fd, &sendbuf);

            // For HEAD requests, only return the headers.
            if is_get {
                loop {
                    match sendfile64(bfd, &file_fd, None, content_length as usize) {
                        Ok(ret) => {
                            if ret == 0 {
                                break;
                            }
                        }
                        Err(e) => {
                            if Errno::last() != Errno::EAGAIN {
                                println!("error in sendfile(): {}", e);
                                //perror("sendfile");
                                break;
                            }
                        }
                    }
                }
            }

            //setcork(fd, 0); // release it all

            let _ = close(file_fd);

            // Read the rest of the request to avoid "connection reset by peer."
            let mut buf: [u8; 1024] = [0; 1024];
            loop {
                let Ok((ret, _)) = recvfrom::<()>(fd, &mut buf) else { break; };
                if ret == 0 {
                    break;
                }
            }

            let _ = close(fd);
        }
    }
}
