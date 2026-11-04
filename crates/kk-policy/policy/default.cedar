permit (principal, action == Action::"read.artifact", resource)
when {
  resource.level <= principal.clearance_level &&
  principal.compartments.containsAll(resource.compartments) &&
  !resource.sensitivity_unknown
};

permit (principal, action == Action::"write.artifact", resource)
when { principal.kind == "AGENT" || principal.kind == "PARSER" };

permit (principal, action == Action::"model.complete", resource)
when {
  resource.class == "device_only" ||
  (resource.class == "verified_tee" && resource.tee_verified)
};

forbid (principal, action == Action::"model.complete", resource)
when { context.taint_level >= 2 && resource.class != "device_only" && resource.class != "verified_tee" };

forbid (principal, action == Action::"model.complete", resource)
when { resource.class == "verified_tee" && !resource.tee_verified && context.taint_level >= 1 };

permit (principal, action == Action::"tool.call", resource)
when { principal.allowed_tools.contains(resource) && resource.readonly };

permit (principal, action == Action::"tool.call", resource)
when { principal.allowed_tools.contains(resource) && !resource.readonly && context.human_approved };

forbid (principal, action == Action::"tool.call", resource)
when { !resource.readonly && context.taint_untrusted && !context.human_approved };

permit (principal, action == Action::"release.propose", resource)
when { principal.kind == "AGENT" };
