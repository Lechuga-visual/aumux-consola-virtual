//! Analysis runs on a worker. Radix-2 FFT avoids any DSP or platform dependency.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Default)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}
impl Complex {
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}
pub fn fft(v: &mut [Complex], inverse: bool) {
    let n = v.len();
    assert!(n.is_power_of_two());
    let mut j = 0;
    for i in 1..n {
        let mut b = n >> 1;
        while j & b != 0 {
            j ^= b;
            b >>= 1;
        }
        j ^= b;
        if i < j {
            v.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = if inverse { 1.0 } else { -1.0 } * std::f64::consts::TAU / len as f64;
        let step = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for base in (0..n).step_by(len) {
            let mut w = Complex { re: 1.0, im: 0.0 };
            for k in 0..len / 2 {
                let a = v[base + k];
                let b = v[base + k + len / 2].mul(w);
                v[base + k] = Complex {
                    re: a.re + b.re,
                    im: a.im + b.im,
                };
                v[base + k + len / 2] = Complex {
                    re: a.re - b.re,
                    im: a.im - b.im,
                };
                w = w.mul(step);
            }
        }
        len *= 2;
    }
    if inverse {
        for x in v {
            x.re /= n as f64;
            x.im /= n as f64;
        }
    }
}
pub const LEAD: usize = 256;
pub const PROBE: usize = 1024;
pub const CAPTURE: usize = 24000;
pub fn probe_sample(position: usize) -> f32 {
    if !(LEAD..LEAD + PROBE).contains(&position) {
        return 0.0;
    }
    let i = position - LEAD;
    let mut x = (i as u32 + 1).wrapping_mul(0x9e3779b9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85ebca6b);
    x ^= x >> 13;
    let edge = (i.min(PROBE - 1 - i) as f32 / 64.0).min(1.0);
    (if x & 1 == 0 { 1.0 } else { -1.0 }) * 0.06309573 * edge // -24 dBFS, tapered edges.
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub samples: Option<usize>,
    pub ms: Option<f64>,
    pub correlation: f64,
    pub inverted: bool,
    pub peak_dbfs: Option<f64>,
    pub error: Option<String>,
}
pub fn detect_probe(y: &[f32], gap: bool) -> ProbeResult {
    let mut result = ProbeResult {
        samples: None,
        ms: None,
        correlation: 0.0,
        inverted: false,
        peak_dbfs: None,
        error: None,
    };
    let peak = y.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    if peak > 0.0 {
        result.peak_dbfs = Some(20.0 * f64::from(peak).log10());
    }
    if gap || y.len() < LEAD + PROBE {
        result.error = Some("Captura incompleta o cola saturada; repite la medición".into());
        return result;
    }
    if peak >= 0.999 {
        result.error = Some("El retorno satura; reduce la ganancia de entrada".into());
        return result;
    }
    let n = (y.len() + PROBE).next_power_of_two();
    let mut x = vec![Complex::default(); n];
    let mut z = vec![Complex::default(); n];
    let mut reference_energy = 0.0;
    for i in 0..PROBE {
        let value = f64::from(probe_sample(LEAD + i));
        x[i].re = value;
        reference_energy += value * value;
    }
    for (z, &v) in z.iter_mut().zip(y) {
        z.re = f64::from(v);
    }
    fft(&mut x, false);
    fft(&mut z, false);
    for (z, x) in z.iter_mut().zip(x) {
        *z = z.mul(Complex {
            re: x.re,
            im: -x.im,
        });
    }
    fft(&mut z, true);
    let best = (LEAD..=y.len() - PROBE)
        .max_by(|&a, &b| z[a].re.abs().total_cmp(&z[b].re.abs()))
        .unwrap();
    let energy = y[best..best + PROBE]
        .iter()
        .map(|&v| f64::from(v).powi(2))
        .sum::<f64>();
    let corr = if energy > 1e-16 {
        z[best].re / (reference_energy * energy).sqrt()
    } else {
        0.0
    };
    result.correlation = corr.clamp(-1.0, 1.0);
    result.inverted = corr < 0.0;
    if corr.abs() < 0.35 || peak < 1e-5 {
        result.error=Some("No se detectó un retorno fiable. Comprueba cable, entrada y nivel; rango máximo aproximado 470 ms".into());
    } else {
        result.samples = Some(best - LEAD);
        result.ms = Some((best - LEAD) as f64 / 48.0);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_style_delays_inversion_noise_and_silence() {
        for delay in [0, 31, 257, 513, 8191, 22000] {
            let mut y = vec![0.0; CAPTURE];
            for (i, s) in y.iter_mut().enumerate().skip(delay) {
                *s =
                    probe_sample(i - delay) * -0.7 + ((i * 37 % 101) as f32 / 100.0 - 0.5) * 0.0001;
            }
            let result = detect_probe(&y, false);
            assert_eq!(result.samples, Some(delay), "{result:?}");
            assert!(result.inverted);
        }
        assert!(detect_probe(&vec![0.0; CAPTURE], false).samples.is_none());
        assert!(detect_probe(&vec![0.1; CAPTURE], true).samples.is_none());
        assert!(detect_probe(&vec![1.0; CAPTURE], false).samples.is_none());
    }
}
