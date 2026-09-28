//! Thread-per-core execution: CPU pinning and `SO_REUSEPORT` listeners.
//!
//! [`pin_current_thread_to`] pins via `sched_setaffinity` on Linux and is a
//! documented no-op elsewhere, where no stable affinity API sits behind the
//! crates already in the graph. [`reuseport_listener`] builds one shard of
//! a sharded accept set: every core binds the same address and the kernel
//! sprays connections across the shards, so each connection is accepted,
//! served, and drained without ever migrating threads.

use std::io;
use std::net::{SocketAddr, TcpListener};

/// Pin the calling thread to `core`; returns whether the pin stuck.
///
/// Out-of-range cores fail closed (`false`): pretending to pin while the
/// thread roams would make sharding claims a lie. Non-Linux hosts always
/// report `false`; the per-core mode still works there, just unpinned.
pub(crate) fn pin_current_thread_to(core: usize) -> bool {
    imp::pin_current_thread_to(core)
}

/// Bind `addr` for one accept shard: `SO_REUSEADDRESS` plus `SO_REUSEPORT`,
/// with the same 1024 backlog Tokio binds by default.
///
/// Returns a std listener on purpose: `tokio::net::TcpListener::from_std`
/// registers with the ambient reactor, so each shard converts its own
/// listener on its own thread. Converting here would pin every shard's
/// readiness to the caller's reactor and stall the shards whenever it
/// stops polling.
pub(crate) fn reuseport_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let domain = if addr.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

#[cfg(target_os = "linux")]
mod imp {
    #[allow(
        unsafe_code,
        reason = "raw sched_setaffinity; no safe API behind the crates in the graph"
    )]
    pub(super) fn pin_current_thread_to(core: usize) -> bool {
        if core >= 8 * std::mem::size_of::<libc::cpu_set_t>() {
            return false;
        }
        // SAFETY: `sched_setaffinity` with pid 0 targets the calling thread
        // only. The mask is a stack `cpu_set_t` sized by `size_of`, zeroed
        // before setting one bit, so the kernel reads exactly the CPU we
        // name. Failure (unknown CPU, sandbox) returns false; nothing traps.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_SET(core, &mut set);
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub(super) fn pin_current_thread_to(_core: usize) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{pin_current_thread_to, reuseport_listener};
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn reuseport_allows_two_shards_on_one_address() {
        let first = reuseport_listener(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0).into())
            .expect("first shard binds");
        let addr = first.local_addr().expect("first shard has an address");
        let _second = reuseport_listener(addr).expect("second shard shares the address");
    }

    #[test]
    fn out_of_range_cores_fail_closed() {
        // No thread is ever harmed: the lookup rejects before any syscall.
        assert!(!pin_current_thread_to(usize::MAX));
    }

    #[test]
    fn pinning_runs_on_its_own_thread() {
        // Pin the probe thread, never a test-runner thread. Linux pins to
        // the CPU it already runs on, so cpusets cannot fail the test.
        let pinned = std::thread::spawn(|| {
            #[cfg(target_os = "linux")]
            {
                // SAFETY: `sched_getcpu` takes no pointers and only reads
                // scheduler state.
                let cpu = unsafe { libc::sched_getcpu() };
                let Ok(cpu) = usize::try_from(cpu) else {
                    return false;
                };
                pin_current_thread_to(cpu)
            }
            #[cfg(not(target_os = "linux"))]
            {
                pin_current_thread_to(0)
            }
        })
        .join()
        .expect("probe thread joins");
        assert_eq!(pinned, cfg!(target_os = "linux"));
    }
}
