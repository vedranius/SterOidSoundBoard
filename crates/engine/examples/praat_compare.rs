//! Prints SterOidSoundBoard's Praat-compatible measures for a WAV file as
//! `key value` lines, for side-by-side validation against Praat itself
//! (see `tools/praat_compare.praat`).
use steroid_engine::praat::{cepstrum, formant, intensity, pitch, pulses, voice, Sound};

fn main() {
    let path = std::env::args().nth(1).expect("usage: praat_compare file.wav");
    let (x, sr) = steroid_engine::record::load_mono(std::path::Path::new(&path)).expect("read wav");
    let s = Sound::new(&x, sr as f32);
    let pp = pitch::PitchParams::default();
    let p = pitch::to_pitch(&s, &pp);
    let pl = pulses::to_pulses(&s, &p);
    let r = voice::voice_report(&s, &p, &pl, 0.0, s.duration(), &pp);
    let o = |v: Option<f64>| v.map(|x| format!("{x:.6}")).unwrap_or_else(|| "--undefined--".into());
    println!("median_pitch {}", o(r.median_pitch));
    println!("mean_pitch {}", o(r.mean_pitch));
    println!("sd_pitch {}", o(r.sd_pitch));
    println!("pulses {}", r.pulses);
    println!("periods {}", r.periods);
    println!("unvoiced_fraction {:.6}", r.unvoiced_fraction);
    println!("voice_breaks {}", r.voice_breaks);
    println!("jitter_local {}", o(r.jitter_local));
    println!("jitter_abs {}", r.jitter_local_abs.map(|x| format!("{x:.10}")).unwrap_or_else(|| "--undefined--".into()));
    println!("jitter_rap {}", o(r.jitter_rap));
    println!("jitter_ppq5 {}", o(r.jitter_ppq5));
    println!("shimmer_local {}", o(r.shimmer_local));
    println!("shimmer_db {}", o(r.shimmer_local_db));
    println!("shimmer_apq3 {}", o(r.shimmer_apq3));
    println!("shimmer_apq5 {}", o(r.shimmer_apq5));
    println!("shimmer_apq11 {}", o(r.shimmer_apq11));
    println!("hnr {}", o(r.mean_hnr));
    let it = intensity::to_intensity(&s, 100.0, 0.0);
    println!("intensity_mean {:.6}", it.db.iter().sum::<f64>() / it.db.len() as f64);
    let fr = formant::to_formants(&s, &formant::FormantParams::default());
    for k in 0..4 {
        let mut v: Vec<f64> = fr.iter().filter_map(|f| f.formants.get(k).map(|x| x.0)).collect();
        v.sort_by(f64::total_cmp);
        println!("F{} {}", k + 1, o(steroid_engine::praat::quantile(&v, 0.5)));
    }
    println!("cpps {}", o(cepstrum::cpps(&s, &cepstrum::CppsParams::default())));
}
