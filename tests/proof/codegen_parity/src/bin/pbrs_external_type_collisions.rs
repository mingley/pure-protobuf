pub mod proof {
    pub mod collisions {
        include!(concat!(
            env!("OUT_DIR"),
            "/native-collisions/type_collisions.rs"
        ));
    }
    pub mod collision_use {
        include!(concat!(
            env!("OUT_DIR"),
            "/native-collisions-extern/type_collision_use.rs"
        ));
    }
}
include!("../type_collisions.rs");
