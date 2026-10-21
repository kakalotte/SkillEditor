//! Domain model: skills, resources, validation issues, naming and path safety.
//!
//! These types are pure (no filesystem or GUI access) so they can be unit
//! tested in isolation and reused by every service.

pub mod naming;
pub mod path_safety;
pub mod resource;
pub mod skill;
pub mod validation;
