//! Windowed spectral analysis; instantiated and run exclusively on the worker.
use super::signal::{Complex, fft};
use serde_json::{Value, json};
pub const WINDOW: usize = 4096;
pub const HOP: usize = 2048;
pub struct Analyzer {
    hold: Vec<u32>,
}
impl Analyzer {
    pub fn new() -> Self {
        Self {
            hold: vec![0; WINDOW / 2],
        }
    }
    #[cfg(test)]
    pub fn analyze(&mut self, a: &[f32], b: &[f32]) -> Value {
        self.analyze_window(a, b, WINDOW, false)
    }
    pub fn analyze_window(&mut self, a: &[f32], b: &[f32], visible: usize, rising: bool) -> Value {
        assert_eq!(a.len(), WINDOW);
        assert_eq!(b.len(), WINDOW);
        let mut left = vec![Complex::default(); WINDOW];
        let mut right = left.clone();
        let mut sum = 0.0;
        for i in 0..WINDOW {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / (WINDOW - 1) as f64).cos();
            left[i].re = f64::from(a[i]) * w;
            right[i].re = f64::from(b[i]) * w;
            sum += w;
        }
        fft(&mut left, false);
        fft(&mut right, false);
        let power = left
            .iter()
            .zip(&right)
            .take(WINDOW / 2)
            .map(|(a, b)| (a.re * a.re + a.im * a.im).max(b.re * b.re + b.im * b.im))
            .collect::<Vec<_>>();
        let total = power.iter().sum::<f64>().max(1e-30);
        let mut next = vec![0u32; WINDOW / 2];
        let mut candidates = Vec::new();
        for i in 8..WINDOW / 2 - 16 {
            if power[i] < power[i - 1] || power[i] <= power[i + 1] {
                continue;
            }
            let background = (3..=12)
                .map(|j| power[i.saturating_sub(j)] + power[(i + j).min(power.len() - 1)])
                .sum::<f64>()
                / 20.0;
            let prominence = 10.0 * (power[i].max(1e-30) / background.max(1e-30)).log10();
            let db = 20.0 * (2.0 * power[i].sqrt() / sum).max(1e-15).log10();
            if prominence < 14.0 || db < -60.0 || power[i] / total < 0.08 {
                continue;
            }
            let previous = self.hold[i - 2..=i + 2].iter().copied().max().unwrap_or(0);
            next[i] = previous.saturating_add(1);
            if next[i] >= 15 {
                let (p, q, r) = (
                    power[i - 1].max(1e-30).ln(),
                    power[i].max(1e-30).ln(),
                    power[i + 1].max(1e-30).ln(),
                );
                let offset = (0.5 * (p - r) / (p - 2.0 * q + r)).clamp(-0.5, 0.5);
                candidates.push(json!({"frequency_hz":(i as f64+offset)*48000.0/WINDOW as f64,"dbfs":db,"prominence_db":prominence,"sustained_ms":next[i]as f64*HOP as f64/48.0,"classification":"possible_feedback_not_proof"}));
            }
        }
        self.hold = next;
        candidates.sort_by(|a, b| {
            b["dbfs"]
                .as_f64()
                .unwrap()
                .total_cmp(&a["dbfs"].as_f64().unwrap())
        });
        candidates.truncate(6);
        let ea = a.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
        let eb = b.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
        let cross = a
            .iter()
            .zip(b)
            .map(|(&a, &b)| f64::from(a) * f64::from(b))
            .sum::<f64>();
        let sa = a.iter().map(|&v| f64::from(v)).sum::<f64>();
        let sb = b.iter().map(|&v| f64::from(v)).sum::<f64>();
        let ac_a = (ea - sa * sa / WINDOW as f64).max(0.0);
        let ac_b = (eb - sb * sb / WINDOW as f64).max(0.0);
        let correlation = if ac_a > WINDOW as f64 * 1e-10 && ac_b > WINDOW as f64 * 1e-10 {
            Some(((cross - sa * sb / WINDOW as f64) / (ac_a * ac_b).sqrt()).clamp(-1.0, 1.0))
        } else {
            None
        };
        let rms = |e: f64| (e / WINDOW as f64).max(0.0).sqrt();
        let db = |amplitude: f64| {
            if amplitude > 1e-8 {
                Some(20.0 * amplitude.log10())
            } else {
                None
            }
        };
        let mono = rms((ea + eb + 2.0 * cross) * 0.25);
        let reference = rms((ea + eb) * 0.5);
        let mono_loss_db = if reference > 1e-8 {
            Some(20.0 * (mono / reference).max(1e-8).log10())
        } else {
            None
        };
        let dominant = (2..WINDOW / 2)
            .max_by(|&i, &j| power[i].total_cmp(&power[j]))
            .unwrap();
        let l = left[dominant];
        let r = right[dominant];
        let real = l.re * r.re + l.im * r.im;
        let imag = l.re * r.im - l.im * r.re;
        let phase = if (l.re * l.re + l.im * l.im).min(r.re * r.re + r.im * r.im) > 1e-10 {
            Some(imag.atan2(real).to_degrees())
        } else {
            None
        };
        let visible = visible.clamp(48, WINDOW);
        let step = visible.div_ceil(512);
        let start = if rising {
            (1..=(WINDOW - visible).min(WINDOW / 2))
                .find(|&i| a[i - 1] < 0.0 && a[i] >= 0.0)
                .unwrap_or(0)
        } else {
            0
        };
        let mut points = Vec::with_capacity(visible.div_ceil(step));
        for base in (start..start + visible).step_by(step) {
            let range = |lane: &[f32]| {
                lane[base..(base + step).min(start + visible)]
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let (al, ah) = range(a);
            let (bl, bh) = range(b);
            points.push([al, ah, bl, bh, a[base], b[base]]);
        }
        let trigger = if start > 0 { Some(start) } else { None };
        json!({"window_frames":WINDOW,"window_ms":WINDOW as f64/48.0,"hop_frames":HOP,"frequency_resolution_hz":48000.0/WINDOW as f64,"scope":{"bucket_frames":step,"visible_frames":visible,"start_frame":start,"points":points,"trigger_frame":trigger,"peak_a":a.iter().fold(0.0f32,|p,&s|p.max(s.abs())),"peak_b":b.iter().fold(0.0f32,|p,&s|p.max(s.abs()))},"feedback_candidates":candidates,"phase":{"correlation":correlation,"rms_a_dbfs":db(rms(ea)),"rms_b_dbfs":db(rms(eb)),"mono_dbfs":db(mono),"mono_relative_db":mono_loss_db,"dominant_phase_deg":phase,"dominant_frequency_hz":dominant as f64*48000.0/WINDOW as f64,"meaning":"AC_correlation_not_a_polarity_wiring_verdict"}})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_sustained_tone_but_not_silence_or_short_transient() {
        let mut analyzer = Analyzer::new();
        let a = (0..WINDOW)
            .map(|i| 0.1 * (std::f64::consts::TAU * i as f64 / 48.0).sin() as f32)
            .collect::<Vec<_>>();
        let first = analyzer.analyze(&a, &a);
        assert!(first["feedback_candidates"].as_array().unwrap().is_empty());
        let mut result = Value::Null;
        for _ in 0..16 {
            result = analyzer.analyze(&a, &a);
        }
        let hz = result["feedback_candidates"][0]["frequency_hz"]
            .as_f64()
            .unwrap();
        assert!((hz - 1000.0).abs() < 3.0, "{result}");
        let silence = vec![0.0; WINDOW];
        assert!(
            analyzer.analyze(&silence, &silence)["feedback_candidates"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let mut random = crate::generator::Generator::new(9);
        let mut noise = vec![0.0; WINDOW];
        for _ in 0..20 {
            for n in &mut noise {
                *n = random.next(crate::config::Signal::White, 1000.0, 48000.0) * 0.1;
            }
            assert!(
                analyzer.analyze(&noise, &noise)["feedback_candidates"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[cfg(test)]
mod phase_tests {
    use super::*;
    #[test]
    fn distinguishes_polarity_quadrature_silence_and_dc() {
        let a = (0..WINDOW)
            .map(|i| 0.2 * (std::f64::consts::TAU * i as f64 / 64.0).sin() as f32)
            .collect::<Vec<_>>();
        let inverse = a.iter().map(|v| -v).collect::<Vec<_>>();
        let q = (0..WINDOW)
            .map(|i| 0.2 * (std::f64::consts::TAU * i as f64 / 64.0).cos() as f32)
            .collect::<Vec<_>>();
        let mut analyzer = Analyzer::new();
        let same = analyzer.analyze(&a, &a);
        assert!((same["phase"]["correlation"].as_f64().unwrap() - 1.0).abs() < 1e-5);
        let opposite = analyzer.analyze(&a, &inverse);
        assert!((opposite["phase"]["correlation"].as_f64().unwrap() + 1.0).abs() < 1e-5);
        assert!(opposite["phase"]["mono_relative_db"].as_f64().unwrap() < -100.0);
        let shifted = analyzer.analyze(&a, &q);
        assert!(shifted["phase"]["correlation"].as_f64().unwrap().abs() < 0.001);
        assert!(
            (shifted["phase"]["dominant_phase_deg"]
                .as_f64()
                .unwrap()
                .abs()
                - 90.0)
                .abs()
                < 0.1
        );
        let constant = vec![0.1; WINDOW];
        assert!(analyzer.analyze(&constant, &constant)["phase"]["correlation"].is_null());
        assert!(analyzer.analyze(&vec![0.0; WINDOW], &a)["phase"]["correlation"].is_null());
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn downsampled_envelope_preserves_short_peaks_and_real_time_axis() {
        let mut a = vec![0.0; WINDOW];
        let b = vec![0.0; WINDOW];
        a[777] = 0.9;
        a[778] = -0.8;
        let v = Analyzer::new().analyze(&a, &b);
        assert_eq!(v["scope"]["bucket_frames"], 8);
        assert_eq!(v["scope"]["points"].as_array().unwrap().len(), 512);
        let bucket = &v["scope"]["points"][777 / 8];
        assert!((bucket[0].as_f64().unwrap() + 0.8).abs() < 1e-6);
        assert!((bucket[1].as_f64().unwrap() - 0.9).abs() < 1e-6);
        assert_eq!(v["window_frames"], 4096);
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;
    #[test]
    fn scope_zoom_preserves_true_samples_and_never_exceeds_512_buckets() {
        let a = (0..WINDOW)
            .map(|i| 0.2 * (std::f64::consts::TAU * i as f64 / 48.0).sin() as f32)
            .collect::<Vec<_>>();
        for visible in [240, 480, 960, 2400, 4096] {
            let v = Analyzer::new().analyze_window(&a, &a, visible, true);
            let s = &v["scope"];
            let step = s["bucket_frames"].as_u64().unwrap() as usize;
            let start = s["start_frame"].as_u64().unwrap() as usize;
            let points = s["points"].as_array().unwrap();
            assert!(points.len() <= 512);
            assert_eq!(s["visible_frames"], visible);
            assert!(start + visible <= WINDOW);
            for (i, p) in points.iter().enumerate() {
                assert!((p[4].as_f64().unwrap() - f64::from(a[start + i * step])).abs() < 1e-8);
            }
        }
    }
}
