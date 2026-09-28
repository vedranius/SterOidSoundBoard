//! Built-in boards. DigiLingua (speech/voice rehabilitation) is not a separate
//! engine: it is an ordinary board whose nodes carry roles the clinical UI binds to.
use crate::graph::{Board, Connection, NodeDesc, INPUT_ID, OUTPUT_ID};
use crate::nodes::NodeKind;
use std::collections::BTreeMap;

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const ALL: &[Template] = &[
    Template {
        id: "digilingua",
        name: "DigiLingua — clinical chain",
        description: "Mic select → 31-band EQ → DAF → FAF → interrupter → masking noise → L/R ears",
    },
    Template { id: "empty", name: "Empty board", description: "Input straight to output" },
];

pub fn build(id: &str) -> Option<Board> {
    match id {
        "digilingua" => Some(digilingua()),
        "empty" => Some(Board::default()),
        _ => None,
    }
}

/// Roles used by the DigiLingua UI, in signal order.
pub const CLINIC_ROLES: &[&str] = &["mic", "eq", "daf", "faf", "interrupt", "noise", "ears"];

pub fn digilingua() -> Board {
    let chain: [(&str, NodeKind, bool, &[(&str, f32)]); 7] = [
        ("mic", NodeKind::Channels, false, &[("source", 1.0)]),
        ("eq", NodeKind::Eq31, false, &[]),
        ("daf", NodeKind::Daf, true, &[("time", 150.0), ("dry", 0.0), ("wet", 1.0)]),
        ("faf", NodeKind::PitchShift, true, &[("semi", -6.0)]),
        ("interrupt", NodeKind::Interrupter, true, &[]),
        ("noise", NodeKind::Noise, true, &[("level", -30.0)]),
        ("ears", NodeKind::Channels, false, &[]),
    ];
    let mut b = Board { name: "DigiLingua".into(), nodes: vec![], connections: vec![], next_id: 1 };
    let mut prev = INPUT_ID.to_string();
    for (i, (role, kind, bypass, over)) in chain.iter().enumerate() {
        let id = format!("n{}", i + 1);
        let mut params: BTreeMap<String, f32> = kind.params().iter().map(|p| (p.id.to_string(), p.default)).collect();
        for (k, v) in over.iter() {
            params.insert(k.to_string(), *v);
        }
        b.nodes.push(NodeDesc {
            id: id.clone(),
            kind: *kind,
            params,
            bypass: *bypass,
            x: 160.0 + (i % 4) as f32 * 240.0,
            y: 30.0 + (i / 4) as f32 * 300.0,
            role: Some(role.to_string()),
        });
        b.connections.push(Connection { from: prev, to: id.clone() });
        prev = id;
    }
    b.connections.push(Connection { from: prev, to: OUTPUT_ID.into() });
    b.next_id = chain.len() as u32 + 1;
    b
}

/// A ready-made DigiLingua configuration (whole clinical board).
pub struct ClinicalPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub board: Board,
}

/// 31 EQ gains (dB) → node params on the "eq" role.
fn set_eq(b: &mut Board, gains: &[f32; 31]) {
    let ids: Vec<&'static str> = NodeKind::Eq31.params().iter().take(31).map(|p| p.id).collect();
    if let Some(n) = b.nodes.iter_mut().find(|n| n.role.as_deref() == Some("eq")) {
        for (id, g) in ids.iter().zip(gains) {
            n.params.insert(id.to_string(), *g);
        }
    }
}

fn set_role(b: &mut Board, role: &str, on: bool, params: &[(&str, f32)]) {
    if let Some(n) = b.nodes.iter_mut().find(|n| n.role.as_deref() == Some(role)) {
        n.bypass = !on;
        for (k, v) in params {
            n.params.insert(k.to_string(), *v);
        }
    }
}

/// Factory presets: the four DigiLingua EQ curves (identical values to the
/// original DigiLingua app) and therapy starting points. The therapy presets
/// are starting points for the clinician, not prescriptions.
pub fn clinical_presets() -> Vec<ClinicalPreset> {
    let eq = |gains: [f32; 31]| {
        let mut b = digilingua();
        set_eq(&mut b, &gains);
        b
    };
    let with = |f: &dyn Fn(&mut Board)| {
        let mut b = digilingua();
        f(&mut b);
        b
    };
    vec![
        ClinicalPreset {
            id: "flat",
            name: "Ravni odziv (Flat Response)",
            description: "Sve frekvencije na 0 dB — neutralna polazna točka za testiranje.",
            category: "Tvornički (DigiLingua)",
            board: eq([0.0; 31]),
        },
        ClinicalPreset {
            id: "bass_boost",
            name: "Pojačani basovi (Bass Boost)",
            description: "Pojačanje niskih frekvencija, blago stišavanje visokih.",
            category: "Tvornički (DigiLingua)",
            board: eq([
                12.0, 12.0, 10.0, 10.0, 8.0, 8.0, 6.0, 6.0, 4.0, 2.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -2.0, -2.0, -2.0, -2.0, -2.0,
                -2.0, -4.0, -4.0, -4.0, -4.0, -4.0, -6.0, -6.0, -6.0,
            ]),
        },
        ClinicalPreset {
            id: "treble_enhance",
            name: "Pojačani visoki tonovi (Treble Enhance)",
            description: "Pojačanje visokih frekvencija za jasnoću i razumljivost.",
            category: "Tvornički (DigiLingua)",
            board: eq([
                -6.0, -6.0, -6.0, -4.0, -4.0, -4.0, -2.0, -2.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 2.0, 2.0, 2.0, 2.0,
                2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 12.0, 14.0, 14.0,
            ]),
        },
        ClinicalPreset {
            id: "speech_focus",
            name: "Fokus na govor (Speech Focus)",
            description: "Naglašeno govorno područje (160 Hz – 4 kHz), stišani krajevi spektra.",
            category: "Tvornički (DigiLingua)",
            board: eq([
                -8.0, -8.0, -6.0, -6.0, -4.0, -4.0, -2.0, -2.0, 0.0, 4.0, 6.0, 8.0, 8.0, 8.0, 8.0, 8.0, 8.0, 10.0, 10.0, 10.0, 10.0,
                10.0, 8.0, 6.0, 4.0, 2.0, 0.0, -2.0, -4.0, -6.0, -8.0,
            ]),
        },
        ClinicalPreset {
            id: "daf_75",
            name: "Tečnost — DAF 75 ms",
            description: "Kratka odgođena slušna povratna veza (bez suhog glasa). Polazna točka, prilagoditi pacijentu.",
            category: "Terapijski predložak",
            board: with(&|b| set_role(b, "daf", true, &[("time", 75.0), ("dry", 0.0), ("wet", 1.0)])),
        },
        ClinicalPreset {
            id: "daf_faf",
            name: "Tečnost — DAF 75 ms + FAF −½ oktave",
            description: "Kombinirana DAF i frekvencijski promijenjena povratna veza.",
            category: "Terapijski predložak",
            board: with(&|b| {
                set_role(b, "daf", true, &[("time", 75.0), ("dry", 0.0), ("wet", 1.0)]);
                set_role(b, "faf", true, &[("semi", -6.0), ("mix", 1.0)]);
            }),
        },
        ClinicalPreset {
            id: "daf_200",
            name: "Usporavanje govora — DAF 200 ms",
            description: "Dulja odgoda za usporavanje tempa govora.",
            category: "Terapijski predložak",
            board: with(&|b| set_role(b, "daf", true, &[("time", 200.0), ("dry", 0.0), ("wet", 1.0)])),
        },
        ClinicalPreset {
            id: "lombard",
            name: "Glasnoća — maskirajući šum (Lombardov efekt)",
            description: "Ružičasti šum u oba uha potiče glasniji govor; razinu povećavati postupno.",
            category: "Terapijski predložak",
            board: with(&|b| set_role(b, "noise", true, &[("color", 1.0), ("level", -30.0), ("route", 0.0)])),
        },
        ClinicalPreset {
            id: "interrupt",
            name: "Diskontinuitet 1 s / 200 ms",
            description: "Periodično prekidanje povratne veze.",
            category: "Terapijski predložak",
            board: with(&|b| set_role(b, "interrupt", true, &[("period", 1000.0), ("gap", 200.0), ("depth", 1.0)])),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LiveNode, Schedule};

    #[test]
    fn clinical_presets_are_valid() {
        let ps = clinical_presets();
        assert_eq!(ps.len(), 9);
        for p in ps {
            let mut b = p.board;
            b.sanitize().unwrap();
            assert!(CLINIC_ROLES.iter().all(|r| b.by_role(r).is_some()), "{}", p.id);
        }
        let bb = clinical_presets().into_iter().find(|p| p.id == "bass_boost").unwrap().board;
        assert_eq!(bb.by_role("eq").unwrap().params["b20"], 12.0);
        assert_eq!(bb.by_role("eq").unwrap().params["b20k"], -6.0);
    }

    #[test]
    fn digilingua_template_is_valid_and_runs() {
        let mut b = digilingua();
        b.sanitize().unwrap();
        for r in CLINIC_ROLES {
            assert!(b.by_role(r).is_some(), "role {r}");
        }
        let live = b.nodes.iter().map(|n| (n.id.clone(), LiveNode::from_desc(n))).collect();
        let mut s = Schedule::build(&b, &live, 48000.0).unwrap();
        let mut out = crate::nodes::new_buf();
        s.input_mut()[0][..256].fill(0.25);
        s.run(256, &mut out);
        assert!(out[0][..256].iter().chain(&out[1][..256]).all(|v| v.is_finite()));
    }
}
