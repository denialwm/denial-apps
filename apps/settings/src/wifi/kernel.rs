//! An interface's addresses, its default gateway and the name servers, as
//! the kernel and resolv.conf have them: for iwd, which leaves addressing to
//! the system and does not report it.

use std::ffi::CStr;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Default)]
pub struct Addresses {
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
}

/// `interface`'s addresses, as address/prefix.
pub fn addresses(interface: &str) -> Addresses {
    let mut found = Addresses::default();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list` with a list freed below.
    if unsafe { libc::getifaddrs(&mut list) } != 0 {
        return found;
    }
    let mut entry = list;
    while !entry.is_null() {
        // SAFETY: entries of the list stay valid until freeifaddrs.
        let ifa = unsafe { &*entry };
        entry = ifa.ifa_next;
        if ifa.ifa_addr.is_null() || ifa.ifa_netmask.is_null() {
            continue;
        }
        // SAFETY: ifa_name is a NUL-terminated string.
        let name = unsafe { CStr::from_ptr(ifa.ifa_name) };
        if name.to_bytes() != interface.as_bytes() {
            continue;
        }
        // SAFETY: the family says which sockaddr the pointers hold.
        unsafe {
            match i32::from((*ifa.ifa_addr).sa_family) {
                libc::AF_INET => {
                    let address = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let mask = &*(ifa.ifa_netmask as *const libc::sockaddr_in);
                    let address = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
                    let prefix = u32::from_be(mask.sin_addr.s_addr).count_ones();
                    found.ipv4.push(format!("{address}/{prefix}"));
                }
                libc::AF_INET6 => {
                    let address = &*(ifa.ifa_addr as *const libc::sockaddr_in6);
                    let mask = &*(ifa.ifa_netmask as *const libc::sockaddr_in6);
                    let address = Ipv6Addr::from(address.sin6_addr.s6_addr);
                    let prefix: u32 = mask.sin6_addr.s6_addr.iter().map(|b| b.count_ones()).sum();
                    found.ipv6.push(format!("{address}/{prefix}"));
                }
                _ => {}
            }
        }
    }
    // SAFETY: `list` came from getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(list) };
    found
}

/// The default route's gateway through `interface`, from /proc/net/route.
pub fn gateway(interface: &str) -> Option<String> {
    let routes = std::fs::read_to_string("/proc/net/route").ok()?;
    routes.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 || fields[0] != interface || fields[1] != "00000000" {
            return None;
        }
        // The address's bytes, read as one number in the kernel's byte order.
        let gateway = u32::from_str_radix(fields[2], 16).ok()?;
        Some(Ipv4Addr::from(gateway.to_ne_bytes()).to_string())
    })
}

/// The name servers in /etc/resolv.conf.
pub fn nameservers() -> Vec<String> {
    std::fs::read_to_string("/etc/resolv.conf")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .map(|server| server.trim().to_owned())
        .collect()
}
