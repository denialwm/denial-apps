//! Asking deniald, through `denialctl ui`, to switch compositions. deniald
//! checks compatibility, confirms the start, keeps the choice and recovers
//! the packaged shell; it is also the only record of what runs.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::builder::valid_bundle;
use crate::object;
use crate::rebuild::waits_for_rebuild_of;
use crate::store::{Store, validate_id};
use crate::util::{self, Map, Result, canonical, display, fail, is_word, obj, text};

/// deniald loads compositions that carry no engine of their own.
pub const BUNDLE_CAPABILITY: &str = "pluginBundleV2";

const LOG_IN_AGAIN: &str =
    "Log out and back into Denial once to finish the update. Your plugin choices are saved.";

pub struct Activation {
    pub store: Store,
    pub denialctl: PathBuf,
}

fn capabilities(native: &Map) -> Vec<String> {
    util::strings(util::list(native, "capabilities"))
}

impl Activation {
    fn request(&self, operation: &[&str]) -> Result<Map> {
        // denialctl waits for mode transitions unless told --no-wait.
        let mut arguments = vec!["--json", "ui"];
        arguments.extend(operation);
        let output = util::run_args(&display(&self.denialctl), &arguments, None)?;
        match serde_json::from_str::<Value>(&output) {
            Ok(Value::Object(map)) => Ok(map),
            _ => fail(format!("denialctl answered badly: {}", output.trim())),
        }
    }

    /// What deniald says, or `available: false` with why it did not answer.
    pub fn status(&self) -> Map {
        self.request(&["status"]).unwrap_or_else(|error| {
            object! {"available" => false, "error" => error.message}
        })
    }

    pub fn require_support(&self) -> Result<()> {
        let native = self.status();
        if native.get("available") == Some(&Value::Bool(false)) {
            return fail("Denial is not available. Apply plugins from your Denial session.");
        }
        // A compositor from before compositions stopped carrying an engine
        // would refuse what this manager builds.
        if !capabilities(&native)
            .iter()
            .any(|capability| capability == BUNDLE_CAPABILITY)
        {
            return fail(LOG_IN_AGAIN);
        }
        Ok(())
    }

    /// The candidate whose bundle `bundle` is in this manager's state.
    pub fn candidate_of(&self, bundle: Option<&Value>) -> Option<String> {
        let bundle = bundle?.as_str()?;
        let path = Path::new(bundle);
        if util::name_of(path) != "bundle" {
            return None;
        }
        let id = util::name_of(path.parent()?);
        (is_word(&id)
            && bundle == display(&self.store.root.join("candidates").join(&id).join("bundle")))
        .then_some(id)
    }

    /// The candidate deniald runs; None for the packaged shell and for a
    /// composition this manager did not build.
    pub fn running_candidate(&self, native: &Map) -> Option<String> {
        self.candidate_of(native.get("plugin_bundle"))
    }

    pub fn activate(&self, id: &str) -> Result<Map> {
        validate_id(id)?;
        let plan = self.store.read(&format!("candidates/{id}/plan.json"))?;
        let build = self.store.read(&format!("candidates/{id}/build.json"))?;
        if text(&build, "status") != Some("built") || text(&plan, "status") != Some("built") {
            return fail("Build the candidate before activation");
        }
        if let Some(base) = text(&plan, "rebuildOf") {
            // A rebuild may replace only the composition deniald still waits
            // for; restoring the packaged shell or applying another one
            // meanwhile is the user's newer choice.
            if !waits_for_rebuild_of(&self.store, &self.status(), base)? {
                return fail("Denial no longer waits for these plugins to be rebuilt");
            }
        }
        let required = util::strings(util::list(
            obj(plan.get("sourceIdentity").unwrap_or(&Value::Null)),
            "required_native_capabilities",
        ));
        if !required.is_empty() {
            let supported = capabilities(&self.status());
            if required
                .iter()
                .any(|capability| !supported.contains(capability))
            {
                return fail(LOG_IN_AGAIN);
            }
        }
        let bundle = display(&canonical(
            &self.store.root.join("candidates").join(id).join("bundle"),
        )?);
        // Applying an unchanged composition leaves the desktop alone:
        // deniald already runs the build this one was copied from.
        if build
            .get("reusedFrom")
            .is_some_and(|value| !value.is_null())
        {
            let status = self.status();
            if let Some(running) = self.running_candidate(&status) {
                let running_build = self
                    .store
                    .read(&format!("candidates/{running}/build.json"))?;
                if running_build.get("cacheKey") == build.get("cacheKey")
                    && status.get("plugin_healthy") == Some(&Value::Bool(true))
                    && text(&status, "active_mode") == Some("custom_optimized")
                    && valid_bundle(Path::new(text(&status, "plugin_bundle").unwrap_or("")))
                {
                    self.remember_installed(&plan)?;
                    return Ok(status);
                }
            }
        }
        let native = self.request(&["activate", &bundle])?;
        if text(&native, "plugin_bundle") != Some(bundle.as_str())
            || native.get("plugin_healthy") != Some(&Value::Bool(true))
            || text(&native, "active_mode") != Some("custom_optimized")
        {
            return fail(match text(&native, "error").map(str::trim) {
                Some(error) if !error.is_empty() => format!("Your plugins did not start: {error}"),
                _ => "Your plugins did not start. Denial keeps the desktop it was running.".into(),
            });
        }
        self.remember_installed(&plan)?;
        Ok(native)
    }

    pub fn restore(&self) -> Result<Map> {
        let native = self.request(&["restore"])?;
        if text(&native, "active_mode") != Some("official_optimized") {
            return fail("The compositor did not restore the packaged shell");
        }
        Ok(native)
    }

    pub fn revert(&self) -> Result<Map> {
        let native = self.request(&["revert"])?;
        if native.get("plugin_healthy") != Some(&Value::Bool(true))
            || !native.get("plugin_bundle").is_some_and(Value::is_string)
        {
            return fail("The previous composition was not confirmed healthy");
        }
        // deniald may go back to plugins this manager has no record of: they
        // run, and the saved selection simply does not describe them.
        let Some(id) = self.running_candidate(&native) else {
            return Ok(native);
        };
        let plan = self.store.read(&format!("candidates/{id}/plan.json"))?;
        let mut selection = self.store.selection()?;
        // Going back restores the intent with the binary; a later Apply
        // keeps this composition's revisions, not those of the one left.
        selection.insert(
            "selections".into(),
            plan.get("selections")
                .cloned()
                .unwrap_or_else(|| Map::new().into()),
        );
        selection.insert(
            "ordering".into(),
            plan.get("ordering")
                .cloned()
                .unwrap_or_else(|| Map::new().into()),
        );
        self.store.write_selection(
            &selection,
            obj(plan.get("roots").unwrap_or(&Value::Null)).clone(),
        )?;
        self.store.write("last-plan.json", object! {"id" => id})?;
        self.remember_installed(&plan)?;
        Ok(native)
    }

    /// Turning an updated plugin on again keeps its last confirmed revision,
    /// even after another composition dropped it.
    fn remember_installed(&self, plan: &Map) -> Result<()> {
        let mut known = self.store.installed()?;
        for (name, root) in obj(plan.get("roots").unwrap_or(&Value::Null)) {
            known.insert(name.clone(), root.clone());
        }
        self.store
            .write("installed.json", object! {"plugins" => known})
    }
}
