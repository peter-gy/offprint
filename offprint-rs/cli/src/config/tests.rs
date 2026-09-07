use std::collections::BTreeMap;

use offprint::{
    BrowserInstallationPolicy, BrowserSourcePolicy, CaptureProfile, CaptureRequest, CaptureScope,
    ColorScheme, ConfigProvenance, LazyLoadPolicy, MAXIMUM_CAPTURE_NODES, Milliseconds,
    MissingResourcePolicy, NetworkPolicy, ReadinessMode, VerificationMode, Viewport,
};

use super::{BrowserSelection, ConfigFile, resolve_documents};

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
    environment.insert("OFFPRINT_LOCALE".to_owned(), "en-GB".to_owned());
    environment.insert("OFFPRINT_TIMEOUT".to_owned(), "45s".to_owned());
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
    environment.insert("OFFPRINT_WAIT_UNTIL".to_owned(), "network-idle".to_owned());

    let resolved = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &environment,
        None,
    );

    assert_eq!(
        resolved.map(|resolved| (
            resolved.profile.readiness.mode,
            resolved.profile.readiness.lazy_load,
        )),
        Ok((ReadinessMode::NetworkIdle, LazyLoadPolicy::Disabled))
    );
}

#[test]
fn one_config_source_rejects_two_browser_selections() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        path = "/opt/chromium"
        cdp_url = "http://127.0.0.1:9222"
        "#,
    );
    let result = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None));

    assert!(result.is_some_and(|result| {
        result.is_err_and(|error| error.code.as_str() == "offprint.config.value")
    }));
}

#[test]
fn invalid_remote_endpoint_does_not_echo_its_value() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        cdp_url = "secret endpoint"
        "#,
    );
    let result = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None));

    assert!(result.is_some_and(|result| {
        result.is_err_and(|error| {
            error.code.as_str() == "offprint.config.value"
                && !error.message.contains("secret endpoint")
        })
    }));
}

#[test]
fn environment_rejects_two_browser_selections() {
    let environment = BTreeMap::from([
        (
            "OFFPRINT_BROWSER_PATH".to_owned(),
            "/opt/chromium".to_owned(),
        ),
        (
            "OFFPRINT_CDP_URL".to_owned(),
            "http://127.0.0.1:9222".to_owned(),
        ),
    ]);

    let result = resolve_documents(
        ConfigFile::default(),
        ConfigFile::default(),
        &environment,
        None,
    );

    assert!(result.is_err_and(|error| error.code.as_str() == "offprint.config.value"));
}

#[test]
fn later_browser_source_replaces_earlier_selection() {
    let user = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        path = "/opt/chromium"
        "#,
    );
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        cdp_url = "http://127.0.0.1:9222"
        "#,
    );
    let resolved = user.ok().zip(explicit.ok()).and_then(|(user, explicit)| {
        resolve_documents(user, explicit, &BTreeMap::new(), None).ok()
    });

    assert!(matches!(
        resolved.as_ref().map(|resolved| &resolved.browser),
        Some(BrowserSelection::Remote(_))
    ));
    assert!(resolved.as_ref().is_some_and(|resolved| {
        !resolved.configuration.contains_key("browser.path")
            && resolved
                .configuration
                .get("browser.cdpUrl")
                .is_some_and(|value| value.provenance == ConfigProvenance::ExplicitConfig)
    }));
}

#[test]
fn environment_applies_the_typed_capture_profile_overlay() {
    let environment = BTreeMap::from([
        ("OFFPRINT_VIEWPORT".to_owned(), "1280x720".to_owned()),
        ("OFFPRINT_LOCALE".to_owned(), "de-AT".to_owned()),
        ("OFFPRINT_TIMEZONE".to_owned(), "Europe/Vienna".to_owned()),
        ("OFFPRINT_COLOR_SCHEME".to_owned(), "dark".to_owned()),
        ("OFFPRINT_TIMEOUT".to_owned(), "45s".to_owned()),
        ("OFFPRINT_WAIT_UNTIL".to_owned(), "network-idle".to_owned()),
        ("OFFPRINT_DELAY".to_owned(), "750ms".to_owned()),
        ("OFFPRINT_MISSING_RESOURCES".to_owned(), "fail".to_owned()),
        ("OFFPRINT_SCOPE".to_owned(), "selection".to_owned()),
        ("OFFPRINT_SELECTOR".to_owned(), "main article".to_owned()),
        ("OFFPRINT_REMOVE_UNUSED_CSS".to_owned(), "true".to_owned()),
        (
            "OFFPRINT_REMOVE_UNUSED_FONTS".to_owned(),
            "false".to_owned(),
        ),
        (
            "OFFPRINT_REMOVE_HIDDEN_ELEMENTS".to_owned(),
            "true".to_owned(),
        ),
        ("OFFPRINT_VERIFY".to_owned(), "static".to_owned()),
        ("OFFPRINT_NETWORK_POLICY".to_owned(), "server".to_owned()),
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
    profile.readiness.lazy_load = LazyLoadPolicy::Disabled;
    profile.readiness.delay = Milliseconds::new(750);
    profile.content.missing_resources = MissingResourcePolicy::Fail;
    profile.content.scope = CaptureScope::Page;
    profile.content.selector = Some("main article".to_owned());
    profile.content.optimizations.remove_unused_css = true;
    profile.content.optimizations.remove_unused_fonts = false;
    profile.content.optimizations.remove_hidden_elements = true;
    profile.verification = VerificationMode::Static;
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
        Some(offprint::ErrorCode::from_static("offprint.config.profile"))
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
fn system_browser_source_keeps_provisioning_external() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        source = "system"
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None))
        .and_then(Result::ok);

    assert_eq!(
        resolved.as_ref().map(|config| config.browser_source),
        Some(BrowserSourcePolicy::System)
    );
}

#[test]
fn browser_management_policy_reaches_the_resolved_configuration() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        source = "managed"
        installation = "install-managed"
        "#,
    );
    let resolved = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None))
        .and_then(Result::ok);

    assert_eq!(
        resolved.as_ref().map(|resolved| resolved.browser_source),
        Some(BrowserSourcePolicy::Managed)
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
            .and_then(|resolved| resolved.profile.content.selector.as_deref()),
        Some("main article")
    );
    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.profile.content.scope),
        Some(CaptureScope::Page)
    );
    assert!(resolved.as_ref().is_some_and(|resolved| {
        resolved.profile.content.optimizations.remove_unused_css
            && resolved.profile.content.optimizations.remove_unused_fonts
            && resolved
                .profile
                .content
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
    environment.insert("OFFPRINT_LOCALE".to_owned(), "en-GB".to_owned());
    environment.insert("OFFPRINT_TIMEOUT".to_owned(), "45s".to_owned());
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
            && matches!(
                &resolved.browser,
                BrowserSelection::Executable(path) if path == "/opt/chromium"
            )
    }));
}

#[test]
fn cdp_flag_replaces_the_configured_path_in_diagnostics() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [browser]
        path = "/opt/chromium"
        "#,
    );
    let mut resolved = explicit
        .ok()
        .map(|explicit| resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None))
        .and_then(Result::ok);

    let applied = resolved
        .as_mut()
        .map(|resolved| resolved.apply_cdp_url_flag("http://127.0.0.1:9222"));

    assert!(applied.is_some_and(|result| result.is_ok()));
    assert!(resolved.as_ref().is_some_and(|resolved| {
        !resolved.configuration.contains_key("browser.path")
            && matches!(&resolved.browser, BrowserSelection::Remote(_))
            && resolved
                .configuration
                .get("browser.cdpUrl")
                .is_some_and(|value| value.provenance == ConfigProvenance::Flag && value.redacted)
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

#[cfg(unix)]
#[test]
fn unrelated_non_unicode_environment_value_is_ignored() {
    use std::ffi::OsString;

    let result = super::resolve::collect_environment([(
        OsString::from("UNRELATED"),
        super::resolve::non_unicode(b"\xff"),
    )]);

    assert_eq!(result, Ok(BTreeMap::new()));
}

#[test]
fn unknown_offprint_environment_variable_is_rejected() {
    use std::ffi::OsString;

    let result = super::resolve::collect_environment([(
        OsString::from("OFFPRINT_UNKNOWN_SETTING"),
        OsString::from("value"),
    )]);

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.config.field")
    );
}

#[cfg(unix)]
#[test]
fn offprint_environment_value_requires_unicode() {
    use std::ffi::OsString;

    let result = super::resolve::collect_environment([(
        OsString::from("OFFPRINT_PROFILE"),
        super::resolve::non_unicode(b"\xff"),
    )]);

    assert!(result.is_err_and(|error| error.code.as_str() == "offprint.config.value"));
}

#[test]
fn duration_overflow_is_a_validation_result() {
    let value = format!("{:.0}h", f64::MAX / 2.0);

    assert_eq!(super::value::parse_duration_text(&value), None);
}

#[test]
fn every_byte_limit_updates_the_profile_and_provenance() {
    let explicit = toml::from_str::<ConfigFile>(
        r#"
        [profile.default.limits]
        resource_bytes = "1KiB"
        total_resource_bytes = "2KiB"
        collector_chunk_bytes = "3KiB"
        artifact_bytes = "4KiB"
        "#,
    );
    let resolved = explicit.ok().and_then(|explicit| {
        resolve_documents(ConfigFile::default(), explicit, &BTreeMap::new(), None).ok()
    });
    let resolved = resolved.as_ref();

    assert_eq!(
        resolved.map(|resolved| (
            resolved.profile.limits.resource_bytes,
            resolved.profile.limits.total_resource_bytes,
            resolved.profile.limits.collector_chunk_bytes,
            resolved.profile.limits.artifact_bytes,
        )),
        Some((1024, 2048, 3072, 4096))
    );
    for field in [
        "limits.resourceBytes",
        "limits.totalResourceBytes",
        "limits.collectorChunkBytes",
        "limits.artifactBytes",
    ] {
        assert_eq!(
            resolved
                .and_then(|resolved| resolved.configuration.get(field))
                .map(|value| value.provenance),
            Some(ConfigProvenance::Profile),
            "{field}"
        );
    }
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
        Some("offprint.input.limit")
    );
    assert_eq!(
        rejected
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(MAXIMUM_CAPTURE_NODES))
    );
}
