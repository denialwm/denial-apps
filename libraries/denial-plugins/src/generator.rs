//! The construction plan of a composition and the Dart it generates:
//! ordinary constructor calls and typed immutable lists, no reflection and
//! no runtime registry. Persisted choices name types by their library.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::discovery::{Cardinality, Discovery, Injection, TypeId};
use crate::util::{Result, fail};

pub struct Plan<'a> {
    discovery: &'a Discovery,
    /// For each contract, the contributions that provide it, in order.
    providers: BTreeMap<TypeId, Vec<usize>>,
    order: Vec<usize>,
}

pub struct Generated {
    pub source: String,
    /// The variable holding each contract's providers.
    pub contracts: BTreeMap<TypeId, String>,
}

fn names(discovery: &Discovery, indices: &[usize]) -> String {
    indices
        .iter()
        .map(|&index| discovery.contributions[index].type_id.key())
        .collect::<Vec<_>>()
        .join(", ")
}

impl<'a> Plan<'a> {
    pub fn resolve(
        discovery: &'a Discovery,
        selections: &BTreeMap<String, String>,
        ordering: &BTreeMap<String, Vec<String>>,
        required: &[TypeId],
    ) -> Result<Self> {
        let mut providers = BTreeMap::new();
        for contract in &discovery.contracts {
            let id = &contract.type_id;
            let mut candidates: Vec<usize> = discovery
                .contributions
                .iter()
                .enumerate()
                .filter(|(_, contribution)| contribution.contracts.contains(id))
                .map(|(index, _)| index)
                .collect();
            if let Some(choice) = selections.get(&id.key()) {
                if contract.cardinality == Cardinality::ZeroOrMore {
                    return fail(format!(
                        "{id}: use collection ordering, not exclusive selection"
                    ));
                }
                candidates.retain(|&index| &discovery.contributions[index].type_id.key() == choice);
                if candidates.len() != 1 {
                    return fail(format!("{id}: selected provider {choice} is unavailable"));
                }
            }
            if contract.cardinality != Cardinality::ZeroOrMore && candidates.len() > 1 {
                return fail(format!(
                    "{id}: conflicting providers {}; select one or disable a root",
                    names(discovery, &candidates)
                ));
            }
            if contract.cardinality == Cardinality::ExactlyOne && candidates.is_empty() {
                return fail(format!("{id}: exactly one provider is required"));
            }
            if let Some(order) = ordering.get(&id.key()) {
                let ids: BTreeSet<String> = candidates
                    .iter()
                    .map(|&index| discovery.contributions[index].type_id.key())
                    .collect();
                let listed: BTreeSet<String> = order.iter().cloned().collect();
                if order.len() != ids.len() || listed != ids {
                    return fail(format!(
                        "{id}: ordering must list every provider exactly once"
                    ));
                }
                candidates.sort_by_key(|&index| {
                    let key = discovery.contributions[index].type_id.key();
                    order.iter().position(|item| *item == key)
                });
            }
            providers.insert(id.clone(), candidates);
        }
        let known: BTreeSet<String> = discovery
            .contracts
            .iter()
            .map(|contract| contract.type_id.key())
            .collect();
        for key in selections.keys().chain(ordering.keys()) {
            if !known.contains(key) {
                return fail(format!(
                    "Selection references an unavailable contract {key}"
                ));
            }
        }
        for contract in required {
            if providers.get(contract).is_none_or(Vec::is_empty) {
                return fail(format!(
                    "{contract}: required by the application, but no provider is enabled"
                ));
            }
        }
        let mut built = BTreeSet::new();
        let mut visiting: Vec<TypeId> = Vec::new();
        let mut order = Vec::new();
        fn visit(
            index: usize,
            discovery: &Discovery,
            providers: &BTreeMap<TypeId, Vec<usize>>,
            built: &mut BTreeSet<TypeId>,
            visiting: &mut Vec<TypeId>,
            order: &mut Vec<usize>,
        ) -> Result<()> {
            let contribution = &discovery.contributions[index];
            if built.contains(&contribution.type_id) {
                return Ok(());
            }
            if visiting.contains(&contribution.type_id) {
                let cycle: Vec<String> = visiting
                    .iter()
                    .chain(std::iter::once(&contribution.type_id))
                    .map(TypeId::key)
                    .collect();
                return fail(format!(
                    "Constructor dependency cycle: {}",
                    cycle.join(" -> ")
                ));
            }
            visiting.push(contribution.type_id.clone());
            for argument in &contribution.arguments {
                let found = providers
                    .get(&argument.contract)
                    .cloned()
                    .unwrap_or_default();
                if !argument.many && (found.len() > 1 || (found.is_empty() && !argument.nullable)) {
                    let listed = if found.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", names(discovery, &found))
                    };
                    return fail(format!(
                        "{}.{} requires {}; found {} providers{listed}",
                        contribution.type_id,
                        argument.name,
                        argument.contract,
                        found.len()
                    ));
                }
                for dependency in found {
                    visit(dependency, discovery, providers, built, visiting, order)?;
                }
            }
            visiting.pop();
            built.insert(contribution.type_id.clone());
            order.push(index);
            Ok(())
        }
        let mut active: Vec<usize> = providers.values().flatten().copied().collect();
        active.sort_by_key(|&index| discovery.contributions[index].type_id.key());
        active.dedup();
        for index in active {
            visit(
                index,
                discovery,
                &providers,
                &mut built,
                &mut visiting,
                &mut order,
            )?;
        }
        Ok(Self {
            discovery,
            providers,
            order,
        })
    }

    /// Ordinary Dart declarations. Every provider lives as long as the
    /// composition, and each is built once.
    pub fn generate(&self) -> Generated {
        let contributions = &self.discovery.contributions;
        let libraries: Vec<String> = self
            .order
            .iter()
            .map(|&index| contributions[index].type_id.library.clone())
            .chain(self.providers.keys().map(|id| id.library.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let aliases: HashMap<&str, String> = libraries
            .iter()
            .enumerate()
            .map(|(index, library)| (library.as_str(), format!("p{index}")))
            .collect();
        let type_name = |id: &TypeId| format!("{}.{}", aliases[id.library.as_str()], id.name);
        let variables: HashMap<TypeId, String> = self
            .order
            .iter()
            .enumerate()
            .map(|(position, &index)| {
                (
                    contributions[index].type_id.clone(),
                    format!("provider{position}"),
                )
            })
            .collect();
        let argument = |injection: &Injection| {
            let values = self
                .providers
                .get(&injection.contract)
                .cloned()
                .unwrap_or_default();
            let expression = if injection.many {
                format!(
                    "<{}>[{}]",
                    type_name(&injection.contract),
                    values
                        .iter()
                        .map(|&index| variables[&contributions[index].type_id].clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else if values.is_empty() {
                "null".to_owned()
            } else {
                variables[&contributions[values[0]].type_id].clone()
            };
            if injection.named {
                format!("{}: {expression}", injection.name)
            } else {
                expression
            }
        };
        let mut out = String::from("// Generated by denial_plugin_manager. Do not edit.\n");
        for library in &libraries {
            out.push_str(&format!(
                "import {} as {};\n",
                serde_json::to_string(library).unwrap_or_default(),
                aliases[library.as_str()]
            ));
        }
        out.push('\n');
        for &index in &self.order {
            let contribution = &contributions[index];
            let name = type_name(&contribution.type_id);
            out.push_str(&format!(
                "final {name} {} = {name}({});\n",
                variables[&contribution.type_id],
                contribution
                    .arguments
                    .iter()
                    .map(argument)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let mut exports = BTreeMap::new();
        for (position, (contract, values)) in self.providers.iter().enumerate() {
            let variable = format!("contract{position}");
            let name = type_name(contract);
            out.push_str(&format!(
                "final List<{name}> {variable} = List<{name}>.unmodifiable([{}]);\n",
                values
                    .iter()
                    .map(|&index| variables[&contributions[index].type_id].clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            exports.insert(contract.clone(), variable);
        }
        Generated {
            source: out,
            contracts: exports,
        }
    }
}
