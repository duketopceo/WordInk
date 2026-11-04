//! The Apple Neural Engine Parakeet engine (Linux on Apple silicon): an
//! optional, user-installed runtime that WordInk drives through one resident
//! worker process. Nothing here is bundled or linked; see `probe` for how
//! WordInk decides the engine can exist on this machine.

// The engine seam that consumes these lands with the model id and Labs toggle.
#[allow(dead_code)]
pub(crate) mod probe;
#[allow(dead_code)]
pub(crate) mod protocol;
#[allow(dead_code)]
pub(crate) mod worker;
