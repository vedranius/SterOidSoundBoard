//! The AI prompt: pseudonymised patient profile, measures and history, in
//! Croatian or English. Shared by the native app and the WebAssembly build.
use crate::clinic::{AnnotationSummary, PatientIn, Recording};
use std::fmt::Write;
use steroid_engine::analysis::{self, VoiceReport};

// ------------------------------------------------------------------ prompt

pub struct Context<'a> {
    pub patient: Option<&'a PatientIn>,
    pub age: Option<u32>,
    pub rec: &'a Recording,
    pub start: f32,
    pub end: f32,
    pub report: &'a VoiceReport,
    /// Earlier recordings of the same patient: (date, task, whole-file analysis).
    pub history: &'a [(String, String, VoiceReport)],
    pub question: &'a str,
    pub has_image: bool,
    /// Disfluency labels made by the clinician on this recording.
    pub annotations: Option<&'a AnnotationSummary>,
    /// dB SPL = Praat dB + offset, when the microphone is calibrated.
    pub calibration_db: Option<f32>,
    /// English prompt and answer (otherwise Croatian).
    pub en: bool,
}

pub const SYSTEM_PROMPT: &str = "\
Ti si iskusni stručnjak za kliničku fonetiku, logopediju i rehabilitaciju govora i glasa. \
Pomažeš rehabilitatoru (logopedu, fonetičaru, surdologu) koji radi s pacijentom u aplikaciji \
SterOidSoundBoard / DigiLingua.

Dobivaš: pseudonimizirani profil pacijenta (bez imena i identifikatora), opis trenutnog problema, \
vrstu govornog zadatka, opažanja rehabilitatora, akustičke mjere izračunate iz snimke algoritmima \
po uzoru na Praat/MDVP, sažetak F0 konture, usporedbu s prethodnim snimkama i ponekad sliku \
sonagrama s ucrtanom F0 konturom. Ne slušaš snimku — zaključuješ iz mjera i slike.

Napiši stručno mišljenje na hrvatskom jeziku za daljnju obradu rehabilitatora, sa sljedećim \
odjeljcima (markdown naslovi ##):
## Sažetak
## Interpretacija akustičkih parametara
(u odnosu na dob, spol i vrstu zadatka; navedi koje su mjere za ovaj zadatak pouzdane, a koje nisu — \
npr. jitter/shimmer/HNR vrijede za produženi vokal, a ne za tečni govor)
## Povezanost s trenutnim problemom i anamnezom
## Mogući obrasci i što dodatno provjeriti
(diferencijalna razmatranja kao hipoteze, ne dijagnoze)
## Prijedlozi za rehabilitaciju
(konkretne vježbe i ciljevi; gdje ima smisla i postavke aplikacije: DAF 0–5000 ms, FAF ±12 \
polutonova, maskirajući šum bijeli/ružičasti/smeđi/uskopojasni po uhu, diskontinuitet, 31-pojasni EQ)
## Preporuke za daljnju obradu
(npr. fonijatrijski/ORL pregled, laringoskopija, audiološka obrada — jasno istakni znakove \
upozorenja koji zahtijevaju liječnički pregled)
## Ograničenja ove analize

Pravila: ne postavljaj medicinsku dijagnozu i ne izmišljaj podatke kojih nema; kad nešto ne znaš, \
reci to. Razlikuj izmjereno od pretpostavljenog. Intenzitet je u dBFS (nije kalibriran na dB SPL). \
Budi sažet i praktičan — rehabilitator je stručnjak.";

pub const SYSTEM_PROMPT_EN: &str = "\
You are an experienced expert in clinical phonetics, speech-language pathology and voice and speech \
rehabilitation. You assist a clinician (speech-language pathologist, phonetician, audiologist) who works \
with a patient in the SterOidSoundBoard / DigiLingua application.

You receive: a pseudonymised patient profile (no name or identifiers), the current complaint, the type \
of speech task, the clinician's observations, acoustic measures computed from the recording with \
Praat-compatible algorithms, a summary of the F0 contour, a comparison with earlier recordings and \
sometimes an image of the sound editor (waveform, spectrogram, F0 and formant tracks). You do not hear \
the recording — you reason from the measures and the image.

Write a professional opinion in English for the clinician's further work, with these sections \
(markdown headings ##):
## Summary
## Interpretation of the acoustic parameters
(relative to age, sex and task; say which measures are reliable for this task and which are not — \
e.g. jitter/shimmer/HNR are valid for a sustained vowel, not for connected speech)
## Relation to the current complaint and history
## Possible patterns and what to check further
(differential considerations as hypotheses, not diagnoses)
## Rehabilitation suggestions
(concrete exercises and goals; where useful, application settings: DAF 0–5000 ms, FAF ±12 semitones, \
masking noise white/pink/brown/narrow-band per ear, discontinuity, 31-band EQ)
## Recommendations for further assessment
(e.g. phoniatric/ENT examination, laryngoscopy, audiological assessment — clearly flag warning signs \
that require a medical examination)
## Limitations of this analysis

Rules: do not make a medical diagnosis and do not invent data that is not given; say so when you do not \
know. Distinguish measured from assumed. Intensity is on Praat's scale unless stated as calibrated dB SPL. \
Be concise and practical — the clinician is an expert.";

fn opt(v: Option<f32>, d: usize, unit: &str) -> String {
    v.map(|x| format!("{x:.d$} {unit}").trim_end().to_string()).unwrap_or_else(|| "nije izmjereno".into())
}
fn opt_en(v: Option<f32>, d: usize, unit: &str) -> String {
    v.map(|x| format!("{x:.d$} {unit}").trim_end().to_string()).unwrap_or_else(|| "not measured".into())
}

fn measures_en(s: &mut String, r: &VoiceReport, cal: Option<f32>) {
    let st = &r.settings;
    let o = opt_en;
    let _ = writeln!(s, "- Praat algorithms (verified against Praat 7); F0 range {:.0}–{:.0} Hz, maximum formant {:.0} Hz", st.pitch_floor, st.pitch_ceiling, st.max_formant);
    let _ = writeln!(s, "- Duration: {:.2} s; voiced part: {:.0} %; fraction of locally unvoiced frames {:.1} %", r.duration, r.voiced_fraction * 100.0, r.unvoiced_fraction);
    let _ = writeln!(s, "- F0 mean: {}; median {}; SD {}; min {}; max {}; range (5th–95th pct.) {}", o(r.f0_mean, 1, "Hz"), o(r.f0_median, 1, "Hz"), o(r.f0_sd, 1, "Hz"), o(r.f0_min, 1, "Hz"), o(r.f0_max, 1, "Hz"), o(r.f0_range_st, 1, "semitones"));
    let _ = writeln!(s, "- Jitter local: {} (norm < {} %); abs: {}; RAP: {}; PPQ5: {}; DDP: {}", o(r.jitter_local, 3, "%"), analysis::JITTER_MAX, o(r.jitter_abs_us, 1, "µs"), o(r.jitter_rap, 3, "%"), o(r.jitter_ppq5, 3, "%"), o(r.jitter_ddp, 3, "%"));
    let _ = writeln!(s, "- Shimmer local: {} (norm < {} %); dB: {} (norm < {} dB); APQ3: {}; APQ5: {}; APQ11: {}; DDA: {}", o(r.shimmer_local, 3, "%"), analysis::SHIMMER_MAX, o(r.shimmer_db, 3, "dB"), analysis::SHIMMER_DB_MAX, o(r.shimmer_apq3, 3, "%"), o(r.shimmer_apq5, 3, "%"), o(r.shimmer_apq11, 3, "%"), o(r.shimmer_dda, 3, "%"));
    let _ = writeln!(s, "- HNR: {} (norm > {} dB); NHR: {}; mean autocorrelation: {}", o(r.hnr_db, 2, "dB"), analysis::HNR_MIN, o(r.nhr, 4, ""), o(r.mean_autocorrelation, 4, ""));
    let _ = writeln!(s, "- CPPS (AVQI protocol settings): {}", o(r.cpps, 2, "dB"));
    let _ = writeln!(s, "- Formants (median of voiced frames): F1 {}, F2 {}, F3 {}, F4 {}", o(r.formants[0], 0, "Hz"), o(r.formants[1], 0, "Hz"), o(r.formants[2], 0, "Hz"), o(r.formants[3], 0, "Hz"));
    let _ = writeln!(s, "- Glottal pulses: {}; periods: {}; mean period {}", r.pulses, r.periods, o(r.mean_period_ms, 3, "ms"));
    match cal {
        Some(c) => {
            let _ = writeln!(s, "- Intensity (calibrated, dB SPL): mean {}, range {}–{}", o(r.intensity_mean_db.map(|v| v + c), 1, "dB"), o(r.intensity_min_db.map(|v| v + c), 1, "dB"), o(r.intensity_max_db.map(|v| v + c), 1, "dB"));
        }
        None => {
            let _ = writeln!(s, "- Intensity (UNCALIBRATED, Praat scale): mean {}, SD {}", o(r.intensity_mean_db, 1, "dB"), o(r.intensity_sd_db, 1, "dB"));
        }
    }
    let _ = writeln!(s, "- Longest uninterrupted phonation: {:.2} s; voice breaks: {} (degree {:.1} %)", r.max_voiced_s, r.voice_breaks, r.voice_break_degree);
    let _ = writeln!(s, "- Pauses ≥ 250 ms: {} (mean {:.2} s; {:.1} % of the duration)", r.pauses, r.pause_mean_s, r.pause_ratio);
    let _ = writeln!(s, "- Syllable nuclei: {}; speech rate {:.2} syll/s; articulation rate {:.2} syll/s", r.syllable_nuclei, r.speech_rate, r.articulation_rate);
}

fn measures(s: &mut String, r: &VoiceReport, cal: Option<f32>) {
    let st = &r.settings;
    let _ = writeln!(s, "- Algoritmi Praata (validirano podudaranje s Praatom 7); raspon F0 {:.0}–{:.0} Hz, maks. formant {:.0} Hz", st.pitch_floor, st.pitch_ceiling, st.max_formant);
    let _ = writeln!(s, "- Trajanje: {:.2} s; zvučni dio: {:.0} %; udio lokalno bezvučnih okvira {:.1} %", r.duration, r.voiced_fraction * 100.0, r.unvoiced_fraction);
    let _ = writeln!(
        s,
        "- F0 srednja: {}; medijan {}; SD {}; min {}; max {}; raspon (5.–95. pct.) {}",
        opt(r.f0_mean, 1, "Hz"),
        opt(r.f0_median, 1, "Hz"),
        opt(r.f0_sd, 1, "Hz"),
        opt(r.f0_min, 1, "Hz"),
        opt(r.f0_max, 1, "Hz"),
        opt(r.f0_range_st, 1, "polutonova")
    );
    let _ = writeln!(
        s,
        "- Jitter local: {} (norma < {} %); abs: {}; RAP: {}; PPQ5: {}; DDP: {}",
        opt(r.jitter_local, 3, "%"),
        analysis::JITTER_MAX,
        opt(r.jitter_abs_us, 1, "µs"),
        opt(r.jitter_rap, 3, "%"),
        opt(r.jitter_ppq5, 3, "%"),
        opt(r.jitter_ddp, 3, "%")
    );
    let _ = writeln!(
        s,
        "- Shimmer local: {} (norma < {} %); dB: {} (norma < {} dB); APQ3: {}; APQ5: {}; APQ11: {}; DDA: {}",
        opt(r.shimmer_local, 3, "%"),
        analysis::SHIMMER_MAX,
        opt(r.shimmer_db, 3, "dB"),
        analysis::SHIMMER_DB_MAX,
        opt(r.shimmer_apq3, 3, "%"),
        opt(r.shimmer_apq5, 3, "%"),
        opt(r.shimmer_apq11, 3, "%"),
        opt(r.shimmer_dda, 3, "%")
    );
    let _ = writeln!(s, "- HNR: {} (norma > {} dB); NHR: {}; srednja autokorelacija: {}", opt(r.hnr_db, 2, "dB"), analysis::HNR_MIN, opt(r.nhr, 4, ""), opt(r.mean_autocorrelation, 4, ""));
    let _ = writeln!(s, "- CPPS (postavke AVQI protokola): {}", opt(r.cpps, 2, "dB"));
    let _ = writeln!(s, "- Formanti (medijan zvučnih okvira): F1 {}, F2 {}, F3 {}, F4 {}", opt(r.formants[0], 0, "Hz"), opt(r.formants[1], 0, "Hz"), opt(r.formants[2], 0, "Hz"), opt(r.formants[3], 0, "Hz"));
    let _ = writeln!(s, "- Glotalni pulsevi: {}; periode: {}; srednja perioda {}", r.pulses, r.periods, opt(r.mean_period_ms, 3, "ms"));
    match cal {
        Some(c) => {
            let _ = writeln!(
                s,
                "- Intenzitet (kalibrirano, dB SPL): srednji {}, raspon {}–{}",
                opt(r.intensity_mean_db.map(|v| v + c), 1, "dB"),
                opt(r.intensity_min_db.map(|v| v + c), 1, "dB"),
                opt(r.intensity_max_db.map(|v| v + c), 1, "dB")
            );
        }
        None => {
            let _ = writeln!(s, "- Intenzitet (NEKALIBRIRANO, Praat skala): srednji {}, SD {}", opt(r.intensity_mean_db, 1, "dB"), opt(r.intensity_sd_db, 1, "dB"));
        }
    }
    let _ = writeln!(s, "- Najduža neprekinuta fonacija: {:.2} s; prekidi zvučnosti: {} (stupanj {:.1} %)", r.max_voiced_s, r.voice_breaks, r.voice_break_degree);
    let _ = writeln!(s, "- Pauze ≥ 250 ms: {} (prosjek {:.2} s; {:.1} % trajanja)", r.pauses, r.pause_mean_s, r.pause_ratio);
    let _ = writeln!(
        s,
        "- Slogovne jezgre: {}; brzina govora {:.2} slog/s; brzina artikulacije {:.2} slog/s",
        r.syllable_nuclei, r.speech_rate, r.articulation_rate
    );
}

/// F0 over time in ten equal slices ("—" = no voicing in that slice).
fn contour(r: &VoiceReport) -> String {
    if r.pitch.is_empty() || r.duration <= 0.0 {
        return "nema podataka".into();
    }
    (0..10)
        .map(|i| {
            let (a, b) = (r.duration * i as f32 / 10.0, r.duration * (i + 1) as f32 / 10.0);
            let v: Vec<f32> = r.pitch.iter().filter(|p| p[0] >= a && p[0] < b && p[1] > 0.0).map(|p| p[1]).collect();
            if v.is_empty() { "—".to_string() } else { format!("{:.0}", v.iter().sum::<f32>() / v.len() as f32) }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// English version of the user message (same content and pseudonymisation).
fn build_prompt_en(c: &Context) -> (String, String) {
    let mut s = String::new();
    s.push_str("# Patient (pseudonymised)\n");
    match c.patient {
        Some(p) => {
            let f = |label: &str, v: &str, s: &mut String| {
                if !v.trim().is_empty() {
                    let _ = writeln!(s, "- {label}: {}", v.trim());
                }
            };
            let _ = writeln!(s, "- Age: {}", c.age.map(|a| format!("{a} years")).unwrap_or_else(|| "not given".into()));
            let sex = match p.sex.as_str() { "Ž" => "F", "M" => "M", "drugo" => "other", "" => "not given", x => x };
            let _ = writeln!(s, "- Sex: {sex}");
            f("Diagnosis", &p.diagnosis, &mut s);
            f("Current complaint / reason for referral", &p.complaint, &mut s);
            f("History of the problem", &p.history, &mut s);
            f("Medical history", &p.medical, &mut s);
            f("Medications", &p.medications, &mut s);
            f("Occupation and vocal load", &p.voice_use, &mut s);
            f("Smoking", &p.smoking, &mut s);
            f("Hearing", &p.hearing, &mut s);
            f("Native language", &p.language, &mut s);
            f("Therapy goals", &p.goals, &mut s);
            f("Clinician's notes on the patient", &p.notes, &mut s);
        }
        None => s.push_str("- The recording is not linked to a patient; profile unknown.\n"),
    }
    s.push_str("\n# Recording\n");
    let _ = writeln!(s, "- Task: {}", if c.rec.task.is_empty() { "not given" } else { &c.rec.task });
    let _ = writeln!(s, "- Label: {}", if c.rec.label.is_empty() { "—" } else { &c.rec.label });
    let _ = writeln!(
        s,
        "- Source: {}; sample rate {} Hz; analysed segment {:.2}–{:.2} s of {:.2} s in total",
        match c.rec.source.as_str() { "out" => "processed output (with effects)", "upload" => "uploaded file", _ => "dry microphone" },
        c.rec.sample_rate, c.start, c.end, c.rec.duration
    );
    if !c.rec.notes.trim().is_empty() {
        let _ = writeln!(s, "- Clinician's observations during the recording: {}", c.rec.notes.trim());
    }
    s.push_str("\n# Acoustic measures (segment)\n");
    measures_en(&mut s, c.report, c.calibration_db);
    let _ = writeln!(s, "- F0 contour by tenths of the segment (Hz): {}", contour(c.report).replace("nema podataka", "no data"));
    if let Some(a) = c.annotations {
        s.push_str("\n# Disfluency labels (clinician, whole recording)\n");
        let _ = writeln!(s, "- Total labels: {} ({:.1} per minute); stuttering-like disfluencies (SLD): {}", a.total, a.per_minute, a.sld);
        if let Some(p) = a.pct_ss {
            let src = if a.syllables_source == "rehabilitator" { "counted by the clinician" } else { "automatic estimate (syllable nuclei)" };
            let _ = writeln!(s, "- %SS: {:.1} % ({} syllables, {src})", p, a.syllables.unwrap_or(0));
        }
        if let Some(d) = a.longest3_mean_s {
            let _ = writeln!(s, "- Mean duration of the 3 longest SLD: {d:.2} s");
        }
        let kinds: Vec<String> = a.by_kind.iter().map(|(k, n)| format!("{}: {n}", crate::clinic::kind_label_en_of(k))).collect();
        if !kinds.is_empty() {
            let _ = writeln!(s, "- By kind: {}", kinds.join("; "));
        }
    }
    if !c.history.is_empty() {
        s.push_str("\n# Earlier recordings of the same patient (whole recordings, oldest first)\n");
        for (date, task, r) in c.history {
            let _ = writeln!(
                s,
                "- {date} · {}: F0 {} · jitter {} · shimmer {} · HNR {} · longest phonation {:.1} s · pauses {} · speech rate {:.2} syll/s",
                if task.is_empty() { "task not given" } else { task },
                opt_en(r.f0_mean, 0, "Hz"), opt_en(r.jitter_local, 2, "%"), opt_en(r.shimmer_local, 2, "%"), opt_en(r.hnr_db, 1, "dB"), r.max_voiced_s, r.pauses, r.speech_rate
            );
        }
    }
    if c.has_image {
        s.push_str("\nAttached image: the sound editor for this recording — waveform on top, spectrogram below (darker/brighter = more energy depending on the colour map), F0 in blue with its scale on the right, formants as red dots, disfluency labels in the bottom tier.\n");
    }
    s.push_str("\n# Clinician's request\n");
    if c.question.trim().is_empty() {
        s.push_str("Write a professional opinion using the given structure.\n");
    } else {
        let _ = writeln!(s, "{}\n(In addition, write a professional opinion using the given structure.)", c.question.trim());
    }
    (SYSTEM_PROMPT_EN.to_string(), s)
}

/// Returns (system prompt, user message). Identifying fields are never included.
pub fn build_prompt(c: &Context) -> (String, String) {
    if c.en {
        return build_prompt_en(c);
    }
    let mut s = String::new();
    s.push_str("# Pacijent (pseudonimizirano)\n");
    match c.patient {
        Some(p) => {
            let f = |label: &str, v: &str, s: &mut String| {
                if !v.trim().is_empty() {
                    let _ = writeln!(s, "- {label}: {}", v.trim());
                }
            };
            let _ = writeln!(s, "- Dob: {}", c.age.map(|a| format!("{a} god.")).unwrap_or_else(|| "nije navedena".into()));
            let _ = writeln!(s, "- Spol: {}", if p.sex.is_empty() { "nije naveden" } else { &p.sex });
            f("Dijagnoza", &p.diagnosis, &mut s);
            f("Trenutni problem / razlog dolaska", &p.complaint, &mut s);
            f("Anamneza problema", &p.history, &mut s);
            f("Medicinska povijest", &p.medical, &mut s);
            f("Lijekovi", &p.medications, &mut s);
            f("Zanimanje i vokalno opterećenje", &p.voice_use, &mut s);
            f("Pušenje", &p.smoking, &mut s);
            f("Sluh", &p.hearing, &mut s);
            f("Materinski jezik", &p.language, &mut s);
            f("Ciljevi terapije", &p.goals, &mut s);
            f("Bilješke rehabilitatora o pacijentu", &p.notes, &mut s);
        }
        None => s.push_str("- Snimka nije povezana s pacijentom; profil nije poznat.\n"),
    }
    s.push_str("\n# Snimka\n");
    let _ = writeln!(s, "- Zadatak: {}", if c.rec.task.is_empty() { "nije naveden" } else { &c.rec.task });
    let _ = writeln!(s, "- Oznaka: {}", if c.rec.label.is_empty() { "—" } else { &c.rec.label });
    let _ = writeln!(
        s,
        "- Izvor: {}; frekvencija uzorkovanja {} Hz; analizirani odsječak {:.2}–{:.2} s od ukupno {:.2} s",
        if c.rec.source == "out" { "obrađeni izlaz (s efektima)" } else { "suhi mikrofon" },
        c.rec.sample_rate,
        c.start,
        c.end,
        c.rec.duration
    );
    if !c.rec.notes.trim().is_empty() {
        let _ = writeln!(s, "- Opažanja rehabilitatora tijekom snimanja: {}", c.rec.notes.trim());
    }
    s.push_str("\n# Akustičke mjere (odsječak)\n");
    measures(&mut s, c.report, c.calibration_db);
    let _ = writeln!(s, "- F0 kontura po desetinama odsječka (Hz): {}", contour(c.report));
    if let Some(a) = c.annotations {
        s.push_str("\n# Oznake disfluencija (rehabilitator, cijela snimka)\n");
        let _ = writeln!(s, "- Ukupno oznaka: {} ({:.1} u minuti); disfluencije tipične za mucanje (SLD): {}", a.total, a.per_minute, a.sld);
        if let Some(p) = a.pct_ss {
            let _ = writeln!(s, "- %SS: {:.1} % ({} slogova, izvor: {})", p, a.syllables.unwrap_or(0), a.syllables_source);
        }
        if let Some(d) = a.longest3_mean_s {
            let _ = writeln!(s, "- Prosječno trajanje 3 najduže SLD: {d:.2} s");
        }
        let kinds: Vec<String> = a.by_kind.iter().map(|(k, n)| format!("{k}: {n}")).collect();
        if !kinds.is_empty() {
            let _ = writeln!(s, "- Po vrsti: {}", kinds.join("; "));
        }
    }
    if !c.history.is_empty() {
        s.push_str("\n# Prethodne snimke istog pacijenta (cijele snimke, od starije prema novijoj)\n");
        for (date, task, r) in c.history {
            let _ = writeln!(
                s,
                "- {date} · {}: F0 {} · jitter {} · shimmer {} · HNR {} · najduža fonacija {:.1} s · pauze {} · brzina govora {:.2} slog/s",
                if task.is_empty() { "zadatak nije naveden" } else { task },
                opt(r.f0_mean, 0, "Hz"),
                opt(r.jitter_local, 2, "%"),
                opt(r.shimmer_local, 2, "%"),
                opt(r.hnr_db, 1, "dB"),
                r.max_voiced_s,
                r.pauses,
                r.speech_rate
            );
        }
    }
    if c.has_image {
        s.push_str("\nPriložena slika: editor snimke — gore valni oblik, ispod spektrogram (jačina energije prema paleti boja), F0 plavom bojom sa skalom desno, formanti crvenim točkama, oznake disfluencija u donjem redu.\n");
    }
    s.push_str("\n# Zahtjev rehabilitatora\n");
    if c.question.trim().is_empty() {
        s.push_str("Napiši stručno mišljenje prema zadanoj strukturi.\n");
    } else {
        let _ = writeln!(s, "{}\n(Uz to napiši stručno mišljenje prema zadanoj strukturi.)", c.question.trim());
    }
    (SYSTEM_PROMPT.to_string(), s)
}

// ------------------------------------------------------------------ providers

