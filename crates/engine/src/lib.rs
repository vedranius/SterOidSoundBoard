//! SterOidSoundBoard real-time audio engine.
pub mod audio;
pub mod graph;
pub mod nodes;

pub use audio::{list_devices, AudioConfig, AudioEngine, EngineStats, HostDevices, RunningInfo};
pub use graph::{Board, Connection, LiveNode, NodeDesc, Schedule, INPUT_ID, OUTPUT_ID};
pub use nodes::{AtomicF32, NodeInfo, NodeKind, ParamSpec};
