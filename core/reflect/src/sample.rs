//! Sample types declared through the same public macro as downstream types.
//!
//! `VehicleWide` exists for the glue proof (check 1.6.7) and differs from
//! `Vehicle` by exactly the `armor_rating` field.

crate::reflect! {
    #[reflect(version = 1)]
    pub struct Wheel {
        #[reflect(label = "Radius", unit = "m", min = 0.1, max = 1.5)]
        pub radius_m: f64,
        #[reflect(label = "Width", unit = "m", min = 0.05, max = 0.6)]
        pub width_m: f64,
        #[reflect(label = "Steered")]
        pub steered: bool,
    }
}

crate::reflect! {
    pub enum DriveMode { Front, Rear, All }
}

crate::reflect! {
    #[reflect(version = 2)]
    pub struct Vehicle {
        #[reflect(label = "Name")]
        pub name: String,
        #[reflect(label = "Mass", unit = "kg", min = 500.0, max = 5000.0, renamed_from = "mass")]
        pub mass_kg: f64,
        #[reflect(label = "Position", unit = "m")]
        pub position: [f64; 3],
        #[reflect(label = "Wheels")]
        pub wheels: Vec<Wheel>,
        #[reflect(label = "Note")]
        pub note: Option<String>,
        #[reflect(label = "Drive", since = 2, default = DriveMode::Rear)]
        pub drive: DriveMode,
    }
}

crate::reflect! {
    #[reflect(version = 2)]
    pub struct VehicleWide {
        #[reflect(label = "Name")]
        pub name: String,
        #[reflect(label = "Mass", unit = "kg", min = 500.0, max = 5000.0, renamed_from = "mass")]
        pub mass_kg: f64,
        #[reflect(label = "Position", unit = "m")]
        pub position: [f64; 3],
        #[reflect(label = "Wheels")]
        pub wheels: Vec<Wheel>,
        #[reflect(label = "Note")]
        pub note: Option<String>,
        #[reflect(label = "Drive", since = 2, default = DriveMode::Rear)]
        pub drive: DriveMode,
        #[reflect(label = "Armor rating", min = 0.0, max = 10.0)]
        pub armor_rating: f64,
    }
}

/// Registers the built-in examples in their public enumeration order.
pub fn register_samples(
    registry: &mut crate::registry::Registry,
) -> Result<(), crate::registry::RegistryError> {
    registry.register::<Vehicle>()?;
    registry.register::<Wheel>()
}
