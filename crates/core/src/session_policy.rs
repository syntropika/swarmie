use super::*;

impl Engine {
    pub(crate) async fn refresh_execution_policy(&mut self) -> Result<()> {
        let backend = self.backend.clone();
        let report = self
            .controlled(
                Box::pin(async move { backend.execution_policy().await }),
                false,
            )
            .await?;
        let mut snapshot = self.snapshot.clone();
        snapshot.execution_policy.observe(report);
        self.commit_change(
            snapshot.clone(),
            EventKind::SessionUpdated(json!({"executionPolicy":snapshot.execution_policy})),
        )
        .await
    }
    pub(super) async fn describe_configuration(&mut self) -> Result<Vec<SettingDescriptor>> {
        self.refresh_execution_policy().await?;
        let mut configuration = if self.snapshot.capabilities.configure.is_available() {
            let backend = self.backend.clone();
            self.controlled(
                Box::pin(async move { backend.configuration().await }),
                false,
            )
            .await?
        } else {
            vec![]
        };
        configuration.push(self.snapshot.execution_policy.descriptor());
        Ok(configuration)
    }
    pub(super) async fn apply_configuration(
        &mut self,
        patch: Value,
    ) -> Result<ConfigurationReceipt> {
        self.snapshot.capabilities.configure.require("configure")?;
        if !self.permissions.is_empty() {
            return Err(Error::Busy);
        }
        let fields = patch
            .as_object()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        let requested = requested_execution_profile(&patch)?;
        let backend = self.backend.clone();
        let before = self
            .controlled(
                Box::pin(async move { backend.execution_policy().await }),
                false,
            )
            .await?;
        let mut native = fields.clone();
        native.remove(EXECUTION_PROFILE_SETTING);
        if let Some(profile) = requested {
            before.require(profile)?;
            let backend = self.backend.clone();
            let plan = self
                .controlled(
                    Box::pin(async move { backend.compile_execution_profile(profile).await }),
                    false,
                )
                .await?;
            if self.snapshot.turn.is_some() && plan.application != SettingApplication::CurrentTurn {
                return Err(Error::Busy);
            }
            let compiled = plan
                .native
                .as_object()
                .ok_or_else(|| Error::Provider("execution policy plan must be an object".into()))?;
            if compiled.keys().any(|key| native.contains_key(key)) {
                return Err(Error::Invalid(
                    "do not combine executionProfile with its native permission controls".into(),
                ));
            }
            native.extend(compiled.clone());
        }
        let backend = self.backend.clone();
        let descriptors = self
            .controlled(
                Box::pin(async move { backend.configuration().await }),
                false,
            )
            .await?;
        for (id, value) in &native {
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.id == *id)
                .ok_or_else(|| Error::Invalid(format!("unknown configuration {id}")))?;
            if descriptor.application == SettingApplication::CreationOnly {
                return Err(Error::Unsupported(format!("{id} requires a new session")));
            }
            if self.snapshot.turn.is_some()
                && descriptor.application != SettingApplication::CurrentTurn
            {
                return Err(Error::Busy);
            }
            if descriptor.kind != SettingType::Select
                || !native.contains_key("model")
                || id == "model"
            {
                descriptor.validate(value)?;
            }
        }
        let policy_changed = requested.is_some()
            || native.iter().any(|(key, value)| {
                before.controlled_settings.contains(key)
                    && (self.snapshot.execution_policy.requested.is_some()
                        || self.snapshot.execution_policy.dispatch_blocked
                        || before.planned.get(key) != Some(value))
            });
        if policy_changed {
            self.snapshot.execution_policy.revision += 1;
            self.snapshot.execution_policy.requested = requested;
            self.snapshot.execution_policy.status = PolicyStatus::Unknown;
            self.snapshot.execution_policy.dispatch_blocked = true;
            self.snapshot.execution_policy.reason =
                Some("Native policy application has not been confirmed".into());
            let mut planned = before.planned.as_object().cloned().unwrap_or_default();
            planned.extend(
                native
                    .iter()
                    .filter(|(key, _)| before.controlled_settings.contains(key))
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
            self.snapshot.execution_policy.planned = Value::Object(planned);
            self.snapshot.execution_policy.planned_profile = requested;
            if !self.snapshot.options.config.is_object() {
                self.snapshot.options.config = json!({});
            }
            if let Some(profile) = requested {
                self.snapshot.options.config[EXECUTION_PROFILE_SETTING] = json!(profile);
            } else {
                self.snapshot
                    .options
                    .config
                    .as_object_mut()
                    .unwrap()
                    .remove(EXECUTION_PROFILE_SETTING);
            }
            // Persist intent and uncertainty before any native write. A crash cannot erase the guard.
            let snapshot = self.snapshot.clone();
            if let Err(error)=self.commit_change(snapshot.clone(),EventKind::SessionUpdated(json!({"executionPolicy":snapshot.execution_policy,"options":snapshot.options}))).await {
                *self.shared.write().await=self.snapshot.clone();
                return Err(error);
            }
        }
        let backend = self.backend.clone();
        let submitted = Value::Object(native);
        let applied = self
            .controlled(
                Box::pin(async move { backend.configure(submitted).await }),
                false,
            )
            .await;
        let backend = self.backend.clone();
        let observed = self
            .controlled(
                Box::pin(async move { backend.configuration().await }),
                false,
            )
            .await;
        let backend = self.backend.clone();
        let report = self
            .controlled(
                Box::pin(async move { backend.execution_policy().await }),
                false,
            )
            .await;
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        if policy_changed {
            if let (Ok(_), Ok(_), Ok(report)) = (&applied, &observed, &report) {
                self.snapshot.execution_policy.dispatch_blocked = false;
                self.snapshot.execution_policy.observe(report.clone());
                if matches!(
                    self.snapshot.execution_policy.status,
                    PolicyStatus::Unknown | PolicyStatus::Partial | PolicyStatus::Rejected
                ) {
                    self.snapshot.execution_policy.dispatch_blocked = true;
                    self.snapshot.execution_policy.reason =
                        Some("Native policy application has not been confirmed".into());
                }
            } else {
                let changed = report
                    .as_ref()
                    .ok()
                    .and_then(|report| report.observed.as_ref())
                    .is_some_and(|observation| {
                        before
                            .observed
                            .as_ref()
                            .is_none_or(|previous| previous.native != observation.native)
                    });
                self.snapshot.execution_policy.status = if changed {
                    PolicyStatus::Partial
                } else {
                    PolicyStatus::Unknown
                };
                self.snapshot.execution_policy.reason=Some("Native policy may be partially applied; select a supported policy to resolve it before dispatch".into());
                if let Ok(report) = &report {
                    self.snapshot.execution_policy.observe(report.clone());
                }
            }
        } else if let Ok(report) = &report {
            self.snapshot.execution_policy.observe(report.clone());
        }
        let mut values = BTreeMap::new();
        if let Ok(Value::Object(receipt)) = &applied {
            for descriptor in &descriptors {
                if let Some(value) = receipt
                    .get(&descriptor.id)
                    .filter(|value| !value.is_object() && !value.is_array())
                {
                    values.insert(descriptor.id.clone(), value.clone());
                }
            }
        }
        if let Ok(configuration) = &observed {
            values.extend(configuration.iter().filter_map(|descriptor| {
                descriptor
                    .value
                    .clone()
                    .map(|value| (descriptor.id.clone(), value))
            }));
        }
        if let Some(profile) = self.snapshot.execution_policy.requested {
            values.insert(EXECUTION_PROFILE_SETTING.into(), json!(profile));
        }
        let mut retained = self
            .snapshot
            .options
            .config
            .as_object()
            .cloned()
            .unwrap_or_default();
        retained.extend(values.clone());
        self.snapshot.options.config = Value::Object(retained);
        if let Some(model) = values.get("model") {
            self.snapshot.options.model = model.as_str().map(String::from);
        }
        let mut configuration = observed.as_ref().ok().cloned().unwrap_or_default();
        configuration.push(self.snapshot.execution_policy.descriptor());
        let snapshot = self.snapshot.clone();
        if let Err(error)=self.commit_change(snapshot.clone(),EventKind::SessionUpdated(json!({"configuration":configuration,"options":snapshot.options,"executionPolicy":snapshot.execution_policy}))).await {
            if policy_changed { self.snapshot.execution_policy.dispatch_blocked=true; self.snapshot.execution_policy.status=PolicyStatus::Unknown; }
            *self.shared.write().await=self.snapshot.clone();
            return Err(error);
        }
        let raw = applied?;
        observed?;
        report?;
        Ok(ConfigurationReceipt {
            values,
            configuration,
            execution_policy: self.snapshot.execution_policy.clone(),
            raw: Some(raw),
        })
    }
}
