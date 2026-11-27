//! Compiling a plan with the installation's Flutter tool and engine
//! compilers, into a fresh, sealed bundle. It never rebuilds Rust or the
//! engine and writes only into its own candidate. A bundle carries no
//! engine: deniald runs every composition on the session's own.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::object;
use crate::progress::CompilerProgress;
use crate::store::{DigestCache, Store, validate_id};
use crate::util::{
    self, Map, Result, canonical, content_key, display, fail, file_digest, is_word, obj,
    plain_tree_digest, read_json_map, text,
};

/// A cache hit reuses only output bytes that still match their manifest.
pub fn valid_bundle(bundle: &Path) -> bool {
    let check = || -> Result<bool> {
        let link =
            |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|stat| stat.is_symlink());
        if link(bundle) || bundle.parent().is_some_and(link) || contains_link(bundle) {
            return Ok(false);
        }
        let manifest = read_json_map(&bundle.join("denial-plugin-bundle.json"))?;
        let id = bundle.parent().map(util::name_of).unwrap_or_default();
        Ok(text(&manifest, "id") == Some(id.as_str())
            && text(&manifest, "app_sha256")
                == Some(file_digest(&bundle.join("lib/libapp.so"))?.as_str())
            && text(&manifest, "assets_sha256")
                == Some(plain_tree_digest(&bundle.join("data/flutter_assets"))?.as_str()))
    };
    check().unwrap_or(false)
}

fn contains_link(directory: &Path) -> bool {
    let Ok(listing) = std::fs::read_dir(directory) else {
        return false;
    };
    listing.flatten().any(|entry| match entry.file_type() {
        Ok(kind) if kind.is_symlink() => true,
        Ok(kind) if kind.is_dir() => contains_link(&entry.path()),
        _ => false,
    })
}

/// The bundle a build cache entry names, when it is still whole.
fn lookup(store: &Store, key: &str) -> Option<PathBuf> {
    let record = store.read(&format!("cache/builds/{key}.json")).ok()?;
    let id = text(&record, "id")?;
    validate_id(id).ok()?;
    let bundle = store.root.join("candidates").join(id).join("bundle");
    let build = store.read(&format!("candidates/{id}/build.json")).ok()?;
    (text(&build, "cacheKey") == Some(key)
        && text(&build, "bundle") == Some(display(&bundle).as_str())
        && valid_bundle(&bundle))
    .then_some(bundle)
}

/// The compiler inputs, named one by one: never hash a whole engine tree.
pub fn compiler_fingerprint(
    flutter: &Path,
    engine_root: &Path,
    target: &str,
    digests: &mut DigestCache,
) -> Result<String> {
    let real = canonical(flutter)?;
    let sdk = real
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("/"))
        .to_path_buf();
    let output = engine_root.join("out").join(target);
    let mut inputs = Map::new();
    let mut include = |name: &str, path: &Path, digests: &mut DigestCache| -> Result<()> {
        let digest = if path.is_dir() {
            digests.tree(path, &[])?
        } else if path.is_file() {
            digests.digest(path)?
        } else {
            return fail(format!("Missing compiler cache input: {}", path.display()));
        };
        inputs.insert(name.to_owned(), digest.into());
        Ok(())
    };
    for name in [
        "gen_snapshot",
        "font-subset",
        "impellerc",
        "flutter_patched_sdk",
        "shader_lib",
        "gen/const_finder.dart.snapshot",
        "icudtl.dat",
    ] {
        include(&format!("engine/{name}"), &output.join(name), digests)?;
    }
    include("launcher", flutter, digests)?;
    let tool_snapshot = sdk.join("bin/cache/flutter_tools.snapshot");
    if tool_snapshot.is_file() {
        include("tool-snapshot", &tool_snapshot, digests)?;
    }
    include("dart", &sdk.join("bin/cache/dart-sdk/bin/dart"), digests)?;
    include(
        "dart-snapshots",
        &sdk.join("bin/cache/dart-sdk/bin/snapshots"),
        digests,
    )?;
    include("internal", &sdk.join("bin/internal"), digests)?;
    let configuration = sdk.join("packages/flutter_tools/.dart_tool/package_config.json");
    let packages = read_json_map(&configuration)?;
    let folder = configuration
        .parent()
        .unwrap_or(Path::new("/"))
        .to_path_buf();
    for package in util::list(&packages, "packages") {
        let uri = package["rootUri"].as_str().unwrap_or("");
        let root = match uri.strip_prefix("file://") {
            Some(path) => PathBuf::from(path),
            None => util::normalize(&folder.join(uri)),
        };
        // Some tool dependencies hold only data; their resources count too.
        let digest = digests.tree(&root, &[".git", ".dart_tool", "build"])?;
        inputs.insert(
            format!("tool/{}", package["name"].as_str().unwrap_or("")),
            digest.into(),
        );
    }
    digests.save()?;
    Ok(content_key(&Value::Object(inputs)))
}

pub struct BuildRequest<'a> {
    pub engine_root: &'a Path,
    pub engine_target: &'a str,
    pub platform: &'a str,
}

/// Builds candidate `id`, already planned. Planning and building run back to
/// back under the manager lock, so the plan is not checked again.
pub fn build(
    store: &Store,
    id: &str,
    request: &BuildRequest,
    progress: &dyn Fn(&str),
) -> Result<Map> {
    validate_id(id)?;
    if !["linux-x64", "linux-arm64"].contains(&request.platform) {
        return fail("Unsupported target platform");
    }
    if !is_word(request.engine_target) {
        return fail("Invalid engine target");
    }
    let candidate = store.root.join("candidates").join(id);
    let plan = store.read(&format!("candidates/{id}/plan.json"))?;
    if text(&plan, "status") != Some("planned") || candidate.join("bundle").exists() {
        return fail("This selection was already built; apply it again to build a new one");
    }
    progress("Preparing the release compiler");
    let flutter = PathBuf::from(text(&plan, "flutter").unwrap_or(""));
    let mut digests = DigestCache::new(store);
    // The engine these compilers belong to; deniald compares it with the
    // session's only to explain a start that failed.
    let engine = request
        .engine_root
        .join("out")
        .join(request.engine_target)
        .join("libflutter_engine.so");
    let built_for = if engine.exists() {
        Some(digests.digest(&engine)?)
    } else {
        None
    };
    let output = candidate.join("assembly");
    let app = candidate.join("app");
    let cache_key = match text(&plan, "compositionKey") {
        Some(composition) => Some(content_key(&Value::Object(object! {
            "format" => 2,
            "composition" => composition,
            "compiler" => compiler_fingerprint(&flutter, request.engine_root, request.engine_target, &mut digests)?,
            "target" => request.engine_target,
            "platform" => request.platform,
        }))),
        None => None,
    };
    let retained = cache_key.as_deref().and_then(|key| lookup(store, key));
    if retained.is_none() {
        progress("Compiling the generated release shell");
        let jobs = std::thread::available_parallelism().map_or(1, |count| count.get());
        let root = display(request.engine_root);
        let target = request.engine_target;
        let platform = request.platform;
        let arguments: Vec<String> = vec![
            "assemble".into(),
            "--verbose".into(),
            format!("--local-engine-src-path={root}"),
            format!("--local-engine={target}"),
            format!("--local-engine-host={target}"),
            "--suppress-analytics".into(),
            format!(
                "--resource-pool-size={}",
                if jobs > 2 { jobs - 2 } else { 1 }
            ),
            format!("--output={}", display(&output)),
            "-dTargetFile=lib/main.dart".into(),
            "-dBuildMode=release".into(),
            format!("-dTargetPlatform={platform}"),
            "-dDartObfuscation=false".into(),
            "-dTrackWidgetCreation=true".into(),
            "-dTreeShakeIcons=true".into(),
            format!("release_bundle_{platform}_assets"),
        ];
        let mut compiler = CompilerProgress::default();
        let mut observe = |line: &str| {
            if let Some(phase) = compiler.observe(line) {
                progress(phase);
            }
        };
        util::run(
            &display(&flutter),
            &arguments,
            Some(&app),
            Some(&mut observe),
        )?;
    } else {
        progress("Reusing a verified desktop bundle");
    }
    progress("Validating and sealing the new bundle");
    let bundle = candidate.join("bundle");
    std::fs::create_dir_all(bundle.join("lib"))?;
    std::fs::create_dir_all(bundle.join("data"))?;
    let from = retained.clone().unwrap_or_else(|| output.clone());
    std::fs::copy(from.join("lib/libapp.so"), bundle.join("lib/libapp.so"))?;
    let assets = match &retained {
        Some(retained) => retained.join("data/flutter_assets"),
        None => output.join("flutter_assets"),
    };
    util::run_args(
        "cp",
        &[
            "-a",
            "--",
            &display(&assets),
            &display(&bundle.join("data/flutter_assets")),
        ],
        None,
    )?;
    let mut manifest = object! {
        "schema" => 1,
        "flutter_generation" => plan.get("flutterGeneration").cloned().unwrap_or(Value::Null),
        "id" => id,
        "mode" => "release",
        "platform" => request.platform,
    };
    if let Some(engine) = built_for {
        manifest.insert("engine_sha256".into(), engine.into());
    }
    manifest.extend(object! {
        "app_sha256" => file_digest(&bundle.join("lib/libapp.so"))?,
        "assets_sha256" => plain_tree_digest(&bundle.join("data/flutter_assets"))?,
        "framework_revision" => obj(plan.get("flutterVersion").unwrap_or(&Value::Null)).get("frameworkRevision").cloned().unwrap_or(Value::Null),
        "source_identity" => plan.get("sourceIdentity").cloned().unwrap_or(Value::Null),
        "pub_lock_sha256" => file_digest(&app.join("pubspec.lock"))?,
    });
    {
        use std::io::Write;
        let path = bundle.join("denial-plugin-bundle.json");
        let mut file = std::fs::File::create(&path)?;
        file.write_all(util::pretty(&Value::Object(manifest.clone())).as_bytes())?;
        file.sync_all()?;
    }
    // deniald loads only a bundle nothing can write to any more.
    util::run_args("chmod", &["-R", "a-w", "--", &display(&bundle)], None)?;
    let mut record = object! {
        "id" => id,
        "status" => "built",
        "bundle" => display(&bundle),
        "manifest" => manifest.clone(),
    };
    if let Some(key) = &cache_key {
        record.insert("cacheKey".into(), key.clone().into());
    }
    if let Some(retained) = &retained {
        record.insert(
            "reusedFrom".into(),
            retained
                .parent()
                .map(util::name_of)
                .unwrap_or_default()
                .into(),
        );
    }
    store.write(&format!("candidates/{id}/build.json"), record)?;
    let mut built = plan.clone();
    built.insert("status".into(), "built".into());
    store.write(&format!("candidates/{id}/plan.json"), built)?;
    if let Some(key) = &cache_key {
        store.write(&format!("cache/builds/{key}.json"), object! {"id" => id})?;
    }
    Ok(object! {
        "id" => id,
        "bundle" => display(&bundle),
        "manifest" => manifest,
        "reused" => retained.is_some(),
    })
}
