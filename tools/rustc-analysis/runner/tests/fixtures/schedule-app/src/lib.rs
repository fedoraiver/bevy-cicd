use bevy::{app::PluginGroupBuilder, prelude::*};

fn startup_should_not_run() {
    panic!("the launcher must not run Startup");
}

#[derive(Component)]
struct FixtureComponent;

fn registered_system(mut commands: Commands) {
    commands.spawn((Name::new("fixture entity"), FixtureComponent));
}

struct FixturePlugins;

impl PluginGroup for FixturePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(DisabledPlugin)
            .add(ReenabledPlugin)
            .add(SeparatelyInstalledPlugin)
    }
}

struct DisabledPlugin;

impl Plugin for DisabledPlugin {
    fn build(&self, _app: &mut App) {
        panic!("the disabled plugin must not build");
    }
}

struct ReenabledPlugin;

impl Plugin for ReenabledPlugin {
    fn build(&self, _app: &mut App) {}
}

struct SeparatelyInstalledPlugin;

impl Plugin for SeparatelyInstalledPlugin {
    fn build(&self, _app: &mut App) {}
}

pub fn build_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        FixturePlugins
            .build()
            .disable::<DisabledPlugin>()
            .set(DisabledPlugin)
            .disable::<ReenabledPlugin>()
            .enable::<ReenabledPlugin>()
            .disable::<SeparatelyInstalledPlugin>(),
        SeparatelyInstalledPlugin,
    ));
    assert!(!app.is_plugin_added::<DisabledPlugin>());
    assert!(app.is_plugin_added::<ReenabledPlugin>());
    assert!(app.is_plugin_added::<SeparatelyInstalledPlugin>());
    app.add_systems(Startup, startup_should_not_run);
    app.add_systems(Update, registered_system);
    app
}
