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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LiveNode, Schedule};

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
