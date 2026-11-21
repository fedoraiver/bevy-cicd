use bevy_ecs::{name::Name, world::World};

pub struct Sample {
    pub count: u32,
}

pub fn spawn_fixture(world: &mut World) {
    world.spawn(Name::new("Bevy rules fixture"));
}

#[cfg(feature = "extra")]
pub fn spawn_extra(world: &mut World) {
    world.spawn(Name::new("Extra fixture entity"));
}

#[cfg(feature = "violations")]
pub struct TupleField(pub u32);

#[cfg(feature = "violations")]
pub fn spawn_unnamed(world: &mut World) {
    world.spawn_empty();
}
