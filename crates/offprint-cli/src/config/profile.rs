use offprint::{
    CaptureProfile, CaptureScope, ConfigProvenance, LazyLoadPolicy, Milliseconds, NetworkPolicy,
    Result, ViewportSweepPolicy,
};
use serde_json::Value;

use super::document::{
    EnvironmentConfig, LazyLoadName, LimitsConfig, OptimizationConfig, ProfilePatch,
    ReadinessConfig, SimpleNetworkPolicy,
};
use super::value::{parse_bytes, parse_config_duration};
use super::{ResolvedConfig, record, set_readiness_mode};

pub(super) fn apply_builtin_profile(resolved: &mut ResolvedConfig, profile: CaptureProfile) {
    let defaults = CaptureProfile::default();
    resolved.profile = profile;
    if resolved.profile.content.missing_resources != defaults.content.missing_resources {
        let value =
            serde_json::to_value(resolved.profile.content.missing_resources).unwrap_or(Value::Null);
        record(
            resolved,
            "missingResources",
            value,
            ConfigProvenance::Profile,
            false,
        );
    }
    if resolved.profile.network != defaults.network {
        let value = network_policy_name(&resolved.profile.network);
        record(
            resolved,
            "networkPolicy",
            Value::String(value.to_owned()),
            ConfigProvenance::Profile,
            false,
        );
    }
}

pub(super) fn apply_profile_patch(
    resolved: &mut ResolvedConfig,
    profile: &ProfilePatch,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(verification) = profile.verification {
        resolved.profile.verification = verification;
        record(
            resolved,
            "verification",
            serde_json::to_value(verification).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(missing) = profile.missing_resources {
        resolved.profile.content.missing_resources = missing;
        record(
            resolved,
            "missingResources",
            serde_json::to_value(missing).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(network) = profile.network_policy {
        resolved.profile.network = simple_network_policy(network);
        record(
            resolved,
            "networkPolicy",
            serde_json::to_value(network).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(preserve) = profile.preserve_password_values {
        resolved.profile.content.preserve_password_values = preserve;
        record(
            resolved,
            "preservePasswordValues",
            Value::Bool(preserve),
            provenance,
            false,
        );
    }
    if let Some(scope) = profile.scope {
        resolved.profile.content.scope = scope;
        resolved.profile.content.selector = None;
        record(
            resolved,
            "scope",
            serde_json::to_value(scope).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(selector) = &profile.selector {
        resolved.profile.content.scope = CaptureScope::Page;
        resolved.profile.content.selector = Some(selector.clone());
        record(
            resolved,
            "selector",
            Value::String(selector.clone()),
            provenance,
            false,
        );
    }
    apply_optimizations(resolved, &profile.optimizations, provenance);
    if let Some(roots) = &profile.allowed_file_roots {
        resolved.profile.content.allowed_file_roots =
            roots.iter().map(|root| root.as_str().into()).collect();
        record(
            resolved,
            "allowedFileRoots",
            Value::Array(roots.iter().cloned().map(Value::String).collect()),
            provenance,
            false,
        );
    }
    apply_profile_environment(resolved, &profile.environment, provenance);
    apply_readiness(resolved, &profile.readiness, provenance)?;
    apply_limits(resolved, &profile.limits, provenance)
}

fn apply_optimizations(
    resolved: &mut ResolvedConfig,
    optimizations: &OptimizationConfig,
    provenance: ConfigProvenance,
) {
    if let Some(value) = optimizations.remove_unused_css {
        resolved.profile.content.optimizations.remove_unused_css = value;
        record(
            resolved,
            "optimizations.removeUnusedCss",
            Value::Bool(value),
            provenance,
            false,
        );
    }
    if let Some(value) = optimizations.remove_unused_fonts {
        resolved.profile.content.optimizations.remove_unused_fonts = value;
        record(
            resolved,
            "optimizations.removeUnusedFonts",
            Value::Bool(value),
            provenance,
            false,
        );
    }
    if let Some(value) = optimizations.remove_hidden_elements {
        resolved
            .profile
            .content
            .optimizations
            .remove_hidden_elements = value;
        record(
            resolved,
            "optimizations.removeHiddenElements",
            Value::Bool(value),
            provenance,
            false,
        );
    }
}

fn apply_profile_environment(
    resolved: &mut ResolvedConfig,
    environment: &EnvironmentConfig,
    provenance: ConfigProvenance,
) {
    if let Some(viewport) = environment.viewport {
        resolved.profile.environment.viewport = viewport;
        record(
            resolved,
            "environment.viewport",
            serde_json::to_value(viewport).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(locale) = &environment.locale {
        resolved.profile.environment.locale = locale.clone();
        record(
            resolved,
            "environment.locale",
            Value::String(locale.clone()),
            provenance,
            false,
        );
    }
    if let Some(timezone) = &environment.timezone {
        resolved.profile.environment.timezone = timezone.clone();
        record(
            resolved,
            "environment.timezone",
            Value::String(timezone.clone()),
            provenance,
            false,
        );
    }
    if let Some(color_scheme) = environment.color_scheme {
        resolved.profile.environment.color_scheme = color_scheme;
        record(
            resolved,
            "environment.colorScheme",
            serde_json::to_value(color_scheme).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(reduced_motion) = environment.reduced_motion {
        resolved.profile.environment.reduced_motion = reduced_motion;
        record(
            resolved,
            "environment.reducedMotion",
            serde_json::to_value(reduced_motion).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
}

fn apply_readiness(
    resolved: &mut ResolvedConfig,
    readiness: &ReadinessConfig,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(mode) = readiness.mode {
        set_readiness_mode(&mut resolved.profile.readiness, mode);
        record(
            resolved,
            "readiness.mode",
            serde_json::to_value(mode).unwrap_or(Value::Null),
            provenance,
            false,
        );
    }
    if let Some(value) = &readiness.network_quiet {
        resolved.profile.readiness.network_quiet =
            Milliseconds::from(parse_config_duration(value)?);
        let milliseconds = resolved.profile.readiness.network_quiet.get();
        record(
            resolved,
            "readiness.networkQuiet",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    if let Some(value) = &readiness.mutation_quiet {
        resolved.profile.readiness.mutation_quiet =
            Milliseconds::from(parse_config_duration(value)?);
        let milliseconds = resolved.profile.readiness.mutation_quiet.get();
        record(
            resolved,
            "readiness.mutationQuiet",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    if let Some(value) = &readiness.delay {
        resolved.profile.readiness.delay = Milliseconds::from(parse_config_duration(value)?);
        let milliseconds = resolved.profile.readiness.delay.get();
        record(
            resolved,
            "readiness.delay",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    if let Some(lazy_load) = readiness.lazy_load {
        resolved.profile.readiness.lazy_load = match lazy_load {
            LazyLoadName::Disabled => LazyLoadPolicy::Disabled,
            LazyLoadName::ViewportSweep => {
                LazyLoadPolicy::ViewportSweep(ViewportSweepPolicy::default())
            }
        };
        let value = match lazy_load {
            LazyLoadName::Disabled => "disabled",
            LazyLoadName::ViewportSweep => "viewport-sweep",
        };
        record(
            resolved,
            "readiness.lazyLoad",
            Value::String(value.to_owned()),
            provenance,
            false,
        );
    }
    Ok(())
}

fn apply_limits(
    resolved: &mut ResolvedConfig,
    limits: &LimitsConfig,
    provenance: ConfigProvenance,
) -> Result<()> {
    if let Some(value) = &limits.duration {
        resolved.profile.limits.duration = Milliseconds::from(parse_config_duration(value)?);
        let milliseconds = resolved.profile.limits.duration.get();
        record(
            resolved,
            "limits.duration",
            Value::from(milliseconds),
            provenance,
            false,
        );
    }
    macro_rules! set_limit {
        ($field:ident, $name:literal) => {
            if let Some(value) = limits.$field {
                resolved.profile.limits.$field = value;
                record(
                    resolved,
                    concat!("limits.", $name),
                    Value::from(value),
                    provenance,
                    false,
                );
            }
        };
    }
    macro_rules! set_byte_limit {
        ($field:ident, $name:literal) => {
            if let Some(value) = &limits.$field {
                let bytes = parse_bytes(value)?;
                resolved.profile.limits.$field = bytes;
                record(
                    resolved,
                    concat!("limits.", $name),
                    Value::from(bytes),
                    provenance,
                    false,
                );
            }
        };
    }
    set_limit!(redirects, "redirects");
    set_limit!(frames, "frames");
    set_limit!(nodes, "nodes");
    set_limit!(resources, "resources");
    set_limit!(concurrent_resources, "concurrentResources");
    set_limit!(css_import_depth, "cssImportDepth");
    set_limit!(frame_depth, "frameDepth");
    set_byte_limit!(resource_bytes, "resourceBytes");
    set_byte_limit!(total_resource_bytes, "totalResourceBytes");
    set_byte_limit!(collector_chunk_bytes, "collectorChunkBytes");
    set_byte_limit!(artifact_bytes, "artifactBytes");
    Ok(())
}

fn simple_network_policy(value: SimpleNetworkPolicy) -> NetworkPolicy {
    match value {
        SimpleNetworkPolicy::Standard => NetworkPolicy::Standard,
        SimpleNetworkPolicy::Server => NetworkPolicy::Server,
        SimpleNetworkPolicy::Unrestricted => NetworkPolicy::Unrestricted,
    }
}

pub(super) fn network_policy_name(value: &NetworkPolicy) -> &'static str {
    match value {
        NetworkPolicy::Standard => "standard",
        NetworkPolicy::Server => "server",
        NetworkPolicy::Unrestricted => "unrestricted",
        NetworkPolicy::Custom(_) => "custom",
    }
}
