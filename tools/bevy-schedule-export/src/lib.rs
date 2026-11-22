//! 导出已完成插件装配的真实调度；不运行 Startup、帧循环或渲染线程。

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use analysis_contract::runtime::{
    RuntimeAccess, RuntimeCondition, RuntimeEdge, RuntimeSchedule, RuntimeSet, RuntimeSnapshot,
    RuntimeSystem,
};
use bevy_app::{App, PluginsState, SubApps};
use bevy_ecs::{
    query::ComponentAccessKind,
    schedule::{NodeId, Schedules, graph::DiGraph},
    system::System,
    world::World,
};
use bevy_extract::MainWorld;
use bevy_log::info;
use bevy_mod_debugdump::schedule_graph::{self, Settings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type DumpResult<T> = Result<T, Box<dyn Error>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    bevy_version: String,
    debugdump_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exporter: Option<&'static str>,
    exported_at: u64,
    target: &'static str,
    features: [&'static str; 1],
    source_fingerprint: String,
    schedules: Vec<ScheduleEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleEntry {
    id: String,
    app: String,
    label: String,
    filename: String,
    sha256: String,
    system_count: usize,
}

/// 等待插件就绪并完成 finish，随后导出主应用与全部子应用的调度。
/// 不调用 cleanup，避免流水线渲染插件将 RenderApp 移交给渲染线程。
///
/// source_root 指向接受分析的 Cargo 项目，供清单读取锁定版本与源码指纹。
pub fn dump_schedules(app: &mut App, output: &Path, source_root: &Path) -> DumpResult<usize> {
    dump_schedules_with_lock(app, output, source_root, None)
}

/// 从共享启动器导出调用方装配的 App。
pub mod launcher {
    use super::{App, DumpResult, Path, dump_schedules_with_lock};

    /// `source_root` 提供调用方的源码指纹，`launcher_lock` 提供实际编译的依赖版本。
    pub fn dump_schedules(
        app: &mut App,
        output: &Path,
        source_root: &Path,
        launcher_lock: &Path,
    ) -> DumpResult<usize> {
        dump_schedules_with_lock(app, output, source_root, Some(launcher_lock))
    }
}

fn dump_schedules_with_lock(
    app: &mut App,
    output: &Path,
    source_root: &Path,
    launcher_lock: Option<&Path>,
) -> DumpResult<usize> {
    while app.plugins_state() == PluginsState::Adding {
        bevy_tasks::tick_global_task_pools_on_main_thread();
        std::thread::yield_now();
    }
    app.finish();

    fs::create_dir_all(output)?;
    let manifest_path = output.join("manifest.json");
    // 完成全部导出后才发布清单，失败时不能沿用上次成功的清单。
    match fs::remove_file(&manifest_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let source_lock = source_root.join("Cargo.lock");
    let lockfile: CargoLock =
        toml::from_str(&fs::read_to_string(launcher_lock.unwrap_or(&source_lock))?)?;
    let bevy_version = lockfile.version("bevy")?;
    if launcher_lock.is_some() {
        let project: CargoLock = toml::from_str(&fs::read_to_string(source_lock)?)?;
        let expected = project.version("bevy")?;
        if bevy_version != expected {
            return Err(format!(
                "Launcher locked Bevy {bevy_version}, but the project locked Bevy {expected}"
            )
            .into());
        }
    }
    let debugdump_version = lockfile.version("bevy_mod_debugdump")?;
    let source_fingerprint = source_fingerprint(source_root)?;
    let mut snapshots = Vec::new();
    let mut entries = dump_world("main", app.world_mut(), output, &mut snapshots)?;
    let SubApps { main, sub_apps } = app.sub_apps_mut();
    let mut sub_apps = sub_apps.iter_mut().collect::<Vec<_>>();
    sub_apps.sort_by_key(|(label, _)| format!("{label:?}"));
    for (label, sub_app) in sub_apps {
        let app_name = format!("{label:?}");
        let mut context = MainWorldGuard::new(main.world_mut(), sub_app.world_mut(), &app_name)?;
        entries.extend(dump_world(
            &app_name,
            context.world_mut(),
            output,
            &mut snapshots,
        )?);
    }
    let count = entries.len();
    let manifest = Manifest {
        schema_version: 1,
        bevy_version,
        debugdump_version,
        exporter: launcher_lock.map(|_| "shared-launcher"),
        exported_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        target: env!("BEVY_SCHEDULE_EXPORT_TARGET"),
        features: ["debugdump"],
        source_fingerprint,
        schedules: entries,
    };
    let runtime = RuntimeSnapshot {
        schema_version: 1,
        source_fingerprint: manifest.source_fingerprint.clone(),
        target: manifest.target.to_owned(),
        features: manifest
            .features
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        schedules: snapshots,
    };
    // DOT、结构化注册和清单属于同一装配快照；最后发布清单才表示导出成功。
    fs::write(
        output.join("runtime.json"),
        serde_json::to_vec_pretty(&runtime)?,
    )?;
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    info!(target: "bevy_schedule_export", directory = %output.display(), schedules = count,
        "Schedule export completed");
    Ok(count)
}

/// Extract 参数初始化也需要真实主世界；守卫在返回错误和 panic 时都将其移回。
struct MainWorldGuard<'a> {
    main_world: &'a mut World,
    sub_world: &'a mut World,
}

impl<'a> MainWorldGuard<'a> {
    fn new(
        main_world: &'a mut World,
        sub_world: &'a mut World,
        app_name: &str,
    ) -> DumpResult<Self> {
        if sub_world.contains_resource::<MainWorld>() {
            return Err(format!(
                "Cannot export sub-app {app_name}: MainWorld resource is already present."
            )
            .into());
        }
        // 替换槽只保留独占所有权；Extract 始终访问原主世界及其原 WorldId。
        let mut resource = MainWorld::default();
        std::mem::swap(main_world, &mut *resource);
        sub_world.insert_resource(resource);
        Ok(Self {
            main_world,
            sub_world,
        })
    }

    fn world_mut(&mut self) -> &mut World {
        self.sub_world
    }
}

impl Drop for MainWorldGuard<'_> {
    fn drop(&mut self) {
        let mut resource = self
            .sub_world
            .remove_resource::<MainWorld>()
            .expect("MainWorld must remain available while exporting schedules");
        std::mem::swap(self.main_world, &mut *resource);
    }
}

fn dump_world(
    app_name: &str,
    world: &mut World,
    output: &Path,
    snapshots: &mut Vec<RuntimeSchedule>,
) -> DumpResult<Vec<ScheduleEntry>> {
    if !world.contains_resource::<Schedules>() {
        info!(target: "bevy_schedule_export", app = app_name, reason = "no_schedules_resource",
            "Sub-app has no schedules to export");
        return Ok(Vec::new());
    }
    world.resource_scope::<Schedules, _>(|world, mut schedules| {
        let ignored_ambiguities = schedules.ignored_scheduling_ambiguities.clone();
        let mut registered = schedules.iter_mut().collect::<Vec<_>>();
        registered.sort_by_key(|(label, _)| format!("{label:?}"));
        let mut entries = Vec::with_capacity(registered.len());
        for (label, schedule) in registered {
            let label_type = format!("{:?}", (label as &dyn std::any::Any).type_id());
            let label = format!("{label:?}");
            // 使用上游同样的图初始化路径，但显式传播构建失败，避免发布残缺图。
            schedule.graph_mut().initialize(world);
            schedule
                .graph_mut()
                .build_schedule(world, &ignored_ambiguities)
                .map_err(|error| {
                    format!("Failed to build schedule {app_name}/{label}: {error:?}")
                })?;
            let system_count = schedule.graph().systems.len();
            let dot = schedule_graph::schedule_graph_dot(schedule, world, &Settings::default());
            let id = schedule_id(app_name, &label);
            let filename = format!("schedule-{id}.dot");
            fs::write(output.join(&filename), dot.as_bytes())?;
            let graph = schedule.graph();
            let systems = graph
                .systems
                .iter()
                .map(|(key, system, conditions)| {
                    let node = NodeId::System(key);
                    let access = graph
                        .systems
                        .get(key)
                        .expect("Registered system must exist")
                        .access();
                    let access_set = access.to_filtered_access_set();
                    let accesses = access_set
                        .combined_access()
                        .try_iter_access()
                        .map(|items| {
                            items
                                .map(|item| {
                                    let (component, mode) = match item {
                                        ComponentAccessKind::Shared(component) => {
                                            (component, "read")
                                        }
                                        ComponentAccessKind::Exclusive(component) => {
                                            (component, "mutable_access")
                                        }
                                        ComponentAccessKind::Archetypal(component) => {
                                            (component, "filter")
                                        }
                                    };
                                    let info = world.components().get_info(component).expect(
                                        "System access must reference a registered component",
                                    );
                                    RuntimeAccess {
                                        type_id: info.type_id().map(|id| format!("{id:?}")),
                                        name: info.name().to_string(),
                                        mode: mode.to_owned(),
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    RuntimeSystem {
                        key: format!("{node:?}"),
                        graph_name: format!("node_{node:?}"),
                        name: system.name().to_string(),
                        type_id: format!("{:?}", system.system_type()),
                        conditions: conditions
                            .iter()
                            .map(|condition| RuntimeCondition {
                                name: condition.name().to_string(),
                                type_id: format!("{:?}", condition.system_type()),
                            })
                            .collect(),
                        accesses,
                        access_scope: if access.is_exclusive() {
                            "exclusive"
                        } else if access_set.combined_access().try_iter_access().is_err() {
                            "unbounded"
                        } else {
                            "finite"
                        }
                        .to_owned(),
                    }
                })
                .collect();
            let sets = graph
                .system_sets
                .iter()
                .map(|(key, set, conditions)| {
                    let node = NodeId::Set(key);
                    RuntimeSet {
                        key: format!("{node:?}"),
                        graph_name: format!("clusternode_{node:?}"),
                        label: format!("{set:?}"),
                        type_id: format!("{:?}", (set as &dyn std::any::Any).type_id()),
                        system_type: set.system_type().map(|id| format!("{id:?}")),
                        anonymous: set.is_anonymous(),
                        conditions: conditions
                            .iter()
                            .map(|condition| RuntimeCondition {
                                name: condition.name().to_string(),
                                type_id: format!("{:?}", condition.system_type()),
                            })
                            .collect(),
                    }
                })
                .collect();
            let edges = |graph: &DiGraph<NodeId>| {
                graph
                    .all_edges()
                    .map(|(from, to)| RuntimeEdge {
                        from: format!("{from:?}"),
                        to: format!("{to:?}"),
                    })
                    .collect()
            };
            snapshots.push(RuntimeSchedule {
                id: id.clone(),
                app: app_name.to_owned(),
                label: label.clone(),
                type_id: label_type,
                dot_filename: filename.clone(),
                dot_sha256: sha256(dot.as_bytes()),
                systems,
                sets,
                hierarchy: edges(graph.hierarchy().graph()),
                dependencies: edges(graph.dependency().graph()),
            });
            info!(target: "bevy_schedule_export", app = app_name, schedule = label,
                systems = system_count, filename, "Schedule exported");
            entries.push(ScheduleEntry {
                id,
                app: app_name.to_owned(),
                label,
                filename,
                sha256: sha256(dot.as_bytes()),
                system_count,
            });
        }
        Ok(entries)
    })
}

fn schedule_id(app: &str, label: &str) -> String {
    sha256(format!("{app}\0{label}").as_bytes())[..16].to_owned()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn source_fingerprint(root: &Path) -> DumpResult<String> {
    let mut paths = vec![PathBuf::from("Cargo.toml"), PathBuf::from("Cargo.lock")];
    if root.join("build.rs").exists() {
        paths.push(PathBuf::from("build.rs"));
    }
    collect_rust_files(root, Path::new("src"), &mut paths)?;
    let mut paths = paths
        .into_iter()
        .map(|path| {
            path.to_str()
                .map(|path| path.replace('\\', "/"))
                .ok_or_else(|| "Source path is not valid UTF-8.".into())
        })
        .collect::<DumpResult<Vec<String>>>()?;
    paths.sort();
    let mut hash = Sha256::new();
    for path in paths {
        let content = fs::read_to_string(root.join(&path))?.replace("\r\n", "\n");
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(content.as_bytes());
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn collect_rust_files(root: &Path, relative: &Path, paths: &mut Vec<PathBuf>) -> DumpResult<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(format!(
                "Source fingerprint does not support symlinks: {}",
                path.display()
            )
            .into());
        }
        if file_type.is_dir() {
            collect_rust_files(root, &path, paths)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            paths.push(path);
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct CargoLock {
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
}

impl CargoLock {
    fn version(&self, name: &str) -> DumpResult<String> {
        self.package
            .iter()
            .find(|package| package.name == name)
            .map(|package| package.version.clone())
            .ok_or_else(|| format!("Missing {name} in Cargo.lock").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_app::{AppLabel, Plugin, Startup, SubApp, Update};

    #[derive(AppLabel, Clone, Copy, Debug, Hash, PartialEq, Eq)]
    struct RenderApp;

    struct FinishRegistration;

    impl Plugin for FinishRegistration {
        fn build(&self, app: &mut App) {
            app.add_systems(Startup, never_run);
        }

        fn finish(&self, app: &mut App) {
            app.add_systems(Update, never_run);
        }

        fn cleanup(&self, _: &mut App) {
            panic!("Export must not call cleanup");
        }
    }

    fn never_run() {
        panic!("Export must not run any systems");
    }

    fn fixture_root() -> PathBuf {
        let run = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/schedule-export-fixtures")
            .join(format!("{}-{run}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = 'fixture'\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        root
    }

    const PROJECT_LOCK: &str = "version = 4\n[[package]]\nname = 'bevy'\nversion = '0.20.0'\n";
    const EXPORTER_LOCK_ENTRY: &str =
        "[[package]]\nname = 'bevy_mod_debugdump'\nversion = '0.17.0'\n";

    #[test]
    fn exports_registered_main_and_sub_app_schedules_without_running_them() {
        let root = fixture_root();
        fs::write(
            root.join("Cargo.lock"),
            format!("{PROJECT_LOCK}{EXPORTER_LOCK_ENTRY}"),
        )
        .unwrap();
        let mut app = App::new();
        app.add_plugins(FinishRegistration);
        let mut sub_app = SubApp::new();
        sub_app.add_systems(Update, never_run);
        app.insert_sub_app(RenderApp, sub_app);

        let output = root.join("output");
        let count = dump_schedules(&mut app, &output, &root).unwrap();
        assert_eq!(app.plugins_state(), PluginsState::Finished);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
        let runtime: RuntimeSnapshot =
            serde_json::from_slice(&fs::read(output.join("runtime.json")).unwrap()).unwrap();
        assert_eq!(manifest["bevyVersion"], "0.20.0");
        assert_eq!(manifest["debugdumpVersion"], "0.17.0");
        assert!(manifest.get("exporter").is_none());
        assert_eq!(manifest["sourceFingerprint"], runtime.source_fingerprint);
        assert_eq!(runtime.schedules.len(), count);
        for app in ["main", "RenderApp"] {
            let schedule = runtime
                .schedules
                .iter()
                .find(|schedule| schedule.app == app && schedule.label == "Update")
                .unwrap();
            assert_eq!(schedule.systems.len(), 1);
            assert!(schedule.systems[0].name.ends_with("never_run"));
            assert_eq!(
                schedule.dot_sha256,
                sha256(&fs::read(output.join(&schedule.dot_filename)).unwrap())
            );
        }
    }

    #[test]
    fn shared_launcher_uses_its_own_lock_and_original_source_fingerprint() {
        let root = fixture_root();
        let source_lock = root.join("Cargo.lock");
        fs::write(&source_lock, PROJECT_LOCK).unwrap();
        let launcher_lock = root.join("launcher.lock");
        fs::write(
            &launcher_lock,
            format!("{PROJECT_LOCK}{EXPORTER_LOCK_ENTRY}"),
        )
        .unwrap();
        let output = root.join("output");
        let mut app = App::new();
        app.add_systems(Update, never_run);
        assert!(launcher::dump_schedules(&mut app, &output, &root, &launcher_lock).unwrap() > 0);

        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
        let runtime: RuntimeSnapshot =
            serde_json::from_slice(&fs::read(output.join("runtime.json")).unwrap()).unwrap();
        let fingerprint = source_fingerprint(&root).unwrap();
        assert_eq!(manifest["exporter"], "shared-launcher");
        assert_eq!(manifest["bevyVersion"], "0.20.0");
        assert_eq!(manifest["debugdumpVersion"], "0.17.0");
        assert_eq!(manifest["sourceFingerprint"], fingerprint);
        assert_eq!(runtime.source_fingerprint, fingerprint);
        assert_eq!(fs::read_to_string(source_lock).unwrap(), PROJECT_LOCK);

        fs::write(
            &launcher_lock,
            format!("{PROJECT_LOCK}{EXPORTER_LOCK_ENTRY}").replace("0.20.0", "0.21.0"),
        )
        .unwrap();
        let error =
            launcher::dump_schedules(&mut App::new(), &output, &root, &launcher_lock).unwrap_err();
        assert!(error.to_string().contains("Launcher locked Bevy 0.21.0"));
        assert!(!output.join("manifest.json").exists());
    }
}
