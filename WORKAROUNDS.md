# Known Issues in VRChat's OSC/mDNS Implementation and Workarounds in This Crate

This document describes known issues in VRChat's OSC/OSCQuery and mDNS implementation, and how the `vrchat_osc` crate works around them.

---

## Issue 1: Advertising 127.0.0.1 on All Interfaces

### Problem Details
In VRChat, OSC is bound to `0.0.0.0` (all interfaces), while OSCQuery is bound to `127.0.0.1` (loopback). However, the IP address advertised via mDNS is `127.0.0.1` for all interfaces, regardless of which interface the service is actually bound to.

This results in VRChat's services being discoverable via mDNS, even though they are unreachable from external networks.

### Workaround in This Crate

[`VRChatOSC::new()`](file:///d:/Develop/Data/2025/First_half/0203vrchat_osc/vrchat_osc/src/lib.rs#L135-L192) accepts an `osc_ip` parameter (VRChat's IP address) and uses the [`find_local_ip_for_destination`](file:///d:/Develop/Data/2025/First_half/0203vrchat_osc/vrchat_osc/src/lib.rs#L108-L123) function to determine the local interface IP that can reach that destination.

```rust
// Let the OS routing table determine the appropriate interface
fn find_local_ip_for_destination(dest_ip: IpAddr) -> Result<IpAddr, Error> {
    if dest_ip.is_loopback() {
        return Ok(dest_ip);
    }
    let socket = match dest_ip { ... };
    socket.connect((dest_ip, 0))?;
    Ok(socket.local_addr()?.ip())
}
```

This `advertised_ip` is passed to the [mDNS module](file:///d:/Develop/Data/2025/First_half/0203vrchat_osc/vrchat_osc/src/mdns.rs#L150) to advertise the correct IP address in A/AAAA records.

---

## Issue 2: Duplicate Service Registration from First A Record

### Problem Details
VRChat registers the IP address from the first A record in an mDNS response as a service, regardless of the domain name. When mDNS responses are sent from multiple network interfaces, each interface's A record gets registered, causing duplicate service entries.

### Workaround in This Crate

The [`setup_multicast_socket`](file:///d:/Develop/Data/2025/First_half/0203vrchat_osc/vrchat_osc/src/mdns/utils.rs#L58-L113) function joins the multicast group and sets the multicast transmission interface for **only** the single `advertised_ip` interface.

```rust
// For IPv4
socket.join_multicast_v4(&MDNS_IPV4_ADDR, &ipv4)?;
socket.set_multicast_if_v4(&ipv4)?;

// For IPv6
socket.join_multicast_v6(&MDNS_IPV6_ADDR, if_index)?;
socket.set_multicast_if_v6(if_index)?;
```

This ensures mDNS responses are sent from **only a single interface**, preventing duplicate service registration on the VRChat side.

---

## Issue 3: PTR Record Required in Answers Section

### Problem Details
VRChat does not recognize a service unless the mDNS response contains a PTR record in the **Answers section**. Standard mDNS implementations may place PTR records in the Additional section, but VRChat does not accept this.

### Workaround in This Crate

The [`create_mdns_response_message`](file:///d:/Develop/Data/2025/First_half/0203vrchat_osc/vrchat_osc/src/mdns/utils.rs#L181-L234) function explicitly places the PTR record in the **Answers section**.

```rust
pub fn create_mdns_response_message(
    instance_name: &Name,
    interface_ip: IpAddr,
    port: u16,
) -> Message {
    let mut message = Message::new();
    // ...

    // Add PTR record to Answers section
    let service_type_name = instance_name.trim_to(3);
    message.add_answer(Record::from_rdata(
        service_type_name.clone(),
        RECORD_TTL,
        RData::PTR(PTR(instance_name.clone())),
    ));

    // SRV and A/AAAA records go in Additional section
    message.add_additional(Record::from_rdata(
        instance_name.clone(),
        RECORD_TTL,
        RData::SRV(SRV::new(0, 0, port, instance_name.clone())),
    ));
    // A/AAAA record...
    message
}
```

Similarly, the [`extract_service_info`](file:///d:/Develop/Data/2025/First_half/0203vrchat_osc/vrchat_osc/src/mdns/utils.rs#L247-L346) function requires a PTR record when parsing responses to extract service information.
