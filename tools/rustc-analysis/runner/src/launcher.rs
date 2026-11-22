use crate::host::{Options, digest, file_digest};
use analysis_contract::normalized;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use toml::Table;

const BEVY_REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
const LAUNCHER_SOURCE: &[u8] = include_bytes!("launcher_main.rs");

pub(crate) struct Launcher {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub metadata: Value,
    source_lock: PathBuf,
    pub source_lock_digest: String,
}

pub(crate) fn caller_package<'a>(metadata: &'a Value, root: &Path) -> Result<&'a Value> {
    let manifest = normalized(&root.join("Cargo.toml"))?;
    packages(metadata)?
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .and_then(|path| normalized(Path::new(path)).ok())
                .as_ref()
                == Some(&manifest)
        })
        .context("The selected root must contain a Cargo package")
}

pub(crate) fn uses_launcher(package: &Value) -> Result<bool> {
    let factory = &package["metadata"]["bevy-architecture"]["app-factory"];
    if factory.is_null() {
        return Ok(false);
    }
    ensure!(
        factory == "build_app",
        "[package.metadata.bevy-architecture] app-factory must be \"build_app\""
    );
    ensure!(
        package["targets"]
            .as_array()
            .is_some_and(|targets| targets.iter().any(|target| target["crate_types"]
                .as_array()
                .is_some_and(|types| types
                    .iter()
                    .any(|kind| matches!(kind.as_str(), Some("lib" | "rlib" | "dylib")))))),
        "An App factory requires a library target exposing pub fn build_app() -> bevy::app::App"
    );
    Ok(true)
}

impl Launcher {
    pub fn prepare(
        options: &Options,
        workspace: &Path,
        cache: &Path,
        baseline: &Value,
        mut cargo: Command,
    ) -> Result<Self> {
        let caller = caller_package(baseline, &options.root)?;
        let caller_id = string(caller, "id")?;
        let base_graph = dependency_graph(baseline, caller_id)?;
        let version = bevy_version(baseline, &base_graph)?;
        let caller_features = selected_features(options, caller)?;
        let mut generated: Table = r#"
[package]
name = "bevy-cicd-schedule-launcher"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]
resolver = "3"

[features]
default = ["debugdump"]
debugdump = ["bevy/debug"]

[dependencies]
"#
        .parse()?;
        let dependencies = generated["dependencies"].as_table_mut().unwrap();
        dependencies.insert(
            "caller".into(),
            Table::from_iter([
                ("package".into(), string(caller, "name")?.into()),
                ("path".into(), normalized(&options.root)?.into()),
                ("features".into(), caller_features.into()),
            ])
            .into(),
        );
        dependencies.insert(
            "bevy".into(),
            Table::from_iter([
                ("version".into(), format!("={version}").into()),
                ("default-features".into(), false.into()),
            ])
            .into(),
        );
        let exporter = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bevy-schedule-export");
        dependencies.insert(
            "bevy-schedule-export".into(),
            Table::from_iter([("path".into(), normalized(&exporter)?.into())]).into(),
        );
        forward_workspace_settings(&mut generated, workspace)?;

        let root = cache.join("launcher");
        fs::create_dir_all(root.join("src"))?;
        let root = root.canonicalize()?;
        ensure!(
            root.starts_with(cache),
            "Launcher escaped the analysis cache"
        );
        let manifest = root.join("Cargo.toml");
        let manifest_bytes = toml::to_string(&generated)?;
        write_if_changed(&manifest, manifest_bytes.as_bytes())?;
        write_if_changed(&root.join("src/main.rs"), LAUNCHER_SOURCE)?;

        let source_lock = workspace.join("Cargo.lock");
        let source_lock_bytes = fs::read(&source_lock)?;
        let source_lock_digest = digest(&source_lock_bytes);
        let launcher_lock = root.join("Cargo.lock");
        let seed_path = root.join("lock-seed");
        let seed = format!(
            "{}\n{}\n{}\n{}\n",
            source_lock_digest,
            digest(manifest_bytes),
            file_digest(&exporter.join("Cargo.toml"))?,
            file_digest(&exporter.join("Cargo.lock"))?
        );
        if fs::read_to_string(&seed_path).ok().as_deref() != Some(seed.as_str())
            || !launcher_lock.is_file()
        {
            write_if_changed(&launcher_lock, &source_lock_bytes)?;
        }
        let output = cargo
            .args([
                "metadata",
                "--offline",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(&manifest)
            .output()?;
        ensure!(
            output.status.success(),
            "Cannot resolve the dedicated launcher offline. Prepare architecture dependencies first.\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: Value = serde_json::from_slice(&output.stdout)?;
        let launcher_graph = dependency_graph(&metadata, caller_id)?;
        ensure!(
            base_graph.iter().all(|(package, edges)| launcher_graph
                .get(package)
                .is_some_and(|locked| edges.is_subset(locked))),
            "Launcher resolution changed the caller's locked dependency graph"
        );
        ensure!(
            bevy_version(&metadata, &launcher_graph)? == version,
            "Launcher resolution changed the caller's Bevy version"
        );

        let launcher = Self {
            root,
            manifest,
            metadata,
            source_lock,
            source_lock_digest,
        };
        launcher.verify_source_lock()?;
        write_if_changed(&seed_path, seed.as_bytes())?;
        Ok(launcher)
    }

    pub fn verify_source_lock(&self) -> Result<()> {
        ensure!(
            file_digest(&self.source_lock)? == self.source_lock_digest,
            "Caller Cargo.lock changed during architecture extraction"
        );
        Ok(())
    }
}

fn selected_features(options: &Options, caller: &Value) -> Result<Vec<String>> {
    let declared = caller["features"]
        .as_object()
        .context("Missing caller features")?;
    for name in &options.features {
        ensure!(
            declared.contains_key(name),
            "Unknown caller feature: {name}"
        );
    }
    if options.all_features {
        Ok(declared.keys().cloned().collect())
    } else {
        Ok(options
            .features
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
}

fn forward_workspace_settings(generated: &mut Table, workspace: &Path) -> Result<()> {
    let original: Table = fs::read_to_string(workspace.join("Cargo.toml"))?.parse()?;
    if let Some(profile) = original.get("profile") {
        generated.insert("profile".into(), profile.clone());
    }
    for key in ["patch", "replace"] {
        let Some(mut source_overrides) = original.get(key).and_then(toml::Value::as_table).cloned()
        else {
            continue;
        };
        for (_, entry) in &mut source_overrides {
            if key == "patch" {
                if let Some(registry) = entry.as_table_mut() {
                    for (_, dependency) in registry {
                        absolute_override_path(dependency, workspace)?;
                    }
                }
            } else {
                absolute_override_path(entry, workspace)?;
            }
        }
        generated.insert(key.into(), source_overrides.into());
    }
    Ok(())
}

fn absolute_override_path(dependency: &mut toml::Value, workspace: &Path) -> Result<()> {
    if let Some(table) = dependency.as_table_mut()
        && let Some(path) = table.get("path").and_then(toml::Value::as_str)
    {
        table.insert("path".into(), normalized(&workspace.join(path))?.into());
    }
    Ok(())
}

type DependencyGraph<'a> = BTreeMap<&'a str, BTreeSet<(&'a str, &'a str, &'a str, &'a str)>>;

/// Compare active non-dev dependency edges, allowing Cargo to add dependencies to enable Bevy's
/// debug features while holding the original caller's selected package IDs.
fn dependency_graph<'a>(metadata: &'a Value, caller_id: &'a str) -> Result<DependencyGraph<'a>> {
    let nodes: BTreeMap<_, _> = metadata["resolve"]["nodes"]
        .as_array()
        .context("Missing resolved Cargo nodes")?
        .iter()
        .map(|node| Ok((string(node, "id")?, node)))
        .collect::<Result<_>>()?;
    let mut pending = vec![caller_id];
    let mut graph = BTreeMap::new();
    while let Some(id) = pending.pop() {
        if graph.contains_key(id) {
            continue;
        }
        let mut edges = BTreeSet::new();
        let node = nodes
            .get(id)
            .with_context(|| format!("Missing resolved Cargo package: {id}"))?;
        for edge in node["deps"]
            .as_array()
            .context("Missing Cargo dependencies")?
        {
            let name = string(edge, "name")?;
            let dependency = string(edge, "pkg")?;
            for kind in edge["dep_kinds"]
                .as_array()
                .context("Missing Cargo dependency kinds")?
            {
                let category = kind["kind"].as_str().unwrap_or("normal");
                if category != "dev" {
                    edges.insert((
                        name,
                        category,
                        kind["target"].as_str().unwrap_or(""),
                        dependency,
                    ));
                    pending.push(dependency);
                }
            }
        }
        graph.insert(id, edges);
    }
    Ok(graph)
}

fn bevy_version<'a>(metadata: &'a Value, graph: &DependencyGraph<'_>) -> Result<&'a str> {
    let candidates = packages(metadata)?
        .iter()
        .filter(|package| {
            package["name"] == "bevy"
                && package["id"]
                    .as_str()
                    .is_some_and(|id| graph.contains_key(id))
        })
        .collect::<Vec<_>>();
    let [bevy] = candidates.as_slice() else {
        anyhow::bail!("An App factory requires one resolved crates.io Bevy 0.20 package");
    };
    let version = string(bevy, "version")?;
    ensure!(
        version.starts_with("0.20.") && bevy["source"] == BEVY_REGISTRY,
        "An App factory requires crates.io Bevy 0.20, found {version} ({})",
        bevy["source"]
    );
    Ok(version)
}

fn packages(metadata: &Value) -> Result<&Vec<Value>> {
    metadata["packages"]
        .as_array()
        .context("Missing Cargo packages")
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .with_context(|| format!("Missing Cargo metadata field: {key}"))
}

fn write_if_changed(filename: &Path, bytes: &[u8]) -> Result<()> {
    if fs::read(filename).is_ok_and(|previous| previous == bytes) {
        return Ok(());
    }
    fs::write(filename, bytes)?;
    Ok(())
}
