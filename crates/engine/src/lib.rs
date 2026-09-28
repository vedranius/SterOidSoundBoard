//! SterOidSoundBoard real-time audio engine.
pub mod analysis;
pub mod audio;
pub mod fft;
pub mod graph;
pub mod live;
pub mod nodes;
pub mod praat;
pub mod record;
pub mod templates;

pub use audio::{list_devices, AudioConfig, AudioEngine, EngineStats, HostDevices, RunningInfo};
pub use graph::{Board, Connection, LiveNode, NodeDesc, Schedule, INPUT_ID, OUTPUT_ID};
pub use nodes::{AtomicF32, NodeInfo, NodeKind, ParamSpec, ISO_FREQS};
