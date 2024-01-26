//! # Tablets
//!
//! todo :3

use wayland_backend::client::ObjectId;

#[derive(Debug)]
pub struct Tablet {
    pub(crate) obj_id: ObjectId,
    pub name: String,
}
