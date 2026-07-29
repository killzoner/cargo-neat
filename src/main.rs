#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use anyhow::anyhow;
use cargo::CargoResult;
use cargo::core::{EitherManifest, Features, SourceId, Workspace};
use cargo::util::context::GlobalContext;
use cargo::util::interning::InternedString;
use cargo::util::toml::read_manifest;
use cargo::util::toml_mut::dependency::Source;
use log::{debug, info, trace, warn};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::{env, vec};
use termtree::Tree;

const DEFAULT_PACKAGE_META: &str = "rust-version,edition,license,homepage,repository";

#[derive(argh::FromArgs)]
#[argh(description = r#"
cargo-neat: Keep your cargo workspace neat

Exit code:
    0:  when no issue is found
    1:  when at least one issue is found
    2:  on error
"#)]
struct CliArgs {
    /// print version
    #[argh(switch)]
    version: bool,

    /// require dependencies to be inherited (ie "workspace = true")
    #[argh(switch, short = 'm')]
    mandatory_workspace_dependencies: bool,

    /// path to directory that must be scanned
    #[argh(positional, greedy)]
    path: Option<PathBuf>,

    /// require declared package metadata keys to be inherited (ie "authors" for package.authors.workspace = true)
    #[argh(switch, short = 'p')]
    package_workspace_meta: bool,

    /// keys checked by --package-workspace-meta,
    /// resolved in order: cli, then workspace.metadata.cargo-neat.package-workspace-meta-values, then default (see README)
    #[argh(option)]
    package_workspace_meta_values: Option<String>,

    /// require opting out of default features (ie "default-features = false")
    #[argh(switch, short = 'f')]
    mandatory_no_default_features: bool,
}

// cargo install --path .
fn main() {
    let exit_code = match run() {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(err) => {
            eprintln!("Error: {err}");
            2
        }
    };

    std::process::exit(exit_code);
}

fn run() -> CargoResult<bool> {
    pretty_env_logger::init();

    let args: CliArgs =
        if std::env::var("CARGO").is_ok() && std::env::var("CARGO_PKG_NAME").is_err() {
            argh::cargo_from_env()
        } else {
            argh::from_env()
        };

    if args.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let args_path = match &args.path {
        Some(dir) => {
            debug!("Running from location {:?}", dir);

            std::fs::canonicalize(dir)? // we need absolute path
        }
        None => {
            let dir = env::current_dir()?;
            debug!("Running from location {:?}", dir);

            dir
        }
    };

    let args_package_workspace_meta: Option<Vec<&str>> = args
        .package_workspace_meta_values
        .as_deref()
        .map(|s| s.split(",").collect());

    let gctx = GlobalContext::default()?;
    // Load the workspace from the current directory
    let ws = Workspace::new(&args_path.join("Cargo.toml"), &gctx)?;

    // Get the root manifest path (root Cargo.toml)
    let root_cargo_toml = ws.root_manifest();

    debug!("Root workspace Cargo.toml: {:?}", root_cargo_toml);

    let mut workspace = cargo::core::Workspace::new(root_cargo_toml, &gctx)?;
    let workspace_usage = workspace
        .load_workspace_config()
        .map(|e| e.is_some())
        .unwrap_or_default();

    debug!("Workspace usage: {workspace_usage:?}",);

    let workspace = workspace;
    let workspace_members: Vec<_> = workspace.members().collect();
    let workspace_member_names: Vec<_> = workspace_members.iter().map(|e| e.name()).collect();
    debug!("Workspace members : {:?}", workspace_member_names);

    let workspace_meta_values: Option<Vec<&str>> = workspace
        .custom_metadata()
        .and_then(|meta| meta.get("cargo-neat"))
        .and_then(|cfg| cfg.get("package-workspace-meta-values"))
        .and_then(|values| values.as_array())
        .map(|values| values.iter().filter_map(|v| v.as_str()).collect());

    trace!("Workspace config meta-values : {:?}", workspace_meta_values);

    // order: cli then [workspace.metadata.cargo-neat].package-workspace-meta-values then DEFAULT_PACKAGE_META
    let args_package_workspace_meta: Vec<&str> = args_package_workspace_meta
        .or(workspace_meta_values)
        .unwrap_or_else(|| DEFAULT_PACKAGE_META.split(",").collect());

    debug!("Workspace meta-values : {:?}", args_package_workspace_meta);

    // read virtual manifest
    let source_id = SourceId::for_manifest_path(root_cargo_toml)?;
    let manifest = read_manifest(root_cargo_toml, source_id, &gctx)?;

    let root_cargo_toml = InternedString::new(
        root_cargo_toml
            .to_str()
            .ok_or(anyhow!("cannot get root workspace"))?,
    );

    run_checks(
        &workspace,
        workspace_usage,
        &manifest,
        root_cargo_toml,
        &args,
        &args_package_workspace_meta,
    )
}

fn run_checks(
    workspace: &Workspace,
    workspace_usage: bool,
    manifest: &EitherManifest,
    root_cargo_toml: InternedString,
    args: &CliArgs,
    args_package_workspace_meta: &[&str],
) -> CargoResult<bool> {
    let document = match manifest {
        EitherManifest::Real(m) => m.document(),
        EitherManifest::Virtual(v) => v.document(),
    }
    .ok_or(anyhow!("cannot get manifest document"))?;

    let workspace_dependencies_toml = document
        .get_ref()
        .get("workspace")
        .and_then(|e| e.get_ref().get("dependencies"))
        .and_then(|e| e.get_ref().as_table());

    let workspace_dependencies = workspace_dependencies_toml
        .map(|e| {
            e.keys()
                .map(|e| e.clone().into_inner())
                .collect::<BTreeSet<_>>() // use BTreeSet to keep deterministic order for debug print
        })
        .unwrap_or_default();

    debug!("Workspace dependencies : {:?}", workspace_dependencies);

    let mut unused_workspace_dependencies = workspace_dependencies;
    let mut mandatory_workspace_dependencies_issues: HashMap<InternedString, Vec<String>> =
        HashMap::new();
    let mut mandatory_workspace_meta_issues: HashMap<InternedString, Vec<String>> = HashMap::new();
    let mut default_features_workspace_issues: HashMap<InternedString, Vec<String>> =
        HashMap::new();

    // check dependencies always opt out from default-features in root package
    if args.mandatory_no_default_features {
        // we need to check from root package "default-features" if there are "true" value
        // because a sub package cannot override this, it errors with
        // `default-features = false` cannot override workspace's `default-features`
        let workspace_default_features_true = workspace_dependencies_toml
            .into_iter()
            .flatten()
            .map(|(name, value)| {
                let default_features = value
                    .get_ref()
                    .get("default-features")
                    .and_then(|v| v.get_ref().as_bool());
                (name.get_ref(), default_features)
            })
            .filter(|(_package, default_features)| default_features.unwrap_or(true))
            .map(|e| e.0)
            .collect::<BTreeSet<_>>(); // use BTreeSet to keep deterministic order for debug print

        debug!(
            "Workspace opt in default features : {:?}",
            workspace_default_features_true
        );

        if !workspace_default_features_true.is_empty() {
            default_features_workspace_issues.insert(
                root_cargo_toml,
                workspace_default_features_true
                    .iter()
                    .map(|e| e.to_string())
                    .collect(),
            );
        }
    }

    if args.mandatory_workspace_dependencies && !workspace_usage {
        warn!(
            "Skipping \"mandatory_workspace_dependencies\" (-m) check because no [workspace] detected"
        );
    }

    if args.package_workspace_meta && !workspace_usage {
        warn!("Skipping \"package_workspace_meta\" (-p) check because no [workspace] detected");
    }

    for pkg in workspace.members() {
        let pkg_manifest_path = InternedString::new(
            pkg.manifest_path()
                .to_str()
                .ok_or(anyhow!("cannot get package manifest path"))?,
        );

        // check unused workspace dependencies
        for dep in pkg.dependencies() {
            let name = dep.package_name();
            let name: &str = name.as_ref();
            unused_workspace_dependencies.remove(name);
        }

        let local_manifest =
            cargo::util::toml_mut::manifest::LocalManifest::try_new(pkg.manifest_path())?;

        // check dependencies always inherited from workspace
        if args.mandatory_workspace_dependencies && workspace_usage {
            let deps_other: Vec<_> = local_manifest
                .get_dependencies(workspace, &Features::default())
                .flat_map(|dep| dep.2.map(|e| (dep.0, e.source)))
                .filter_map(|dep| dep.1.map(|e| (dep.0, e)))
                .collect();

            for (dep, source) in deps_other {
                if let Source::Registry(_) = source {
                    let values = mandatory_workspace_dependencies_issues
                        .entry(pkg_manifest_path)
                        .or_insert(vec![]);
                    values.push(dep);
                }
            }
        }

        // check selected meta items are using workspace inheritance
        if args.package_workspace_meta && workspace_usage {
            let package_meta = local_manifest.manifest.data.get("package").unwrap();

            for key in args_package_workspace_meta {
                let key_exists = package_meta.get(key).is_some();
                let is_workspace_meta = package_meta
                    .get(key)
                    .and_then(|e| e.get("workspace").and_then(|e| e.as_bool()))
                    .unwrap_or_default();

                if key_exists && !is_workspace_meta {
                    let values = mandatory_workspace_meta_issues
                        .entry(pkg_manifest_path)
                        .or_insert(vec![]);
                    values.push(key.to_string());
                }
            }
        }

        // check dependencies always opt out from default-features
        if args.mandatory_no_default_features {
            let deps: Vec<_> = local_manifest
                .get_dependencies(workspace, &Features::default())
                .flat_map(|dep| dep.2.map(|e| (dep.0, e.default_features)))
                .filter_map(|dep| dep.1.map(|e| (dep.0, e)))
                .collect();

            for (dep, default_features) in deps {
                if default_features {
                    let values = default_features_workspace_issues
                        .entry(pkg_manifest_path)
                        .or_insert(vec![]);
                    values.push(dep);
                }
            }
        }
    }

    if unused_workspace_dependencies.is_empty()
        && mandatory_workspace_dependencies_issues.is_empty()
        && mandatory_workspace_meta_issues.is_empty()
        && default_features_workspace_issues.is_empty()
    {
        info!("No unused workspace dependencies");

        if args.mandatory_workspace_dependencies {
            info!("No non workspace dependencies");
        }

        if args.package_workspace_meta {
            info!("No non workspace metadata");
        }

        if args.mandatory_no_default_features {
            info!("No default-features enabled");
        }

        Ok(false)
    } else {
        if !unused_workspace_dependencies.is_empty() {
            let mut unused_workspace_dependencies: Vec<_> = unused_workspace_dependencies
                .into_iter()
                .map(|e| e.to_string())
                .collect();
            unused_workspace_dependencies.sort();

            eprintln!(
                "{}",
                tree(
                    InternedString::new("Unused workspace dependencies :"),
                    &[(root_cargo_toml, unused_workspace_dependencies)]
                )?
            );
        }

        if !mandatory_workspace_dependencies_issues.is_empty() {
            let mut mandatory_workspace_dependencies_issues: Vec<_> =
                mandatory_workspace_dependencies_issues
                    .into_iter()
                    .collect();
            mandatory_workspace_dependencies_issues.sort();

            eprintln!(
                "{}",
                tree(
                    InternedString::new("Non workspace dependencies :"),
                    &mandatory_workspace_dependencies_issues
                )?
            );
        }

        if !mandatory_workspace_meta_issues.is_empty() {
            let mut mandatory_workspace_meta_issues: Vec<_> =
                mandatory_workspace_meta_issues.into_iter().collect();
            mandatory_workspace_meta_issues.sort();

            eprintln!(
                "{}",
                tree(
                    InternedString::new("Non workspace metadata :"),
                    &mandatory_workspace_meta_issues
                )?
            );
        }

        if !default_features_workspace_issues.is_empty() {
            let mut default_features_workspace_issues: Vec<_> =
                default_features_workspace_issues.into_iter().collect();
            default_features_workspace_issues.sort();

            eprintln!(
                "{}",
                tree(
                    InternedString::new("Workspace default-features enabled :"),
                    &default_features_workspace_issues
                )?
            );
        }

        Ok(true)
    }
}

fn tree(
    root: InternedString,
    issues: &[(InternedString, Vec<String>)],
) -> anyhow::Result<Tree<InternedString>> {
    let mut tree: Tree<InternedString> = Tree::new(root);

    for (pkg, deps) in issues {
        let mut pkg: Tree<InternedString> = Tree::new(InternedString::new(pkg));

        for dep in deps {
            pkg.push(InternedString::new(dep.as_ref()));
        }

        tree.push(pkg);
    }

    Ok(tree)
}
