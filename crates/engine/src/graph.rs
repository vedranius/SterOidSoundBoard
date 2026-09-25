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
            b.nodes.push(NodeDesc { id: id.clone(), kind: *k, params: Default::default(), bypass: false, x: 0.0, y: 0.0 });
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
