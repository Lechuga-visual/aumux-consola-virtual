use serde_json::{Value, json};
pub fn addresses() -> Value {
    match if_addrs::get_if_addrs() {
        Ok(list) => {
            json!({"interfaces":list.into_iter().filter(|a|!a.ip().is_unspecified()).map(|a|{let ip=a.ip();let tailscale=a.name.to_lowercase().contains("tailscale")||match ip{std::net::IpAddr::V4(v)=>{let b=v.octets();b[0]==100&&(64..=127).contains(&b[1])},std::net::IpAddr::V6(v)=>v.segments()[0]==0xfd7a};json!({"name":a.name,"ip":ip.to_string(),"loopback":ip.is_loopback(),"family":if ip.is_ipv4(){"IPv4"}else{"IPv6"},"tailscale_candidate":tailscale})}).collect::<Vec<_>>() })
        }
        Err(e) => json!({"interfaces":[],"error":e.to_string()}),
    }
}
