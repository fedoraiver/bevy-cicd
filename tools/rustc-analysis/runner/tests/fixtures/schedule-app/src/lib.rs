use bevy::prelude::*;

fn startup_should_not_run() {
    panic!("the launcher must not run Startup");
}

#[derive(Component)]
struct FixtureComponent;

fn registered_system(mut commands: Commands) {
    commands.spawn((Name::new("fixture entity"), FixtureComponent));
}

pub fn build_app() -> App {
    let mut app = App::new();
    app.add_systems(Startup, startup_should_not_run);
    app.add_systems(Update, registered_system);
    app
}
