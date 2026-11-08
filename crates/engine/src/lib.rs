//! SterOidSoundBoard real-time audio engine.
pub mod analysis;
#[cfg(feature = "native")]
pub mod audio;
pub mod fft;
pub mod graph;
pub mod live;
pub mod nodes;
pub mod praat;
#[cfg(feature = "native")]
pub mod record;
pub mod templates;
pub mod wav;

#[cfg(feature = "native")]
pub use audio::{list_devices, AudioConfig, AudioEngine, EngineStats, HostDevices, RunningInfo};
pub use graph::{Board, Connection, LiveNode, NodeDesc, Schedule, INPUT_ID, OUTPUT_ID};
pub use nodes::{AtomicF32, NodeInfo, NodeKind, ParamSpec, ISO_FREQS};
