use anyhow::{bail, ensure, Context, Result};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

pub fn parse_mac_address(mac: &str) -> Result<[u8; 6]> {
    let cleaned: Vec<&str> = mac.split(|c| c == ':' || c == '-').collect();
    ensure!(cleaned.len() == 6, "MAC 地址格式应为 AA:BB:CC:DD:EE:FF");
    let mut bytes = [0u8; 6];
    for (idx, part) in cleaned.iter().enumerate() {
        ensure!(part.len() == 2, "MAC 地址字节段长度错误: {part}");
        bytes[idx] = u8::from_str_radix(part, 16)
            .with_context(|| format!("无效十六进制字节: {part}"))?;
    }
    Ok(bytes)
}

pub fn build_magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut pkt = [0xFFu8; 102];
    for i in 0..16 {
        let offset = 6 + i * 6;
        pkt[offset..offset + 6].copy_from_slice(&mac);
    }
    pkt
}

pub fn send_wol_broadcast(local_lan_ip: Ipv4Addr, broadcast_ip: Ipv4Addr, mac_str: &str) -> Result<()> {
    if broadcast_ip.octets()[0] == 100 && (64..=127).contains(&broadcast_ip.octets()[1]) {
        bail!("禁止向 Tailscale 网段发送唤醒包");
    }
    let mac = parse_mac_address(mac_str)?;
    let pkt = build_magic_packet(mac);
    let sock = UdpSocket::bind(SocketAddrV4::new(local_lan_ip, 0))
        .context("绑定局域网 UDP 套接字失败")?;
    sock.set_broadcast(true)?;
    sock.send_to(&pkt, SocketAddrV4::new(broadcast_ip, 9))
        .context("发送魔术唤醒包失败")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_102_byte_magic_packet() {
        let mac = parse_mac_address("A0-B1-C2-D3-E4-F5").unwrap();
        assert_eq!(mac, [0xA0, 0xB1, 0xC2, 0xD3, 0xE4, 0xF5]);
        let pkt = build_magic_packet(mac);
        assert_eq!(pkt.len(), 102);
        assert_eq!(&pkt[0..6], &[0xFF; 6]);
        assert_eq!(&pkt[6..12], &mac);
        assert_eq!(&pkt[96..102], &mac);
    }
}
