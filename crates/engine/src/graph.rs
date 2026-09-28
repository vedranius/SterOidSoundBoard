//! Board (serializable pedalboard) and Schedule (compiled, RT-ready graph).
use crate::nodes::{new_buf, AtomicF32, Buf, NodeKind, Processor};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;

pub const INPUT_ID: &str = "in";
pub const OUTPUT_ID: &str = "out";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDesc {
    pub id: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
    #[serde(default)]
    pub bypass: bool,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    /// Optional function tag ("eq", "daf", …) so mode-specific UIs (e.g.
    /// DigiLingua) can find their controls on an ordinary board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub name: String,
    pub nodes: Vec<NodeDesc>,
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub next_id: u32,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            name: "Default".into(),
            nodes: vec![],
            connections: vec![Connection { from: INPUT_ID.into(), to: OUTPUT_ID.into() }],
            next_id: 1,
        }
    }
}

impl Board {
    pub fn node(&self, id: &str) -> Option<&NodeDesc> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: &str) -> Option<&mut NodeDesc> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn by_role(&self, role: &str) -> Option<&NodeDesc> {
        self.nodes.iter().find(|n| n.role.as_deref() == Some(role))
    }

    /// Make an externally supplied board (preset file, API) safe to run:
    /// unique ids, known endpoints, no duplicate cables, clamped params,
    /// consistent `next_id`. Fails only if the graph has a cycle.
    pub fn sanitize(&mut self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        self.nodes.retain(|n| n.id != INPUT_ID && n.id != OUTPUT_ID && seen.insert(n.id.clone()));
        for n in &mut self.nodes {
            let specs = n.kind.params();
            n.params.retain(|k, _| specs.iter().any(|s| s.id == k));
            for s in specs {
                let v = n.params.get(s.id).copied().unwrap_or(s.default);
                n.params.insert(s.id.to_string(), s.clamp(v));
            }
        }
        let mut conns: Vec<Connection> = vec![];
        for c in std::mem::take(&mut self.connections) {
            if self.exists(&c.from) && self.exists(&c.to) && c.from != OUTPUT_ID && c.to != INPUT_ID && c.from != c.to && !conns.contains(&c) {
                conns.push(c);
            }
        }
        self.connections = conns;
        let max = self.nodes.iter().filter_map(|n| n.id.strip_prefix('n')?.parse::<u32>().ok()).max().unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);
        self.topo_order().map(|_| ())
    }

    fn exists(&self, id: &str) -> bool {
        id == INPUT_ID || id == OUTPUT_ID || self.node(id).is_some()
    }

    /// Validate a new connection (endpoints, duplicates, cycles).
    pub fn can_connect(&self, c: &Connection) -> Result<(), String> {
        if !self.exists(&c.from) || !self.exists(&c.to) {
            return Err("unknown node".into());
        }
        if c.from == OUTPUT_ID || c.to == INPUT_ID || c.from == c.to {
            return Err("invalid direction".into());
        }
        if self.connections.contains(c) {
            return Err("already connected".into());
        }
        let mut test = self.clone();
        test.connections.push(c.clone());
        test.topo_order().map(|_| ())
    }

    /// Kahn topological sort of processing nodes. Err on cycle.
    pub fn topo_order(&self) -> Result<Vec<String>, String> {
        let mut indeg: HashMap<&str, usize> = self.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
        for c in &self.connections {
            if let Some(d) = indeg.get_mut(c.to.as_str()) {
                if c.from != INPUT_ID {
                    *d += 1;
                }
            }
        }
        let mut ready: Vec<&str> = self.nodes.iter().map(|n| n.id.as_str()).filter(|id| indeg[id] == 0).collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = ready.pop() {
            order.push(id.to_string());
            for c in self.connections.iter().filter(|c| c.from == id) {
                if let Some(d) = indeg.get_mut(c.to.as_str()) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(c.to.as_str());
                    }
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err("connection would create a feedback loop".into());
        }
        Ok(order)
    }
}

/// Control-side handles for a live node (shared with the audio thread).
pub struct LiveNode {
    pub kind: NodeKind,
    pub params: Arc<[AtomicF32]>,
    pub bypass: Arc<AtomicBool>,
    pub meter: Arc<AtomicF32>,
}

impl LiveNode {
    pub fn from_desc(d: &NodeDesc) -> Self {
        let params: Vec<AtomicF32> = d
            .kind
            .params()
            .iter()
            .map(|s| AtomicF32::new(s.clamp(*d.params.get(s.id).unwrap_or(&s.default))))
            .collect();
        LiveNode {
            kind: d.kind,
            params: Arc::from(params),
            bypass: Arc::new(AtomicBool::new(d.bypass)),
            meter: Arc::new(AtomicF32::new(0.0)),
        }
    }
}

struct Step {
    proc: Box<dyn Processor>,
    sources: Vec<usize>,
    params: Arc<[AtomicF32]>,
    bypass: Arc<AtomicBool>,
    meter: Arc<AtomicF32>,
}

/// Compiled graph. Built on control thread, executed on audio thread.
pub struct Schedule {
    steps: Vec<Step>,
    out_src: Vec<usize>,
    /// bufs[0] = device input, bufs[i+1] = output of step i
    bufs: Vec<Buf>,
    scratch: Buf,
}

impl Schedule {
    pub fn build(board: &Board, live: &HashMap<String, LiveNode>, sr: f32) -> Result<Self, String> {
        let order = board.topo_order()?;
        let index: HashMap<&str, usize> = order.iter().enumerate().map(|(i, id)| (id.as_str(), i + 1)).collect();
        let buf_of = |id: &str| -> Option<usize> { if id == INPUT_ID { Some(0) } else { index.get(id).copied() } };
        let mut steps = Vec::with_capacity(order.len());
        for id in &order {
            let ln = live.get(id).ok_or_else(|| format!("missing live node {id}"))?;
            let sources = board.connections.iter().filter(|c| &c.to == id).filter_map(|c| buf_of(&c.from)).collect();
            steps.push(Step {
                proc: ln.kind.create(sr),
                sources,
                params: ln.params.clone(),
                bypass: ln.bypass.clone(),
                meter: ln.meter.clone(),
            });
        }
        let out_src = board.connections.iter().filter(|c| c.to == OUTPUT_ID).filter_map(|c| buf_of(&c.from)).collect();
        let bufs = (0..=steps.len()).map(|_| new_buf()).collect();
        Ok(Schedule { steps, out_src, bufs, scratch: new_buf() })
    }

    pub fn input_mut(&mut self) -> &mut Buf {
        &mut self.bufs[0]
    }

    /// Real-time safe: no allocation, no locks.
    pub fn run(&mut self, n: usize, out: &mut Buf) {
        let Schedule { steps, out_src, bufs, scratch } = self;
        for (si, step) in steps.iter_mut().enumerate() {
            for ch in 0..2 {
                scratch[ch][..n].fill(0.0);
                for &src in &step.sources {
                    for (d, s) in scratch[ch][..n].iter_mut().zip(&bufs[src][ch][..n]) {
                        *d += *s;
                    }
                }
            }
            let mut ob = std::mem::take(&mut bufs[si + 1]);
            if step.bypass.load(Relaxed) {
                ob[0][..n].copy_from_slice(&scratch[0][..n]);
                ob[1][..n].copy_from_slice(&scratch[1][..n]);
            } else {
                step.proc.process(&step.params, scratch, &mut ob, n);
            }
            let mut pk = 0f32;
            for ch in 0..2 {
                for v in ob[ch][..n].iter_mut() {
                    if !v.is_finite() {
                        *v = 0.0; // never let NaN/inf propagate
                    }
                    pk = pk.max(v.abs());
                }
            }
            step.meter.max(pk);
            bufs[si + 1] = ob;
        }
        for ch in 0..2 {
            out[ch][..n].fill(0.0);
            for &src in out_src.iter() {
                for (d, s) in out[ch][..n].iter_mut().zip(&bufs[src][ch][..n]) {
                    *d += *s;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with(kinds: &[NodeKind]) -> (Board, HashMap<String, LiveNode>) {
        let mut b = Board { connections: vec![], ..Default::default() };
        let mut prev = INPUT_ID.to_string();
        for (i, k) in kinds.iter().enumerate() {
            let id = format!("n{i}");
            b.nodes.push(NodeDesc { id: id.clone(), kind: *k, params: Default::default(), bypass: false, x: 0.0, y: 0.0, role: None });
            b.connections.push(Connection { from: prev, to: id.clone() });
            prev = id;
        }
        b.connections.push(Connection { from: prev, to: OUTPUT_ID.into() });
        let live = b.nodes.iter().map(|n| (n.id.clone(), LiveNode::from_desc(n))).collect();
        (b, live)
    }

    #[test]
    fn gain_chain() {
        let (b, live) = board_with(&[NodeKind::Gain]);
        live["n0"].params[0].set(-6.0206); // x0.5
        let mut s = Schedule::build(&b, &live, 48000.0).unwrap();
        let mut out = crate::nodes::new_buf();
        for _ in 0..200 {
            s.input_mut()[0][..256].fill(1.0);
            s.input_mut()[1][..256].fill(1.0);
            s.run(256, &mut out);
        }
        assert!((out[0][255] - 0.5).abs() < 1e-3, "{}", out[0][255]);
    }

    #[test]
    fn sanitize_repairs_foreign_boards() {
        let (mut b, _) = board_with(&[NodeKind::Gain, NodeKind::Delay]);
        b.nodes.push(b.nodes[0].clone()); // duplicate id
        b.nodes[0].params.insert("gain".into(), 1e9);
        b.nodes[0].params.insert("bogus".into(), 1.0);
        b.connections.push(Connection { from: "ghost".into(), to: OUTPUT_ID.into() });
        b.connections.push(b.connections[0].clone());
        b.next_id = 0;
        b.sanitize().unwrap();
        assert_eq!(b.nodes.len(), 2);
        assert_eq!(b.connections.len(), 3);
        assert_eq!(b.nodes[0].params["gain"], 24.0);
        assert!(!b.nodes[0].params.contains_key("bogus"));
        assert_eq!(b.next_id, 2);
        b.connections.push(Connection { from: "n1".into(), to: "n0".into() });
        assert!(b.sanitize().is_err());
    }

    #[test]
    fn cycle_rejected() {
        let (b, _) = board_with(&[NodeKind::Gain, NodeKind::Delay]);
        assert!(b.can_connect(&Connection { from: "n1".into(), to: "n0".into() }).is_err());
        assert!(b.can_connect(&Connection { from: "n0".into(), to: OUTPUT_ID.into() }).is_ok());
    }

    #[test]
    fn nan_never_escapes_and_all_nodes_stable() {
        let (b, live) = board_with(NodeKind::ALL);
        let mut s = Schedule::build(&b, &live, 48000.0).unwrap();
        let mut out = crate::nodes::new_buf();
        s.input_mut()[0][0] = f32::NAN;
        for i in 0..400 {
            for ch in 0..2 {
                for k in 0..512 {
                    s.input_mut()[ch][k] = ((i * 512 + k) as f32 * 0.05).sin() * 0.8;
                }
            }
            s.run(512, &mut out);
            assert!(out[0][..512].iter().chain(&out[1][..512]).all(|v| v.is_finite() && v.abs() < 10.0));
        }
    }
}
