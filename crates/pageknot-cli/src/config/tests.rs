use std::collections::BTreeMap;

use pageknot::{
    BrowserChannel, BrowserInstallationPolicy, CaptureProfile, CaptureRequest, CaptureScope,
    ColorScheme, ConfigProvenance, MAXIMUM_CAPTURE_NODES, Milliseconds, MissingResourcePolicy,
    NetworkPolicy, ReadinessMode, VerificationPolicy, Viewport,
};

use super::{ConfigFile, resolve_documents};

#[test]
fn environment_overrides_explicit_and_user_profile_values() {
    let user = toml::from_str::<ConfigFile>(
        r#"
        default_profile = "research"
        [profile.research.environment]
        locale = "de-AT"
        [profile.research.limits]
        duration = "3m"
        "#,
    );
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [profile.research.environment]
        locale = "fr-FR"
        [profile.research.limits]
        duration = "90s"
        "#,
    );
    let mut environment = BTreeMap::new();
    environment.insert("PAGEKNOT_LOCALE".to_owned(), "en-GB".to_owned());
    environment.insert("PAGEKNOT_TIMEOUT".to_owned(), "45s".to_owned());
    let resolved = user
        .ok()
        .zip(explicit.ok())
        .map(|(user, explicit)| resolve_documents(user, explicit, &environment, None));

    assert_eq!(
        resolved
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|resolved| resolved.profile.environment.locale.as_str()),
        Some("en-GB")
    );
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|resolved| resolved.profile.limits.duration.get()),
        Some(45_000)
    );
}

#[test]
fn environment_selects_network_idle_readiness() {
    let mut environment = BTreeMap::new();
    environment.insert("PAGEKNOT_WAIT_UNTIL".to_owned(), "network-idle".to_owned());

    let resolved = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &environment,
        None,
    );

    assert_eq!(
        resolved.map(|resolved| resolved.profile.readiness.mode),
        Ok(ReadinessMode::NetworkIdle)
    );
}

#[test]
fn environment_applies_the_typed_capture_profile_overlay() {
    let environment = BTreeMap::from([
        ("PAGEKNOT_VIEWPORT".to_owned(), "1280x720".to_owned()),
        ("PAGEKNOT_LOCALE".to_owned(), "de-AT".to_owned()),
        ("PAGEKNOT_TIMEZONE".to_owned(), "Europe/Vienna".to_owned()),
        ("PAGEKNOT_COLOR_SCHEME".to_owned(), "dark".to_owned()),
        ("PAGEKNOT_TIMEOUT".to_owned(), "45s".to_owned()),
        ("PAGEKNOT_WAIT_UNTIL".to_owned(), "network-idle".to_owned()),
        ("PAGEKNOT_DELAY".to_owned(), "750ms".to_owned()),
        ("PAGEKNOT_MISSING_RESOURCES".to_owned(), "fail".to_owned()),
        ("PAGEKNOT_SCOPE".to_owned(), "selection".to_owned()),
        ("PAGEKNOT_SELECTOR".to_owned(), "main article".to_owned()),
        ("PAGEKNOT_REMOVE_UNUSED_CSS".to_owned(), "true".to_owned()),
        (
            "PAGEKNOT_REMOVE_UNUSED_FONTS".to_owned(),
            "false".to_owned(),
        ),
        (
            "PAGEKNOT_REMOVE_HIDDEN_ELEMENTS".to_owned(),
            "true".to_owned(),
        ),
        ("PAGEKNOT_VERIFY".to_owned(), "static".to_owned()),
        ("PAGEKNOT_NETWORK_POLICY".to_owned(), "server".to_owned()),
    ]);
    let actual = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &environment,
        None,
    )
    .and_then(|resolved| {
        let mut request = CaptureRequest::builder("https://example.com")?.build()?;
        resolved.apply_to_request(&mut request);
        Ok(request)
    });
    let mut profile = CaptureProfile::default();
    profile.environment.viewport = Viewport {
        width: 1280,
        height: 720,
        scale: 1,
    };
    profile.environment.locale = "de-AT".to_owned();
    profile.environment.timezone = "Europe/Vienna".to_owned();
    profile.environment.color_scheme = ColorScheme::Dark;
    profile.limits.duration = Milliseconds::new(45_000);
    profile.readiness.mode = ReadinessMode::NetworkIdle;
    profile.readiness.delay = Milliseconds::new(750);
    profile.capture.missing_resources = MissingResourcePolicy::Fail;
    profile.capture.scope = CaptureScope::Page;
    profile.capture.selector = Some("main article".to_owned());
    profile.capture.optimizations.remove_unused_css = true;
    profile.capture.optimizations.remove_unused_fonts = false;
    profile.capture.optimizations.remove_hidden_elements = true;
    profile.verification = VerificationPolicy::Static;
    profile.network = NetworkPolicy::Server;
    let expected = CaptureRequest::builder("https://example.com")
        .and_then(|builder| builder.build())
        .map(|mut request| {
            profile.apply_to(&mut request);
            request
        });

    assert_eq!(actual, expected);
}

#[test]
fn profile_configures_network_idle_and_post_ready_delay() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [profile.research.readiness]
        mode = "network-idle"
        network_quiet = "750ms"
        delay = "1s"
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| {
            resolve_documents(
                ConfigFile::default(),
                explicit,
                &BTreeMap::new(),
                Some("research"),
            )
        })
        .and_then(Result::ok);

    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.profile.readiness.mode),
        Some(ReadinessMode::NetworkIdle)
    );
    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.profile.readiness.network_quiet.get()),
        Some(750)
    );
    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.profile.readiness.delay.get()),
        Some(1_000)
    );
}

#[test]
fn undefined_profile_returns_a_typed_configuration_error() {
    let result = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &BTreeMap::new(),
        Some("missing"),
    );

    assert_eq!(
        result.err().map(|error| error.code),
        Some(pageknot::ErrorCode::from_static("pageknot.config.profile"))
    );
}

#[test]
fn built_in_profiles_apply_the_canonical_capture_profile() {
    for name in ["default", "strict", "server"] {
        let actual = resolve_documents(
            ConfigFile::default(),
            ConfigFile::default(),
            &BTreeMap::new(),
            Some(name),
        )
        .and_then(|resolved| {
            let mut request = CaptureRequest::builder("https://example.com")?.build()?;
            resolved.apply_to_request(&mut request);
            Ok(request)
        });
        let expected = CaptureProfile::named(name).and_then(|profile| {
            let mut request = CaptureRequest::builder("https://example.com")?.build()?;
            profile.apply_to(&mut request);
            Ok(request)
        });

        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn built_in_profile_overrides_record_profile_provenance() {
    let resolved = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &BTreeMap::new(),
        Some("server"),
    )
    .ok();

    for (field, value) in [("missingResources", "fail"), ("networkPolicy", "server")] {
        assert_eq!(
            resolved
                .as_ref()
                .and_then(|resolved| resolved.configuration.get(field))
                .map(|entry| (entry.value.clone(), entry.provenance)),
            Some((
                serde_json::Value::String(value.to_owned()),
                ConfigProvenance::Profile,
            )),
            "{field}"
        );
    }
}

#[test]
fn default_configuration_provisions_a_managed_browser_on_demand() {
    let resolved = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &BTreeMap::new(),
        None,
    );

    assert_eq!(
        resolved
            .as_ref()
            .ok()
            .map(|config| config.browser_installation),
        Some(BrowserInstallationPolicy::InstallManaged)
    );
    assert_eq!(
        resolved
            .as_ref()
            .ok()
            .and_then(|config| config.configuration.get("browser.installation"))
            .map(|value| (value.value.clone(), value.provenance)),
        Some((
            serde_json::Value::String("install-managed".to_owned()),
            ConfigProvenance::Default,
        ))
    );
}

#[test]
fn system_browser_channel_keeps_provisioning_external() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        channel = "system"
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None))
        .and_then(Result::ok);

    assert_eq!(
        resolved.as_ref().map(|config| config.browser_channel),
        Some(BrowserChannel::System)
    );
}

#[test]
fn browser_management_policy_reaches_the_resolved_configuration() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        channel = "managed"
        installation = "install-managed"
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None))
        .and_then(Result::ok);

    assert_eq!(
        resolved.as_ref().map(|resolved| resolved.browser_channel),
        Some(BrowserChannel::Managed)
    );
    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.browser_installation),
        Some(BrowserInstallationPolicy::InstallManaged)
    );
}

#[test]
fn profile_resolves_selector_and_optimizer_policy() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [profile.research]
        selector = "main article"
        [profile.research.optimizations]
        remove_unused_css = true
        remove_unused_fonts = true
        remove_hidden_elements = true
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| {
            resolve_documents(
                ConfigFile::default(),
                explicit,
                &BTreeMap::new(),
                Some("research"),
            )
        })
        .and_then(Result::ok);

    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolved| resolved.profile.capture.selector.as_deref()),
        Some("main article")
    );
    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.profile.capture.scope),
        Some(CaptureScope::Page)
    );
    assert!(resolved.as_ref().is_some_and(|resolved| {
        resolved.profile.capture.optimizations.remove_unused_css
            && resolved.profile.capture.optimizations.remove_unused_fonts
            && resolved
                .profile
                .capture
                .optimizations
                .remove_hidden_elements
    }));
}

#[test]
fn selected_profile_and_environment_values_record_their_provenance() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [profile.research.environment]
        locale = "de-AT"
        [profile.research.readiness]
        delay = "2s"
        "#,
    );
    let mut environment = BTreeMap::new();
    environment.insert("PAGEKNOT_LOCALE".to_owned(), "en-GB".to_owned());
    environment.insert("PAGEKNOT_TIMEOUT".to_owned(), "45s".to_owned());
    let resolved = explicit
        .ok()
        .map(|explicit| {
            resolve_documents(
                ConfigFile::default(),
                explicit,
                &environment,
                Some("research"),
            )
        })
        .and_then(Result::ok);

    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolved| resolved.configuration.get("profile"))
            .map(|value| value.provenance),
        Some(ConfigProvenance::Flag)
    );
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolved| resolved.configuration.get("environment.locale"))
            .map(|value| value.provenance),
        Some(ConfigProvenance::Environment)
    );
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolved| resolved.configuration.get("limits.duration"))
            .map(|value| value.provenance),
        Some(ConfigProvenance::Environment)
    );
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolved| resolved.configuration.get("readiness.delay"))
            .map(|value| value.provenance),
        Some(ConfigProvenance::Profile)
    );
}

#[test]
fn browser_flag_replaces_the_configured_endpoint_in_diagnostics() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        cdp_url = "http://127.0.0.1:9222"
        "#,
    );
    let mut resolved = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None))
        .and_then(Result::ok);

    if let Some(resolved) = &mut resolved {
        resolved.apply_browser_path_flag("/opt/chromium".to_owned());
    }

    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolved| resolved.configuration.get("browser.path"))
            .map(|value| value.provenance),
        Some(ConfigProvenance::Flag)
    );
    assert!(resolved.as_ref().is_some_and(|resolved| {
        !resolved.configuration.contains_key("browser.cdpUrl")
            && !resolved.uses_remote_browser()
            && resolved.browser_path.as_deref() == Some("/opt/chromium")
    }));
}

#[test]
fn documented_profile_environment_shape_is_accepted() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [profile.research]
        verification = "offline"
        missing_resources = "fail"

        [profile.research.environment]
        viewport = { width = 1440, height = 900, scale = 1 }
        locale = "en-US"
        timezone = "UTC"
        color_scheme = "light"

        [profile.research.limits]
        duration = "2m"
        resource_bytes = "64MiB"
        total_resource_bytes = "512MiB"
        frames = 256
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| {
            resolve_documents(
                ConfigFile::default(),
                explicit,
                &BTreeMap::new(),
                Some("research"),
            )
        })
        .and_then(Result::ok);

    assert!(resolved.is_some());
}

#[test]
fn configured_node_limit_uses_the_public_capture_boundary() {
    let at_maximum = toml::from_str::<ConfigFile>(&format!(
        r#"
        [profile.default.limits]
        nodes = {MAXIMUM_CAPTURE_NODES}
        "#
    ));
    let above_maximum = toml::from_str::<ConfigFile>(&format!(
        r#"
        [profile.default.limits]
        nodes = {}
        "#,
        MAXIMUM_CAPTURE_NODES + 1
    ));

    let accepted = at_maximum
        .ok()
        .map(|config| resolve_documents(config, ConfigFile::default(), &BTreeMap::new(), None));
    let rejected = above_maximum
        .ok()
        .map(|config| resolve_documents(config, ConfigFile::default(), &BTreeMap::new(), None));

    assert_eq!(
        accepted
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|resolved| resolved.profile.limits.nodes),
        Some(MAXIMUM_CAPTURE_NODES)
    );
    assert_eq!(
        rejected
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .map(|error| error.code.as_str()),
        Some("pageknot.input.limit")
    );
    assert_eq!(
        rejected
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(MAXIMUM_CAPTURE_NODES))
    );
}
