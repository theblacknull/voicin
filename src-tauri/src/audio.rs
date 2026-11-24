// Thu âm micro bằng cpal (WASAPI trên Windows), đổi về mono 16 kHz 16-bit để gửi Google STT.
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::SampleFormat;
use std::sync::mpsc::Sender;

pub const RATE: f64 = 16000.0;

/// Đổi tần số mẫu bằng cách lấy trung bình các mẫu đầu vào cho mỗi mẫu đầu ra
struct Resampler {
    step: f64,
    t: f64,
    sum: f32,
    n: u32,
}

impl Resampler {
    fn new(input_rate: f64) -> Self {
        Resampler { step: RATE / input_rate, t: 0.0, sum: 0.0, n: 0 }
    }

    /// `data` là các mẫu xen kẽ theo kênh, `ch` là số kênh
    fn feed(&mut self, data: &[f32], ch: usize, out: &mut Vec<i16>) {
        for frame in data.chunks(ch) {
            let mono = frame.iter().sum::<f32>() / frame.len() as f32;
            self.sum += mono;
            self.n += 1;
            self.t += self.step;
            if self.t >= 1.0 {
                self.t -= 1.0;
                let v = (self.sum / self.n as f32).clamp(-1.0, 1.0);
                self.sum = 0.0;
                self.n = 0;
                out.push((v * 32767.0) as i16);
            }
        }
    }
}

pub fn start(tx: Sender<Vec<i16>>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let dev = host.default_input_device().ok_or("Không tìm thấy micro")?;
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let rate = cfg.sample_rate().0 as f64;
    let ch = cfg.channels() as usize;
    let fmt = cfg.sample_format();
    let config: cpal::StreamConfig = cfg.into();
    let mut rs = Resampler::new(rate);
    let err_fn = |e| eprintln!("[Voicin] lỗi micro: {e}");

    let stream = match fmt {
        SampleFormat::F32 => dev.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut out = Vec::new();
                rs.feed(data, ch, &mut out);
                if !out.is_empty() {
                    let _ = tx.send(out);
                }
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => dev.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                let mut out = Vec::new();
                rs.feed(&f, ch, &mut out);
                if !out.is_empty() {
                    let _ = tx.send(out);
                }
            },
            err_fn,
            None,
        ),
        other => return Err(format!("Định dạng micro chưa hỗ trợ: {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    Ok(stream)
}
