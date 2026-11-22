//! Named layers controlling instance visibility.

/// Instances refer to a layer by its ID, not its position in the layer list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub id: u32,
    pub name: String,
    pub visible: bool,
}
