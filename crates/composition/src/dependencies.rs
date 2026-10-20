use std::collections::{BTreeMap, BTreeSet};

use crate::{CompositionError, Diagnostic, EffectiveInstance, InstanceState};

type Instances = BTreeMap<String, EffectiveInstance>;

type Edges = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn resolve(instances: &mut Instances) -> Result<Vec<String>, CompositionError> {
    let mut edges = Edges::new();
    let mut diagnostics = BTreeMap::<String, Vec<Diagnostic>>::new();
    for (id, instance) in instances.iter() {
        if instance.state == InstanceState::Disabled {
            continue;
        }
        let dependencies = edges.entry(id.clone()).or_default();
        let issues = diagnostics.entry(id.clone()).or_default();
        if let InstanceState::Blocked { diagnostics } = &instance.state {
            issues.extend(diagnostics.clone());
        }
        for requirement in &instance.selection.requires {
            if requirement.optional {
                continue;
            }
            match instances.get(&requirement.instance) {
                None => issues.push(Diagnostic::MissingInstance {
                    instance: requirement.instance.clone(),
                }),
                Some(dependency) if dependency.state == InstanceState::Disabled => {
                    issues.push(Diagnostic::UnavailableInstance {
                        instance: requirement.instance.clone(),
                    });
                }
                Some(_) => {
                    dependencies.insert(requirement.instance.clone());
                }
            }
        }
        let Some(release) = &instance.release else {
            continue;
        };
        for requirement in &release.requires {
            if requirement.optional {
                continue;
            }
            let mut candidates = Vec::new();
            let mut available = BTreeSet::new();
            for (provider_id, provider) in instances.iter() {
                if provider.state == InstanceState::Disabled {
                    continue;
                }
                let Some(release) = &provider.release else {
                    continue;
                };
                for capability in &release.provides {
                    if capability.contract != requirement.contract {
                        continue;
                    }
                    available.insert(capability.version);
                    if capability.version == requirement.version {
                        candidates.push(provider_id.clone());
                    }
                }
            }
            match candidates.len() {
                0 => issues.push(if available.is_empty() {
                    Diagnostic::MissingCapability {
                        contract: requirement.contract.clone(),
                        version: requirement.version,
                    }
                } else {
                    Diagnostic::IncompatibleCapability {
                        contract: requirement.contract.clone(),
                        version: requirement.version,
                        available: available.into_iter().collect(),
                    }
                }),
                1 => {
                    let provider = candidates.remove(0);
                    // Intra-plugin object readiness belongs to Tembra. A package's own
                    // compatible capability does not create a deployment dependency.
                    if provider != *id {
                        dependencies.insert(provider);
                    }
                }
                _ => issues.push(Diagnostic::AmbiguousCapability {
                    contract: requirement.contract.clone(),
                    version: requirement.version,
                    providers: candidates,
                }),
            }
        }
    }
    let order = topological(&edges)?;
    for id in &order {
        let unavailable: Vec<_> = edges[id]
            .iter()
            .filter(|dependency| !diagnostics[*dependency].is_empty())
            .cloned()
            .collect();
        for instance in unavailable {
            let issue = Diagnostic::UnavailableInstance { instance };
            let issues = diagnostics
                .get_mut(id)
                .expect("every enabled instance has diagnostics");
            if !issues.contains(&issue) {
                issues.push(issue);
            }
        }
    }
    for (id, issues) in &diagnostics {
        if !issues.is_empty() {
            instances
                .get_mut(id)
                .expect("diagnostics originate from selected instances")
                .state = InstanceState::Blocked {
                diagnostics: issues.clone(),
            };
        }
    }
    Ok(order
        .into_iter()
        .filter(|id| instances[id].state == InstanceState::Ready)
        .collect())
}

fn topological(edges: &Edges) -> Result<Vec<String>, CompositionError> {
    fn visit(
        id: &str,
        edges: &Edges,
        visited: &mut BTreeSet<String>,
        path: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<(), CompositionError> {
        if let Some(index) = path.iter().position(|part| part == id) {
            let mut cycle = path[index..].to_vec();
            cycle.push(id.into());
            return Err(CompositionError::DependencyCycle(cycle));
        }
        if visited.contains(id) {
            return Ok(());
        }
        path.push(id.into());
        for dependency in &edges[id] {
            visit(dependency, edges, visited, path, order)?;
        }
        path.pop();
        visited.insert(id.into());
        order.push(id.into());
        Ok(())
    }
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for id in edges.keys() {
        visit(id, edges, &mut visited, &mut Vec::new(), &mut order)?;
    }
    Ok(order)
}
