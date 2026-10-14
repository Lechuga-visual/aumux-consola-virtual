use crate::config::Signal;

/// Independent deterministic streams; no allocation, system RNG or locks in audio.
pub struct Generator {
    random: u32,
    pink: [f32; 7],
    phase: f64,
}
impl Generator {
    pub fn new(channel: usize) -> Self {
        Self {
            random: 0x6D2B79F5u32.wrapping_mul(channel as u32 + 1),
            pink: [0.0; 7],
            phase: 0.0,
        }
    }
    pub fn next(&mut self, signal: Signal, frequency: f32, rate: f32) -> f32 {
        if signal == Signal::Sine {
            let sample = (self.phase * std::f64::consts::TAU).sin() as f32;
            self.phase = (self.phase + f64::from(frequency / rate)).fract();
            return sample;
        }
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        let white = (self.random as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32;
        match signal {
            Signal::Off => 0.0,
            Signal::White => white,
            Signal::Pink => {
                // Paul Kellet's pink-noise filter, bounded to nominal full scale.
                let b = &mut self.pink;
                b[0] = 0.99886 * b[0] + white * 0.0555179;
                b[1] = 0.99332 * b[1] + white * 0.0750759;
                b[2] = 0.96900 * b[2] + white * 0.1538520;
                b[3] = 0.86650 * b[3] + white * 0.3104856;
                b[4] = 0.55000 * b[4] + white * 0.5329522;
                b[5] = -0.7616 * b[5] - white * 0.0168980;
                let out = (b.iter().sum::<f32>() + white * 0.5362) * 0.11;
                b[6] = white * 0.115926;
                out.clamp(-1.0, 1.0)
            }
            Signal::Sine => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noise_is_bounded_independent_and_pink_has_more_low_frequency_energy() {
        let mut a = Generator::new(0);
        let mut b = Generator::new(1);
        let mut pink = Generator::new(0);
        let (mut white_energy, mut pink_energy, mut white_delta, mut pink_delta) =
            (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let (mut previous_white, mut previous_pink) = (0.0f32, 0.0f32);
        let mut difference = 0.0;
        for _ in 0..96000 {
            let w = a.next(Signal::White, 1000.0, 48000.0);
            let p = pink.next(Signal::Pink, 1000.0, 48000.0);
            assert!(w.is_finite() && w.abs() <= 1.0 && p.is_finite() && p.abs() <= 1.0);
            difference += (w - b.next(Signal::White, 1000.0, 48000.0)).abs();
            white_energy += f64::from(w * w);
            pink_energy += f64::from(p * p);
            white_delta += f64::from((w - previous_white).powi(2));
            pink_delta += f64::from((p - previous_pink).powi(2));
            previous_white = w;
            previous_pink = p;
        }
        assert!(difference > 1000.0 && white_energy > 1000.0 && pink_energy > 10.0);
        assert!(pink_delta / pink_energy < white_delta / white_energy * 0.5);
    }
}
