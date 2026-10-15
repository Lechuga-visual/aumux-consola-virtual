use crate::config::Config;
use serde_json::{Value, json};
pub fn ms(samples: usize, rate: u32) -> f64 {
    samples as f64 * 1000.0 / rate as f64
}
pub fn report(
    c: &Config,
    delays: &[usize],
    frames: u32,
    driver: Option<u64>,
    running: bool,
    obs_packets: usize,
) -> Value {
    let channels = c.channels.len();
    let plugin = |i: usize| delays.get(i).copied().unwrap_or(0);
    let paths=c.buses.iter().enumerate().map(|(b,bus)|{
  let paths=c.channels.iter().enumerate().filter(|(_,ch)|!ch.mute&&(b==0||ch.sends_db[b-1]>-90.0&&!ch.send_mute.get(b-1).copied().unwrap_or(false))).map(|(i,ch)|json!({"channel":i,"name":ch.name,"plugin_samples":plugin(i)+plugin(channels+b),"plugin_ms":ms(plugin(i)+plugin(channels+b),c.sample_rate)})).collect::<Vec<_>>();
  json!({"index":b,"name":bus.name,"plugin_samples":plugin(channels+b),"plugin_ms":ms(plugin(channels+b),c.sample_rate),"paths":paths})
 }).collect::<Vec<_>>();
    json!({"sample_rate":c.sample_rate,"running":running,"block_frames":frames,"block_ms":ms(frames as usize,c.sample_rate),"driver_frames":driver,"driver_ms":driver.map(|v|ms(v as usize,c.sample_rate)),"channels":c.channels.iter().enumerate().map(|(i,ch)|json!({"name":ch.name,"plugin_samples":plugin(i),"plugin_ms":ms(plugin(i),c.sample_rate)})).collect::<Vec<_>>(),"buses":paths,"obs_queue_packets":obs_packets,"obs_queue_ms_upper_bound":ms(obs_packets*256,c.sample_rate),"obs_receiver_ms":Value::Null,"audio_network_one_way_ms":Value::Null,"end_to_end_measured_ms":Value::Null})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn block_and_route_delays_are_separate_from_unmeasured_audio_latency() {
        let mut c = Config::default();
        c.channels[0].sends_db[0] = 0.0;
        let mut delays = vec![0; c.channels.len() + c.buses.len()];
        delays[0] = 480;
        delays[c.channels.len() + 1] = 240;
        let l = report(&c, &delays, 256, Some(512), true, 2);
        assert!((l["block_ms"].as_f64().unwrap() - 5.333333).abs() < 1e-5);
        assert_eq!(l["driver_ms"], 512.0 / 48.0);
        assert_eq!(l["buses"][1]["paths"][0]["plugin_ms"], 15.0);
        assert!(l["end_to_end_measured_ms"].is_null());
    }
}
